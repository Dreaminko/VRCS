use super::{CloudEvent, MAX_TRANSCRIPT_BYTES};
use std::collections::VecDeque;
use tokio::time::Instant;

use crate::asr::streaming::LiveTranslationResult;
use crate::config::AsrConfig;
use crate::models::LiveTranslation;

mod timed;
pub(in crate::asr::streaming) use timed::append_delta;

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
    // Gemini target-language speech has no translated stream. Keep it apart
    // from foreign speech whose native translation can still be arriving.
    pub(super) source_only: Option<Box<State>>,
    pub(super) source_only_input: bool,
    pub(super) source_only_active: bool,
    timing: Option<timed::Timing>,
}

pub(super) fn merge_events(
    before: Option<CloudEvent>,
    next: Option<CloudEvent>,
) -> Option<CloudEvent> {
    match (before, next) {
        (
            Some(CloudEvent::LiveTranslation {
                completed: mut before,
                translations: mut old,
                ..
            }),
            Some(CloudEvent::LiveTranslation {
                service,
                snapshot,
                completed,
                translations,
            }),
        ) => {
            before.extend(completed);
            old.extend(translations);
            Some(CloudEvent::LiveTranslation {
                service,
                snapshot,
                completed: before,
                translations: old,
            })
        }
        (before, next) => next.or(before),
    }
}

pub(super) fn result(
    config: &AsrConfig,
    transcript: LiveTranslation,
    pending: bool,
) -> LiveTranslationResult {
    let source_utterance_ids = vec![transcript.utterance_id.clone()];
    LiveTranslationResult {
        pending,
        source_utterance_ids,
        transcript,
        provider: config
            .api_profiles
            .iter()
            .find(|profile| config.active_profile_id.as_deref() == Some(profile.id.as_str()))
            .map(|profile| profile.provider.as_str())
            .unwrap_or_else(|| {
                crate::providers::recognition_service(&config.backend)
                    .unwrap()
                    .0
            })
            .into(),
        model: config.service_settings[&config.backend].model.clone(),
    }
}

impl State {
    pub(super) fn source_len(&self) -> usize {
        self.input.len()
            + self
                .source_only
                .as_ref()
                .map_or(0, |state| state.source_len())
    }

    pub(super) fn active_preview(&self, mut event: Option<CloudEvent>) -> Option<CloudEvent> {
        let active = if self.source_only_active {
            self.source_only
                .as_deref()
                .and_then(|state| state.snapshot.as_ref())
        } else {
            self.snapshot.as_ref()
        };
        if let (Some(CloudEvent::LiveTranslation { snapshot, .. }), Some(active)) =
            (&mut event, active)
        {
            *snapshot = active.clone();
        }
        event
    }

    pub(super) fn collect(&mut self, config: &AsrConfig, flush: bool) -> Option<CloudEvent> {
        let mut timing = self.timing.take()?;
        let event = timing.collect(self, config, flush);
        self.timing = Some(timing);
        event
    }
}

pub(super) fn poll(config: &AsrConfig, state: &mut State) -> Option<CloudEvent> {
    let translated = state.collect(config, false);
    let original = state
        .source_only
        .as_mut()
        .and_then(|state| state.collect(config, false));
    state.active_preview(merge_events(translated, original))
}

pub(super) fn finish(config: &AsrConfig, state: &mut State) -> Option<CloudEvent> {
    let translated = state.collect(config, true);
    let original = state
        .source_only
        .as_mut()
        .and_then(|state| state.collect(config, true));
    let event = merge_events(translated, original);
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
        source_utterance_id: None,
        conversation_preview: None,
        speaker: None,
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
