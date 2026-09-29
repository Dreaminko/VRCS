use super::{CloudEvent, MAX_TRANSCRIPT_BYTES};
use std::{collections::VecDeque, time::Duration};
use tokio::time::Instant;

use crate::asr::streaming::LiveTranslationResult;
use crate::config::AsrConfig;
use crate::models::LiveTranslation;
use crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE;

mod timed;
pub(in crate::asr::streaming) use timed::{append_delta, apply, window};

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
    sources: VecDeque<LiveTranslation>,
    targets: VecDeque<String>,
    last_pair: Option<LiveTranslation>,
    last_partial: Option<(String, String)>,
    language: Option<String>,
    last_input: Option<Instant>,
    last_output: Option<Instant>,
    timing: Option<timed::Timing>,
}

const END_MARK_DELAY: Duration = Duration::from_millis(200);
const SOURCE_IDLE_DELAY: Duration = Duration::from_millis(1500);

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

// Delay an end mark at the delta boundary so decimals and closing punctuation can arrive.
fn sentence_end(text: &str, allow_trailing: bool) -> Option<usize> {
    let chars: Vec<_> = text.char_indices().collect();
    for (pos, &(index, c)) in chars.iter().enumerate() {
        if !terminal(c) {
            continue;
        }
        if c == '.' {
            let before = &text[..index];
            let next = chars.get(pos + 1).map(|(_, c)| *c);
            if before.chars().last().is_some_and(|c| c.is_ascii_digit())
                && next.is_some_and(|c| c.is_ascii_digit())
            {
                continue;
            }
            let word = before
                .split_whitespace()
                .last()
                .unwrap_or("")
                .to_ascii_lowercase();
            if matches!(
                word.as_str(),
                "mr" | "mrs" | "ms" | "dr" | "prof" | "sr" | "jr" | "e.g" | "i.e"
            ) || (word.len() == 1 && word.chars().all(|c| c.is_ascii_alphabetic()))
            {
                continue;
            }
        }
        let mut end = index + c.len_utf8();
        for &(i, c) in &chars[pos + 1..] {
            if terminal(c) || closing(c) || c.is_whitespace() {
                end = i + c.len_utf8();
            } else {
                break;
            }
        }
        if end < text.len() || allow_trailing {
            return Some(end);
        }
    }
    None
}

