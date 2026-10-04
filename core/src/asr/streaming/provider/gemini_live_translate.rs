use serde_json::{json, Value};

use crate::config::AsrConfig;
use crate::providers::SERVICE_GEMINI_LIVE_TRANSLATE;

use super::live_translation::State;
#[cfg(test)]
use super::{
    live_translation::{append_display_text, finish, MAX_DISPLAY_CHARS},
    MAX_TRANSCRIPT_BYTES,
};
use super::{service_settings, CloudEvent};

pub(super) fn setup(config: &AsrConfig) -> Result<Value, String> {
    let settings = service_settings(config, SERVICE_GEMINI_LIVE_TRANSLATE)?;
    let target = config
        .live_translation_target
        .as_deref()
        .ok_or("Select an automatic translation target for Gemini Live Translate")?;
    crate::providers::validate_live_translation_language(SERVICE_GEMINI_LIVE_TRANSLATE, target)?;
    Ok(json!({ "setup": {
        "model": format!("models/{}", settings.model.trim_start_matches("models/")),
        "inputAudioTranscription": {},
        "outputAudioTranscription": {},
        "generationConfig": {
            "responseModalities": ["AUDIO"],
            "translationConfig": {"targetLanguageCode": target, "echoTargetLanguage": false}
        }
    }}))
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    state: &mut State,
) -> Result<Option<CloudEvent>, String> {
    if let Some(error) = value.get("error") {
        return Err(error
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("Gemini Live Translate failed")
            .to_owned());
    }
    if value.get("goAway").is_some() {
        return Err("Gemini Live Translate is closing the session".into());
    }
    let Some(content) = value.get("serverContent") else {
        return Ok(None);
    };
    if content.get("interrupted").and_then(Value::as_bool) == Some(true) {
        *state = State::default();
        return Ok(Some(CloudEvent::Failed {
            utterance_id: None,
            reset_session: true,
            code: "asr.live_translation_interrupted".into(),
            detail: "Gemini Live Translate output was interrupted".into(),
        }));
    }
    let input = content.get("inputTranscription");
    let output = content.get("outputTranscription");
    let text = input
        .and_then(|v| v.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let translation = output
        .and_then(|v| v.get("text"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let language = input
        .and_then(|v| v.get("languageCode"))
        .and_then(Value::as_str)
        .or_else(|| (config.language != "auto").then_some(config.language.as_str()))
        .or_else(|| {
            if state.source_only_input {
                state
                    .source_only
                    .as_deref()
                    .and_then(|state| state.language.as_deref())
            } else {
                state.language.as_deref()
            }
        })
        .map(str::to_owned);
    let source_only = language
        .as_deref()
        .zip(config.live_translation_target.as_deref())
        .is_some_and(|(source, target)| {
            crate::providers::same_live_translation_language(source, target)
        });
    let mut event = None;
    if !text.is_empty() {
        state.source_only_input = source_only;
        state.source_only_active = source_only;
        if !source_only {
            // Finish any source-only tail before previewing the next foreign
            // source. Never flush a foreign source merely because language changes.
            event = state
                .source_only
                .as_mut()
                .and_then(|state| state.collect(config, true));
            state.language = language.clone();
        }
    } else if !translation.is_empty() {
        state.source_only_active = false;
    }
    let translated = super::live_translation::append_delta(
        config,
        state,
        if source_only { "" } else { text },
        translation,
        None,
        None,
    )?;
    event = super::live_translation::merge_events(event, translated);
    if source_only && !text.is_empty() {
        let original = state.source_only.get_or_insert_with(Default::default);
        original.language = language;
        let next = super::live_translation::append_delta(config, original, text, "", None, None)?;
        event = super::live_translation::merge_events(event, next);
    }
    if content["turnComplete"].as_bool() == Some(true)
        || input.and_then(|input| input["finished"].as_bool()) == Some(true)
    {
        // Only the independent source-only stream can complete without output.
        // Merge rather than overwrite any punctuation prefix completed above.
        let tail = state
            .source_only
            .as_mut()
            .and_then(|state| state.collect(config, true));
        event = super::live_translation::merge_events(event, tail);
    }
    Ok(state.active_preview(event))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AsrConfig {
        AsrConfig {
            backend: SERVICE_GEMINI_LIVE_TRANSLATE.into(),
            live_translation_target: Some("zh-Hant".into()),
            ..Default::default()
        }
    }

    #[test]
    fn setup_requires_target_and_requests_both_transcripts() {
        let value = setup(&config()).unwrap();
        assert_eq!(
            value.pointer("/setup/generationConfig/translationConfig/targetLanguageCode"),
            Some(&json!("zh-Hant"))
        );
        assert_eq!(
            value.pointer("/setup/generationConfig/responseModalities"),
            Some(&json!(["AUDIO"]))
        );
        assert!(value.pointer("/setup/inputAudioTranscription").is_some());
        assert!(value.pointer("/setup/outputAudioTranscription").is_some());
        assert!(setup(&AsrConfig::default()).is_err());
    }

    #[test]
    fn audio_and_turn_markers_do_not_drop_or_finalize_text() {
        let mut state = State::default();
        let first = normalize_event(
            &config(),
            &json!({"serverContent": {
                "modelTurn": {"parts": [{"inlineData": {"data": "AA=="}}]},
                "outputTranscription": {"text": "你好"}, "turnComplete": true
            }}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        let CloudEvent::LiveTranslation {
            snapshot: first, ..
        } = first
        else {
            panic!("expected snapshot")
        };
        assert_eq!(first.translation, "你好");
        assert!(first.text.is_empty());
        let second = normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription": {"text": "hello hello", "languageCode": "en"}
            }}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        let CloudEvent::LiveTranslation {
            snapshot: second, ..
        } = second
        else {
            panic!("expected snapshot")
        };
        assert_eq!(first.utterance_id, second.utterance_id);
        assert_eq!(second.text, "hello hello");
        assert_eq!(second.translation, "你好");
        assert!(normalize_event(
            &config(),
            &json!({"serverContent": {"generationComplete": true}}),
            &mut state
        )
        .unwrap()
        .is_none());
    }

    #[test]
    fn real_continuous_stream_previews_each_delta_and_preserves_native_text_exactly() {
        let messages: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/gemini_live_translate.json")).unwrap();
        let mut state = State::default();
        let mut visible_translations = 0;
        for message in messages {
            let translated_delta = message
                .pointer("/serverContent/outputTranscription/text")
                .and_then(Value::as_str)
                .is_some_and(|text| !text.is_empty());
            let event = normalize_event(&config(), &message, &mut state).unwrap();
            if let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                snapshot,
                ..
            }) = event
            {
                assert!(
                    completed.is_empty() && translations.is_empty(),
                    "no local timer may pair native sentences"
                );
                if translated_delta {
                    assert!(!snapshot.translation.is_empty());
                    visible_translations += 1;
                }
            } else {
                assert!(
                    !translated_delta,
                    "every translated delta must preview without waiting for alignment"
                );
            }
        }
        assert!(visible_translations > 0);
        assert_eq!(state.input, messages_text("inputTranscription"));
        assert_eq!(state.output, messages_text("outputTranscription"));
        let CloudEvent::LiveTranslation {
            completed,
            snapshot,
            translations,
            ..
        } = finish(&config(), &mut state).unwrap()
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(
            completed[0].transcript.text,
            messages_text("inputTranscription")
        );
        assert_eq!(translations.len(), 1);
        assert_eq!(
            translations[0].transcript.translation,
            messages_text("outputTranscription")
        );
        assert!(snapshot.text.is_empty() && snapshot.translation.is_empty());
        assert_eq!(snapshot.language.as_deref(), Some("en"));
    }

    fn messages_text(key: &str) -> String {
        let messages: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/gemini_live_translate.json")).unwrap();
        messages
            .iter()
            .filter_map(|v| v["serverContent"][key]["text"].as_str())
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn local_alignment_merges_gemini_sentence_counts_without_rewriting_text() {
        let source = "Platform 2.0 started in 2020, with 288 communities and 3487 tasks.";
        let translated = "平台2.0于2020年启动。共有288家社区。发布3487项任务。";
        let mut state = State::default();
        normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription":{"text":source,"languageCode":"en"},
                "outputTranscription":{"text":translated}
            }}),
            &mut state,
        )
        .unwrap();
        tokio::time::advance(std::time::Duration::from_millis(450)).await;
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = super::super::live_translation::poll(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(translations.len(), 1);
        assert_eq!(completed[0].transcript.text, source);
        assert_eq!(translations[0].transcript.translation, translated);
        assert_eq!(translations[0].transcript.language.as_deref(), Some("en"));
        assert_eq!(
            completed[0].transcript.utterance_id,
            translations[0].transcript.utterance_id
        );
        assert!(state.input.is_empty() && state.output.is_empty());
    }

    #[test]
    fn display_keeps_punctuation_decimals_and_bounds_long_streams() {
        let mut text = String::new();
        for delta in ["Value 3.", "14!", "”", " "] {
            append_display_text(&mut text, delta);
        }
        assert_eq!(text, "Value 3.14!” ");
        append_display_text(&mut text, "Next sentence.");
        assert_eq!(text, "Value 3.14!” Next sentence.");

        text.clear();
        for _ in 0..10_000 {
            append_display_text(&mut text, "汉字🙂");
            assert!(text.chars().count() <= MAX_DISPLAY_CHARS);
            assert!(text.ends_with("汉字🙂"));
        }
        text = "a".repeat(MAX_DISPLAY_CHARS);
        append_display_text(&mut text, " recent words");
        assert_eq!(text, "recent words");
    }

    #[test]
    fn full_sentences_survive_display_limits_and_stop_flushes_once() {
        let mut state = State::default();
        let original = "word ".repeat(100);
        let translated = "文字".repeat(200);
        let event = normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription": {"text": original, "languageCode": "en"},
                "outputTranscription": {"text": translated}
            }}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        let CloudEvent::LiveTranslation {
            snapshot,
            completed,
            ..
        } = event
        else {
            panic!()
        };
        assert!(completed.is_empty());
        assert!(snapshot.text.chars().count() <= MAX_DISPLAY_CHARS);
        let CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        } = finish(&config(), &mut state).unwrap()
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].transcript.text, original);
        assert_eq!(translations[0].transcript.translation, translated);
        assert!(finish(&config(), &mut state).is_none());
    }

    #[test]
    fn same_language_finishes_without_translation_and_interrupt_clears_pending_text() {
        let mut state = State::default();
        let event = normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription": {"text": "你好。", "languageCode": "zh-Hant"}
            }}),
            &mut state,
        )
        .unwrap()
        .unwrap();
        let CloudEvent::LiveTranslation { completed, .. } = event else {
            panic!()
        };
        assert!(completed.is_empty());
        let event = finish(&config(), &mut state).unwrap();
        let CloudEvent::LiveTranslation { completed, .. } = event else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert!(!completed[0].pending);
        assert!(completed[0].transcript.translation.is_empty());
        normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription": {"text": "pending", "languageCode": "en"}
            }}),
            &mut state,
        )
        .unwrap();
        normalize_event(
            &config(),
            &json!({"serverContent": {"interrupted": true}}),
            &mut state,
        )
        .unwrap();
        assert!(finish(&config(), &mut state).is_none());
        assert!(state.input.is_empty());
        assert!(state.output.is_empty());
    }

    #[test]
    fn missing_translation_retains_the_whole_native_source_at_stop() {
        let mut state = State::default();
        normalize_event(
            &config(),
            &json!({"serverContent": {
                "inputTranscription": {"text": "Hello. Tail", "languageCode": "en"}
            }}),
            &mut state,
        )
        .unwrap();
        let CloudEvent::LiveTranslation { completed, .. } = finish(&config(), &mut state).unwrap()
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].transcript.text, "Hello. Tail");
        assert!(completed
            .iter()
            .all(|result| result.transcript.translation.is_empty()));
    }

    #[test]
    fn errors_and_limits_are_bounded() {
        let mut state = State::default();
        assert!(
            normalize_event(&config(), &json!({"error":{"message":"quota"}}), &mut state).is_err()
        );
        assert!(normalize_event(&config(), &json!({"serverContent":{"inputTranscription":{"text":"a".repeat(MAX_TRANSCRIPT_BYTES + 1)}}}), &mut state).is_err());
    }
    #[tokio::test(start_paused = true)]
    async fn same_language_completes_before_close_and_language_changes_keep_the_old_source() {
        let mut state = State::default();
        normalize_event(&config(), &json!({"serverContent":{"inputTranscription":{"text":"你好", "languageCode":"zh-Hant"}}}), &mut state).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = normalize_event(
            &config(),
            &json!({"serverContent":{"turnComplete":true}}),
            &mut state,
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "你好");
        assert!(!completed[0].pending);
        assert!(translations.is_empty());
        normalize_event(&config(), &json!({"serverContent":{"inputTranscription":{"text":"再见", "languageCode":"zh-Hant"}}}), &mut state).unwrap();
        let Some(CloudEvent::LiveTranslation {
            completed,
            snapshot,
            ..
        }) = normalize_event(
            &config(),
            &json!({"serverContent":{"inputTranscription":{"text":"Hello.", "languageCode":"en"}}}),
            &mut state,
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "再见");
        assert_eq!(completed[0].transcript.language.as_deref(), Some("zh-Hant"));
        assert_eq!(snapshot.text, "Hello.");
        assert_eq!(snapshot.language.as_deref(), Some("en"));
    }
    #[tokio::test(start_paused = true)]
    async fn same_frame_completion_preserves_prefix_and_tail_once() {
        for marker in ["turnComplete", "finished"] {
            let mut state = State::default();
            let mut message = json!({"serverContent":{
                "inputTranscription":{"text":"你好。接下来", "languageCode":"zh-Hant"}
            }});
            if marker == "turnComplete" {
                message["serverContent"][marker] = json!(true);
            } else {
                message["serverContent"]["inputTranscription"][marker] = json!(true);
            }
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                snapshot,
                ..
            }) = normalize_event(&config(), &message, &mut state).unwrap()
            else {
                panic!("expected both source completions")
            };
            assert_eq!(completed.len(), 2);
            assert_eq!(
                completed
                    .iter()
                    .map(|r| r.transcript.text.as_str())
                    .collect::<String>(),
                "你好。接下来"
            );
            assert!(completed.iter().all(|r| !r.pending));
            assert!(translations.is_empty());
            assert!(snapshot.text.is_empty());
            assert!(super::super::live_translation::poll(&config(), &mut state).is_none());
            assert!(finish(&config(), &mut state).is_none());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn foreign_to_target_keeps_the_foreign_source_until_its_translation_arrives() {
        for alignment_enabled in [true, false] {
            let mut config = config();
            config.live_alignment.enabled = alignment_enabled;
            let mut state = State::default();
            normalize_event(
                &config,
                &json!({"serverContent":{"inputTranscription":{
                    "text":"Hello.", "languageCode":"en"
                }}}),
                &mut state,
            )
            .unwrap();
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                snapshot,
                ..
            }) = normalize_event(
                &config,
                &json!({"serverContent":{
                    "inputTranscription":{"text":"你好。", "languageCode":"zh-Hant"},
                    "turnComplete":true
                }}),
                &mut state,
            )
            .unwrap()
            else {
                panic!()
            };
            assert_eq!(completed.len(), 1);
            assert_eq!(completed[0].transcript.text, "你好。");
            assert_eq!(completed[0].transcript.language.as_deref(), Some("zh-Hant"));
            assert!(!completed[0].pending);
            assert!(translations.is_empty());
            assert!(snapshot.text.is_empty());
            assert_eq!(state.input, "Hello.");
            assert_eq!(state.language.as_deref(), Some("en"));
            normalize_event(
                &config,
                &json!({"serverContent":{
                    "outputTranscription":{"text":"哈囉。"}
                }}),
                &mut state,
            )
            .unwrap();
            tokio::time::advance(std::time::Duration::from_millis(450)).await;
            let event = if alignment_enabled {
                super::super::live_translation::poll(&config, &mut state)
            } else {
                finish(&config, &mut state)
            };
            let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = event
            else {
                panic!()
            };
            assert_eq!(completed.len(), 1);
            assert_eq!(completed[0].transcript.text, "Hello.");
            assert_eq!(completed[0].transcript.language.as_deref(), Some("en"));
            assert_eq!(translations.len(), 1);
            assert_eq!(translations[0].transcript.translation, "哈囉。");
            assert_eq!(
                translations[0].transcript.utterance_id,
                completed[0].transcript.utterance_id
            );
            assert!(finish(&config, &mut state).is_none());
        }
    }

    #[tokio::test(start_paused = true)]
    async fn source_only_continuations_without_language_codes_stay_separate() {
        let mut state = State::default();
        normalize_event(
            &config(),
            &json!({"serverContent":{"inputTranscription":{
                "text":"Hello.", "languageCode":"en"
            }}}),
            &mut state,
        )
        .unwrap();
        normalize_event(
            &config(),
            &json!({"serverContent":{"inputTranscription":{
                "text":"你好", "languageCode":"zh-Hant"
            }}}),
            &mut state,
        )
        .unwrap();
        // Output for the earlier English source must not change how a later
        // Chinese delta without its own language code is routed.
        normalize_event(
            &config(),
            &json!({"serverContent":{
                "outputTranscription":{"text":"哈囉。"}
            }}),
            &mut state,
        )
        .unwrap();
        let Some(CloudEvent::LiveTranslation { completed, .. }) = normalize_event(
            &config(),
            &json!({"serverContent":{
                "inputTranscription":{"text":"朋友"}, "turnComplete":true
            }}),
            &mut state,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "你好朋友");
        assert_eq!(completed[0].transcript.language.as_deref(), Some("zh-Hant"));
        assert_eq!(state.input, "Hello.");
        let Some(CloudEvent::LiveTranslation { completed, .. }) = finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "Hello.");
        assert_eq!(completed[0].transcript.language.as_deref(), Some("en"));
    }
}
