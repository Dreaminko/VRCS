use super::{CloudEvent, MAX_TRANSCRIPT_BYTES};
use std::collections::VecDeque;
use tokio::time::Instant;

use crate::asr::streaming::LiveTranslationResult;
use crate::config::AsrConfig;
use crate::models::LiveTranslation;

mod timed;
pub(in crate::asr::streaming) use timed::{append_delta, apply, confirm, window};

pub(super) const MAX_DISPLAY_CHARS: usize = 160;

pub(super) fn append_display_text(current: &mut String, delta: &str) {
    if delta.is_empty() {
        return;
    }
    current.push_str(delta);

    let excess = current.chars().count().saturating_sub(MAX_DISPLAY_CHARS);
    if excess > 0 {
        let cut = current.char_indices().nth(excess).unwrap().0;
        // Prefer a word or clause boundary within the retained window.
        let cut = current[cut..]
            .char_indices()
            .find_map(|(index, c)| {
                let end = cut + index + c.len_utf8();
                ((c.is_whitespace() || matches!(c, ',' | '，' | ';' | '；'))
                    && !current[end..].trim().is_empty())
                .then_some(end)
            })
            .unwrap_or(cut);
        current.drain(..cut);
    }
    let leading = current.len() - current.trim_start().len();
    current.drain(..leading);
}

#[derive(Default)]
pub(super) struct State {
    pub(super) snapshot: Option<LiveTranslation>,
    pub(super) input: String,
    pub(super) output: String,
    pub(super) language: Option<String>,
    timing: Option<timed::Timing>,
}

fn terminal(c: char) -> bool {
    matches!(
        c,
        '.' | '。'
            | '．'
            | '｡'
            | '!'
            | '！'
            | '?'
            | '？'
            | '؟'
            | '‼'
            | '⁇'
            | '⁈'
            | '⁉'
            | '\n'
            | '\r'
    )
}

fn closing(c: char) -> bool {
    matches!(
        c,
        '"' | '\'' | '”' | '’' | '」' | '』' | ')' | '）' | ']' | '】' | '》' | '»'
    )
}

fn has_content(text: &str) -> bool {
    text.chars().any(|c| {
        !c.is_whitespace()
            && !terminal(c)
            && !closing(c)
            && !matches!(
                c,
                '“' | '‘' | '「' | '『' | '(' | '（' | '[' | '【' | '《' | '«'
            )
    })
}

fn result(config: &AsrConfig, transcript: LiveTranslation, pending: bool) -> LiveTranslationResult {
    let source_utterance_ids = vec![transcript.utterance_id.clone()];
    LiveTranslationResult {
        pending,
        source_utterance_ids,
        transcript,
        provider: crate::providers::recognition_service(&config.backend)
            .unwrap()
            .0
            .into(),
        model: config.service_settings[&config.backend].model.clone(),
    }
}

impl State {
    fn collect(&mut self, config: &AsrConfig, flush: bool) -> Option<CloudEvent> {
        let mut timing = self.timing.take()?;
        let event = timing.collect(self, config, flush);
        self.timing = Some(timing);
        event
    }
}

pub(super) fn poll(config: &AsrConfig, state: &mut State) -> Option<CloudEvent> {
    state.collect(config, false)
}

pub(super) fn finish(config: &AsrConfig, state: &mut State) -> Option<CloudEvent> {
    let event = state.collect(config, true);
    *state = State::default();
    event
}

pub(super) fn append(
    config: &AsrConfig,
    state: &mut State,
    text: &str,
    translation: &str,
) -> Result<Option<CloudEvent>, String> {
    if text.is_empty() && translation.is_empty() {
        return Ok(None);
    }
    if state.input.len() + text.len() > MAX_TRANSCRIPT_BYTES
        || state.output.len() + translation.len() > MAX_TRANSCRIPT_BYTES
    {
        return Err("Live translation transcript limit reached; restart recognition".into());
    }
    let snapshot = state.snapshot.get_or_insert_with(|| LiveTranslation {
        utterance_id: format!("live-{}", uuid::Uuid::new_v4()),
        text: String::new(),
        language: None,
        translation: String::new(),
        target_language: config.live_translation_target.clone().unwrap_or_default(),
    });
    if snapshot.text.is_empty() && snapshot.translation.is_empty() {
        snapshot.utterance_id = format!("live-{}", uuid::Uuid::new_v4());
    }
    snapshot.language = state.language.clone();
    state.input.push_str(text);
    state.output.push_str(translation);
    Ok(state.collect(config, false))
}