fn take_sentences(buffer: &mut String, updated_at: Option<Instant>, flush: bool) -> Vec<String> {
    let trailing = flush || updated_at.is_some_and(|last| last.elapsed() >= END_MARK_DELAY);
    let idle = flush || updated_at.is_some_and(|last| last.elapsed() >= SOURCE_IDLE_DELAY);
    let mut sentences = Vec::new();
    while let Some(end) = sentence_end(buffer, trailing)
        .or_else(|| (idle && !buffer.is_empty()).then_some(buffer.len()))
    {
        let text = buffer.drain(..end).collect::<String>().trim().to_owned();
        if has_content(&text) {
            sentences.push(text);
        }
    }
    sentences
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
        if let Some(mut timing) = self.timing.take() {
            let event = timing.collect(self, config, flush);
            self.timing = Some(timing);
            return event;
        }
        let mut snapshot = self.snapshot.clone()?;
        let same_language = config.backend == SERVICE_GEMINI_LIVE_TRANSLATE
            && self.language.as_deref().is_some_and(|language| {
                crate::translation::same_translation_language(language, &snapshot.target_language)
            });
        let mut completed = Vec::new();
        for text in take_sentences(&mut self.input, self.last_input, flush) {
            let mut transcript = snapshot.clone();
            transcript.utterance_id = format!("live-source-{}", uuid::Uuid::new_v4());
            transcript.text = text;
            transcript.translation.clear();
            if !same_language {
                self.sources.push_back(transcript.clone());
            }
            completed.push(result(config, transcript, !same_language));
        }
        self.targets
            .extend(take_sentences(&mut self.output, self.last_output, flush));
        let mut translations = Vec::new();
        // Both streams retain their own boundaries. Associate completed sentences in stream order.
        while !self.sources.is_empty() && !self.targets.is_empty() {
            let mut transcript = self.sources.pop_front().unwrap();
            transcript.translation = self.targets.pop_front().unwrap();
            self.last_pair = Some(transcript.clone());
            self.last_partial = None;
            translations.push(result(config, transcript, false));
        }
        let idle = self
            .last_input
            .into_iter()
            .chain(self.last_output)
            .all(|last| last.elapsed() >= SOURCE_IDLE_DELAY);
        // A translation may expand one source sentence into several sentences.
        // Keep any trailing target text on that source instead of dropping it at session end.
        if self.sources.is_empty() && self.input.trim().is_empty() && (flush || idle) {
            if let Some(last) = &mut self.last_pair {
                if !self.targets.is_empty() {
                    for text in self.targets.drain(..) {
                        last.translation.push(' ');
                        last.translation.push_str(&text);
                    }
                    if translations
                        .last()
                        .is_some_and(|update| update.transcript.utterance_id == last.utterance_id)
                    {
                        translations.pop();
                    }
                    translations.push(result(config, last.clone(), false));
                }
            } else if same_language {
                self.targets.clear();
            }
        }
        if flush {
            translations.extend(
                self.sources
                    .drain(..)
                    .map(|source| result(config, source, false)),
            );
            self.last_partial = None;
        } else if let Some(source) = self.sources.front().filter(|_| has_content(&self.output)) {
            let key = (source.utterance_id.clone(), self.output.trim().to_owned());
            if self.last_partial.as_ref() != Some(&key) {
                let mut transcript = source.clone();
                transcript.translation = key.1.clone();
                translations.push(result(config, transcript, true));
                self.last_partial = Some(key);
            }
        }
        snapshot.text.clear();
        snapshot.translation.clear();
        append_display_text(&mut snapshot.text, &self.input);
        if self.sources.is_empty() {
            for text in &self.targets {
                append_display_text(&mut snapshot.translation, text);
            }
            append_display_text(&mut snapshot.translation, &self.output);
        }
        if flush {
            snapshot.text.clear();
            snapshot.translation.clear();
        }
        let changed = self.snapshot.as_ref() != Some(&snapshot);
        self.snapshot = Some(snapshot.clone());
        (!completed.is_empty() || !translations.is_empty() || changed).then_some(
            CloudEvent::LiveTranslation {
                service: config.backend.clone(),
                snapshot,
                completed,
                translations,
            },
        )
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
    language: Option<&str>,
) -> Result<Option<CloudEvent>, String> {
    if text.is_empty() && translation.is_empty() {
        return Ok(None);
    }
    if state.input.len()
        + state
            .sources
            .iter()
            .map(|source| source.text.len())
            .sum::<usize>()
        + text.len()
        > MAX_TRANSCRIPT_BYTES
        || state.output.len()
            + state.targets.iter().map(String::len).sum::<usize>()
            + state
                .last_pair
                .as_ref()
                .map_or(0, |pair| pair.translation.len())
            + translation.len()
            > MAX_TRANSCRIPT_BYTES
    {
        return Err("Live translation transcript limit reached; restart recognition".into());
    }
    if let Some(language) = language {
        state.language = Some(language.to_owned());
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
    if !text.is_empty() {
        state.last_input = Some(Instant::now());
    }
    if !translation.is_empty() {
        state.last_output = Some(Instant::now());
    }
    Ok(state.collect(config, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AsrConfig {
        AsrConfig {
            backend: crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE.into(),
            live_translation_target: Some("zh-Hans".into()),
            ..Default::default()
        }
    }

    #[test]
    fn translated_sentences_are_published_before_the_rest_of_the_stream_finishes() {
        let mut state = State::default();
        append(&config(), &mut state, "Hello. How are you? More", "", None).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            append(&config(), &mut state, "", "你好。最近怎么样？还有", None).unwrap()
        else {
            panic!("complete translated sentences must not wait for global silence");
        };
        assert_eq!(translations.len(), 2);
        assert_eq!(translations[0].transcript.text, "Hello.");
        assert_eq!(translations[0].transcript.translation, "你好。");
        assert_eq!(translations[1].transcript.text, "How are you?");
        assert_eq!(translations[1].transcript.translation, "最近怎么样？");
        assert!(translations.iter().all(|update| !update.pending));
    }

    #[test]
    fn each_stream_uses_its_own_end_mark_timer() {
        for backend in [
            crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE,
            SERVICE_GEMINI_LIVE_TRANSLATE,
        ] {
            let config = AsrConfig {
                backend: backend.into(),
                ..config()
            };
            let mut state = State::default();
            append(&config, &mut state, "Hello.", "你好。", None).unwrap();
            state.last_input = Some(Instant::now() - END_MARK_DELAY);
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = poll(&config, &mut state)
            else {
                panic!()
            };
            assert_eq!(completed[0].transcript.text, "Hello.");
            assert!(translations[0].pending);
            let id = completed[0].transcript.utterance_id.clone();
            state.last_output = Some(Instant::now() - END_MARK_DELAY);
            let Some(CloudEvent::LiveTranslation { translations, .. }) = poll(&config, &mut state)
            else {
                panic!()
            };
            assert_eq!(translations[0].transcript.utterance_id, id);
            assert!(!translations[0].pending);
        }
    }

    #[test]
    fn late_translation_streams_into_the_saved_sentence_without_waiting_for_punctuation() {
        let mut state = State::default();
        let Some(CloudEvent::LiveTranslation { completed, .. }) =
            append(&config(), &mut state, "Hello. Next", "", None).unwrap()
        else {
            panic!()
        };
        let id = completed[0].transcript.utterance_id.clone();
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            append(&config(), &mut state, "", "你", None).unwrap()
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.utterance_id, id);
        assert_eq!(translations[0].transcript.translation, "你");
        assert!(translations[0].pending);
        assert!(poll(&config(), &mut state).is_none());
    }

    #[test]
    fn target_can_finish_before_source_and_repetitions_are_not_deduplicated() {
        let mut state = State::default();
        append(&config(), &mut state, "", "你好。你好。", None).unwrap();
        state.last_output = Some(Instant::now() - END_MARK_DELAY);
        poll(&config(), &mut state);
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            append(&config(), &mut state, "Hello. Hello. Tail", "", None).unwrap()
        else {
            panic!()
        };
        assert_eq!(translations.len(), 2);
        assert_ne!(
            translations[0].transcript.utterance_id,
            translations[1].transcript.utterance_id
        );
        assert!(translations
            .iter()
            .all(|update| update.transcript.translation == "你好。"));
    }

    #[test]
    fn stop_preserves_extra_target_sentences_and_reports_untranslated_sources() {
        let mut state = State::default();
        append(&config(), &mut state, "Hello.", "你好。还好吗？", None).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].transcript.translation, "你好。 还好吗？");
        assert!(finish(&config(), &mut state).is_none());
        append(&config(), &mut state, "No translation. Tail", "", None).unwrap();
        let Some(CloudEvent::LiveTranslation { translations, .. }) = finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(translations.len(), 2);
        assert!(translations
            .iter()
            .all(|update| update.transcript.translation.is_empty()));
    }

    #[test]
    fn continuous_source_finalizes_without_translation_or_global_silence() {
        let mut state = State::default();
        let mut originals = Vec::new();
        for text in ["Hello?", " Next!", " Value 3.", "14?", " Last sentence"] {
            if let Some(CloudEvent::LiveTranslation { completed, .. }) =
                append(&config(), &mut state, text, "", None).unwrap()
            {
                originals.extend(completed);
            }
        }
        assert_eq!(
            originals
                .iter()
                .map(|r| r.transcript.text.as_str())
                .collect::<Vec<_>>(),
            ["Hello?", "Next!", "Value 3.14?"]
        );
        assert!(originals.iter().all(|r| r.pending));
    }

    #[test]
    fn idle_flushes_each_unpunctuated_stream_independently() {
        let mut state = State::default();
        append(&config(), &mut state, "unfinished words", "未完", None).unwrap();
        state.last_input = Some(Instant::now() - SOURCE_IDLE_DELAY);
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = poll(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "unfinished words");
        assert!(translations[0].pending);
        state.last_output = Some(Instant::now() - SOURCE_IDLE_DELAY);
        let Some(CloudEvent::LiveTranslation { translations, .. }) = poll(&config(), &mut state)
        else {
            panic!()
        };
        assert!(!translations[0].pending);
        assert_eq!(translations[0].transcript.translation, "未完");
    }

    #[test]
    fn sentence_boundaries_preserve_decimals_abbreviations_and_closing_marks() {
        for (text, expected) in [
            ("Dr. Smith paid 3.14. Next", "Dr. Smith paid 3.14. "),
            ("Ready?! Go", "Ready?! "),
            ("你好！』继续", "你好！』"),
            ("Hello\r\nNext", "Hello\r\n"),
        ] {
            assert_eq!(&text[..sentence_end(text, false).unwrap()], expected);
        }
        assert_eq!(sentence_end("Value 3.", false), None);
    }
}
