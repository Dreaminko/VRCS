use super::*;
use crate::asr::streaming::alignment;
use std::time::Duration;

const SETTLE: Duration = Duration::from_millis(450);
const TAIL_IDLE: Duration = Duration::from_millis(1500);

#[derive(Default)]
pub(super) struct Timing {
    last_input: Option<Instant>,
    last_output: Option<Instant>,
    context: VecDeque<(String, String)>,
    last_committed: Option<LiveTranslation>,
    continuation_bytes: usize,
}

pub(in crate::asr::streaming) fn append_delta(
    config: &AsrConfig,
    state: &mut State,
    text: &str,
    translation: &str,
    _elapsed_ms: Option<u64>,
    _event_id: Option<&str>,
) -> Result<Option<CloudEvent>, String> {
    if text.is_empty() && translation.is_empty() {
        return Ok(None);
    }
    let timing = state.timing.get_or_insert_with(Timing::default);
    if text.is_empty() && !translation.is_empty() && state.input.trim().is_empty() {
        if let Some(previous) = &timing.last_committed {
            let continuation_bytes = state.output.len() + translation.len();
            if previous.translation.len() + continuation_bytes > MAX_TRANSCRIPT_BYTES {
                return Err(
                    "Live translation transcript limit reached; restart recognition".into(),
                );
            }
            // Bind the continuation when it arrives. A new source can arrive
            // before the output settles, but must never consume this prefix.
            timing.continuation_bytes = continuation_bytes;
        }
    }
    if !text.is_empty() {
        timing.last_input = Some(Instant::now());
    }
    if !translation.is_empty() {
        timing.last_output = Some(Instant::now());
    }
    append(config, state, text, translation)
}

