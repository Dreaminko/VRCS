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
    if text.is_empty() && translation.is_empty() {
        return Ok(None);
    }
    super::live_translation::append(
        config,
        state,
        text,
        translation,
        input
            .and_then(|v| v.get("languageCode"))
            .and_then(Value::as_str),
    )
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
    fn real_continuous_stream_preserves_both_complete_texts() {
        let messages: Vec<Value> =
            serde_json::from_str(include_str!("fixtures/gemini_live_translate.json")).unwrap();
        let mut state = State::default();
        let mut results = Vec::new();
        let mut translated = std::collections::HashMap::new();
        for message in messages {
            if let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = normalize_event(&config(), &message, &mut state).unwrap()
            {
                results.extend(completed);
                for update in translations.into_iter().filter(|update| !update.pending) {
                    translated.insert(
                        update.transcript.utterance_id,
                        update.transcript.translation,
                    );
                }
            }
        }
        assert!(
            !results.is_empty(),
            "continuous speech must finalize source sentences"
        );
        assert!(
            !translated.is_empty(),
            "continuous translation must complete individual sentences"
        );
        let CloudEvent::LiveTranslation {
            completed,
            snapshot,
            translations,
            ..
        } = finish(&config(), &mut state).unwrap()
        else {
            panic!()
        };
        results.extend(completed);
        for update in translations {
            translated.insert(
                update.transcript.utterance_id,
                update.transcript.translation,
            );
        }
        let source = results
            .iter()
            .map(|r| r.transcript.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            source.split_whitespace().collect::<String>(),
            messages_text("inputTranscription")
                .split_whitespace()
                .collect::<String>()
        );
        let target = results
            .iter()
            .filter_map(|r| translated.get(&r.transcript.utterance_id))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(
            target.split_whitespace().collect::<String>(),
            messages_text("outputTranscription")
                .split_whitespace()
                .collect::<String>()
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

    #[test]
    fn sentences_roll_independently_and_keep_late_translation() {
        let mut state = State::default();
        for (input, output, expected_input, expected_output) in [
            ("Hello.", "你好。", "Hello.", "你好。"),
            (" Next", "", "Next", ""),
            (" sentence.", "下一", "Next sentence.", "下一"),
            ("", "句。", "Next sentence.", "下一句。"),
            ("", "再见。新的", "Next sentence.", "下一句。再见。新的"),
        ] {
            let event = normalize_event(
                &config(),
                &json!({"serverContent": {
                    "inputTranscription": {"text": input},
                    "outputTranscription": {"text": output}
                }}),
                &mut state,
            )
            .unwrap()
            .unwrap();
            let CloudEvent::LiveTranslation { snapshot, .. } = event else {
                panic!("expected snapshot")
            };
            assert_eq!(snapshot.text, expected_input);
            assert_eq!(snapshot.translation, expected_output);
        }
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
        assert_eq!(completed[0].transcript.text, original.trim());
        assert_eq!(translations[0].transcript.translation, translated);
        assert!(finish(&config(), &mut state).is_none());
    }

    #[test]
    fn same_language_does_not_wait_for_translation_and_interrupt_clears_pending_text() {
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
    fn sentence_boundaries_preserve_decimals_and_missing_translation() {
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
        assert_eq!(completed[0].transcript.text, "Tail");
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
}