impl Timing {
    pub(super) fn collect(
        &mut self,
        state: &mut State,
        config: &AsrConfig,
        flush: bool,
    ) -> Option<CloudEvent> {
        let mut snapshot = state.snapshot.clone()?;
        snapshot.language = state.language.clone();
        let same_language = config.backend == crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE
            && snapshot.language.as_deref().is_some_and(|language| {
                crate::providers::same_live_translation_language(
                    language,
                    &snapshot.target_language,
                )
            })
            && state.output.trim().is_empty();
        let settled =
            |last: Option<Instant>, delay| last.is_some_and(|time| time.elapsed() >= delay);
        let mut translations = Vec::new();
        if self.continuation_bytes > 0
            && (flush || !state.input.trim().is_empty() || settled(self.last_output, SETTLE))
        {
            let end = std::mem::take(&mut self.continuation_bytes);
            let continuation: String = state.output.drain(..end).collect();
            if let Some(previous) = &mut self.last_committed {
                previous.translation.push_str(&continuation);
                if let Some(context) = self.context.back_mut() {
                    context.1 = previous.translation.clone();
                }
                translations.push(result(config, previous.clone(), false));
            }
        }
        let idle = settled(self.last_input, TAIL_IDLE) && settled(self.last_output, TAIL_IDLE);
        let unpunctuated = alignment::sentences(&state.input).is_empty()
            && alignment::sentences(&state.output).is_empty();
        let mut cuts = Vec::new();
        // Gemini intentionally does not echo target-language speech. Complete
        // source-only rows without waiting for output or preview preferences.
        let source_boundary = alignment::sentences(&state.input)
            .last()
            .copied()
            .unwrap_or(0);
        let source_has_lookahead =
            source_boundary > 0 && source_boundary < state.input.trim_end().len();
        if same_language && (source_has_lookahead || settled(self.last_input, SETTLE)) {
            let source_end = if settled(self.last_input, TAIL_IDLE) {
                state.input.len()
            } else {
                source_boundary
            };
            if source_end > 0 {
                cuts.push((source_end, 0));
            }
        } else if settled(self.last_output, SETTLE) {
            cuts = alignment::align(
                &state.input,
                &state.output,
                &self.context.iter().cloned().collect::<Vec<_>>(),
                idle && unpunctuated,
            );
        }
        if !state.input.trim().is_empty()
            && !state.output.trim().is_empty()
            && settled(self.last_input, Duration::from_secs(5))
            && settled(self.last_output, Duration::from_secs(5))
        {
            // Length/number heuristics can be inconclusive (e.g. spelled-out
            // numbers). Commit a coarser group after both streams settle.
            cuts = vec![(state.input.len(), state.output.len())];
        }
        let fallback = flush || state.input.len() + state.output.len() >= MAX_TRANSCRIPT_BYTES / 2;
        if fallback && !state.input.trim().is_empty() {
            // Close and memory pressure preserve all remaining native text once.
            cuts.clear();
            cuts.push((state.input.len(), state.output.len()));
        }
        let mut completed = Vec::new();
        let (mut source_start, mut target_start) = (0, 0);
        for &(source_end, target_end) in &cuts {
            let mut source = snapshot.clone();
            source.utterance_id = format!("live-source-{}", uuid::Uuid::new_v4());
            source.text = state.input[source_start..source_end].to_owned();
            source.translation.clear();
            if !source.text.trim().is_empty() {
                completed.push(result(config, source.clone(), !same_language));
                source.translation = state.output[target_start..target_end].to_owned();
                if !same_language {
                    self.context
                        .push_back((source.text.clone(), source.translation.clone()));
                    while self.context.len() > 2 {
                        self.context.pop_front();
                    }
                    self.last_committed = Some(source.clone());
                    translations.push(result(config, source, false));
                }
            }
            source_start = source_end;
            target_start = target_end;
        }
        state.input.drain(..source_start);
        state.output.drain(..target_start);
        if flush && state.input.trim().is_empty() && !state.output.trim().is_empty() {
            // A late native continuation can outlive its original. Preserve it
            // as a revision of the last group rather than dropping it at close.
            if let Some(previous) = &mut self.last_committed {
                previous
                    .translation
                    .push_str(&std::mem::take(&mut state.output));
                translations.push(result(config, previous.clone(), false));
            }
        }
        snapshot.text.clear();
        snapshot.translation.clear();
        if !flush {
            append_display_text(&mut snapshot.text, &state.input);
            append_display_text(&mut snapshot.translation, &state.output);
        }
        let changed = state.snapshot.as_ref() != Some(&snapshot);
        state.snapshot = Some(snapshot.clone());
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

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> AsrConfig {
        AsrConfig {
            backend: crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE.into(),
            live_translation_target: Some("ja".into()),
            ..Default::default()
        }
    }
    fn append_text(state: &mut State, source: &str, target: &str) {
        append_delta(&config(), state, source, target, None, None).unwrap();
    }
    #[tokio::test(start_paused = true)]
    async fn locally_completes_without_a_profile_or_model() {
        let config = config();
        let mut state = State::default();
        append_delta(
            &config,
            &mut state,
            "Hello. Next.",
            "こんにちは。次。",
            None,
            None,
        )
        .unwrap();
        tokio::time::advance(SETTLE).await;
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            snapshot,
            ..
        }) = poll(&config, &mut state)
        else {
            panic!()
        };
        assert!(!completed.is_empty());
        assert_eq!(
            completed
                .iter()
                .map(|r| r.transcript.text.as_str())
                .collect::<String>(),
            "Hello. Next."
        );
        assert_eq!(
            translations
                .iter()
                .map(|r| r.transcript.translation.as_str())
                .collect::<String>(),
            "こんにちは。次。"
        );
        assert!(snapshot.text.is_empty() && snapshot.translation.is_empty());
        assert!(poll(&config, &mut state).is_none());
    }
    #[tokio::test(start_paused = true)]
    async fn numeric_continuation_is_retained_until_it_arrives() {
        let mut state = State::default();
        let source = "In 2020 there were 288 teams and 3487 tasks.";
        append_text(&mut state, source, "2020年に開始。288組。");
        tokio::time::advance(TAIL_IDLE).await;
        assert!(poll(&config(), &mut state).is_none());
        append_text(&mut state, " Next.", "3487件。次。");
        tokio::time::advance(SETTLE).await;
        let Some(CloudEvent::LiveTranslation { translations, .. }) = poll(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.text, format!("{source} "));
        assert_eq!(
            translations[0].transcript.translation,
            "2020年に開始。288組。3487件。"
        );
    }
    #[tokio::test(start_paused = true)]
    async fn same_language_completes_without_output() {
        let mut config = config();
        config.backend = crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE.into();
        let mut state = State {
            language: Some("ja".into()),
            ..Default::default()
        };
        let event = append_delta(&config, &mut state, "こんにちは。続き", "", None, None).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = event
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "こんにちは。");
        assert!(!completed[0].pending);
        assert!(translations.is_empty());
        tokio::time::advance(TAIL_IDLE).await;
        let Some(CloudEvent::LiveTranslation { completed, .. }) = poll(&config, &mut state) else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "続き");
        assert!(finish(&config, &mut state).is_none());
    }
    #[test]
    fn close_and_memory_pressure_preserve_whitespace_unicode_and_missing_translation() {
        let mut state = State::default();
        append_text(&mut state, " Hello🙂.\n Next.", " こんにちは。\n 次。 ");
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, " Hello🙂.\n Next.");
        assert_eq!(
            translations[0].transcript.translation,
            " こんにちは。\n 次。 "
        );
        assert!(finish(&config(), &mut state).is_none());
        append_text(&mut state, "source", "");
        let Some(CloudEvent::LiveTranslation { translations, .. }) = finish(&config(), &mut state)
        else {
            panic!()
        };
        assert!(!translations[0].pending);
        assert!(translations[0].transcript.translation.is_empty());
        let Some(CloudEvent::LiveTranslation { completed, .. }) = append_delta(
            &config(),
            &mut state,
            &"甲".repeat(MAX_TRANSCRIPT_BYTES / 6 + 1),
            "",
            None,
            None,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(
            completed[0].transcript.text.chars().count(),
            MAX_TRANSCRIPT_BYTES / 6 + 1
        );
    }
    #[tokio::test(start_paused = true)]
    async fn late_translation_is_bound_before_the_next_source_arrives() {
        for wait_before_next_source in [Duration::ZERO, SETTLE] {
            let config = config();
            let mut state = State::default();
            append_delta(
                &config,
                &mut state,
                "Hello, how are you today?",
                "こんにちは。",
                None,
                None,
            )
            .unwrap();
            tokio::time::advance(SETTLE).await;
            let Some(CloudEvent::LiveTranslation { completed, .. }) = poll(&config, &mut state)
            else {
                panic!()
            };
            let first_id = completed[0].transcript.utterance_id.clone();
            append_delta(&config, &mut state, "", "今日はお元気ですか？", None, None).unwrap();
            tokio::time::advance(wait_before_next_source).await;
            let revision = poll(&config, &mut state);
            let next =
                append_delta(&config, &mut state, "Thank you very much.", "", None, None).unwrap();
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = merge_events(revision, next)
            else {
                panic!()
            };
            assert!(completed.is_empty());
            assert_eq!(translations.len(), 1);
            assert_eq!(translations[0].transcript.utterance_id, first_id);
            assert_eq!(
                translations[0].transcript.translation,
                "こんにちは。今日はお元気ですか？"
            );
            assert_eq!(state.input, "Thank you very much.");
            assert!(state.output.is_empty());
            append_delta(
                &config,
                &mut state,
                "",
                "本当にありがとうございます。",
                None,
                None,
            )
            .unwrap();
            tokio::time::advance(SETTLE).await;
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = poll(&config, &mut state)
            else {
                panic!()
            };
            assert_eq!(completed[0].transcript.text, "Thank you very much.");
            assert_eq!(
                translations[0].transcript.translation,
                "本当にありがとうございます。"
            );
            assert_ne!(translations[0].transcript.utterance_id, first_id);
            assert!(finish(&config, &mut state).is_none());
        }
    }
}
