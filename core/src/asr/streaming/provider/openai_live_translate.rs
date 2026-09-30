use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::{http::Request, Message};

use crate::config::AsrConfig;
use crate::providers::SERVICE_OPENAI_REALTIME_TRANSLATE;

use super::{
    authenticated_request, live_translation, pcm16_base64, resample_16k_to_24k, service_settings,
    CloudEvent, InitializationEvent,
};

const TRANSCRIPTION_MODEL: &str = "gpt-realtime-whisper";

pub(super) fn build_request(config: &AsrConfig, key: &str) -> Result<Request<()>, String> {
    let model = &service_settings(config, SERVICE_OPENAI_REALTIME_TRANSLATE)?.model;
    let mut url = reqwest::Url::parse("wss://api.openai.com/v1/realtime/translations").unwrap();
    url.query_pairs_mut().append_pair("model", model);
    authenticated_request(url.into(), key, false)
}

pub(super) fn session_update(config: &AsrConfig) -> Result<Value, String> {
    let target = config
        .live_translation_target
        .as_deref()
        .ok_or("Select an automatic translation target for OpenAI Realtime Translation")?;
    let target = crate::providers::openai_translation_language(target)?;
    Ok(json!({"type": "session.update", "session": {"audio": {
        "input": {"transcription": {"model": TRANSCRIPTION_MODEL}, "noise_reduction": null},
        "output": {"language": target}
    }}}))
}

fn error_detail(value: &Value) -> String {
    let error = &value["error"];
    let mut detail = error["message"]
        .as_str()
        .unwrap_or("OpenAI Realtime Translation failed")
        .to_owned();
    for field in ["type", "code", "param", "event_id"] {
        if let Some(value) = error[field].as_str() {
            detail.push_str(&format!("; {field}={value}"));
        }
    }
    detail
}

pub(super) fn initialization_event(value: &Value, update: &Value) -> InitializationEvent {
    match value["type"].as_str() {
        Some("session.updated") => {
            if value.pointer("/session/audio/output/language")
                == update.pointer("/session/audio/output/language")
                && value
                    .pointer("/session/audio/input/transcription/model")
                    .and_then(Value::as_str)
                    == Some(TRANSCRIPTION_MODEL)
            {
                InitializationEvent::Ready
            } else {
                InitializationEvent::Failed("OpenAI did not confirm the requested translation target and source transcription model".into())
            }
        }
        Some("error") => InitializationEvent::Failed(error_detail(value)),
        Some("session.closed") => InitializationEvent::Failed(
            "OpenAI closed the translation session before configuration completed".into(),
        ),
        _ => InitializationEvent::Pending,
    }
}

pub(super) fn audio_message(samples: &[f32]) -> Message {
    Message::Text(
        json!({"type": "session.input_audio_buffer.append",
        "audio": pcm16_base64(&resample_16k_to_24k(samples))})
        .to_string()
        .into(),
    )
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    state: &mut live_translation::State,
) -> Result<Option<CloudEvent>, String> {
    let delta = value["delta"].as_str().unwrap_or_default();
    match value["type"].as_str() {
        Some("session.input_transcript.delta") => live_translation::append_delta(
            config,
            state,
            delta,
            "",
            value["elapsed_ms"].as_u64(),
            value["event_id"].as_str(),
        ),
        Some("session.output_transcript.delta") => live_translation::append_delta(
            config,
            state,
            "",
            delta,
            value["elapsed_ms"].as_u64(),
            value["event_id"].as_str(),
        ),
        Some("error") => {
            // A recoverable session error must not terminate the current subtitle.
            tracing::warn!(detail = %error_detail(value), "OpenAI translation session error");
            Ok(None)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> AsrConfig {
        AsrConfig {
            backend: SERVICE_OPENAI_REALTIME_TRANSLATE.into(),
            live_translation_target: Some("zh-Hant".into()),
            ..Default::default()
        }
    }

    #[test]
    fn target_codes_match_the_api_without_changing_stored_targets() {
        for (target, wire) in [
            ("zh-Hans", "zh"),
            ("zh-Hant", "zh"),
            ("pt-BR", "pt"),
            ("pt-PT", "pt"),
            ("fil", "tl"),
            ("nb", "no"),
            ("en", "en"),
        ] {
            let mut config = config();
            config.live_translation_target = Some(target.into());
            let update = session_update(&config).unwrap();
            assert_eq!(update["session"]["audio"]["output"]["language"], wire);
            let mut confirmation = update.clone();
            confirmation["type"] = json!("session.updated");
            assert!(matches!(
                initialization_event(&confirmation, &update),
                InitializationEvent::Ready
            ));
            let mut state = live_translation::State::default();
            live_translation::append_delta(
                &config,
                &mut state,
                "Hello.",
                "Translation.",
                None,
                None,
            )
            .unwrap();
            assert_eq!(state.snapshot.unwrap().target_language, target);
            assert_eq!(config.live_translation_target.as_deref(), Some(target));
        }
        let mut config = config();
        config.live_translation_target = Some("yue-Hant".into());
        assert!(session_update(&config).unwrap_err().contains("yue-Hant"));
    }

    #[test]
    fn translation_endpoint_and_configuration_are_independent_of_transcription() {
        let config = config();
        let request = build_request(&config, "test-key").unwrap();
        assert_eq!(
            request.uri().to_string(),
            "wss://api.openai.com/v1/realtime/translations?model=gpt-realtime-translate"
        );
        assert_eq!(request.headers()["Authorization"], "Bearer test-key");
        let update = session_update(&config).unwrap();
        assert_eq!(update["session"]["audio"]["output"]["language"], "zh");
        assert!(matches!(
            initialization_event(&json!({"type":"session.created"}), &update),
            InitializationEvent::Pending
        ));
        let mut confirmation = update.clone();
        confirmation["type"] = json!("session.updated");
        assert!(matches!(
            initialization_event(&confirmation, &update),
            InitializationEvent::Ready
        ));
        confirmation["session"]["audio"]["output"]["language"] = json!("en");
        assert!(matches!(
            initialization_event(&confirmation, &update),
            InitializationEvent::Failed(_)
        ));
        confirmation["session"]["audio"]["output"]["language"] = json!("zh");
        confirmation["session"]["audio"]["input"]["transcription"] = Value::Null;
        assert!(matches!(
            initialization_event(&confirmation, &update),
            InitializationEvent::Failed(_)
        ));
        assert!(session_update(&AsrConfig::default()).is_err());
    }

    #[test]
    fn audio_is_24k_pcm_and_preserves_silence() {
        use base64::Engine as _;
        for size in [3200, 100] {
            let Message::Text(message) = audio_message(&vec![0.0; size]) else {
                panic!()
            };
            let value: Value = serde_json::from_str(&message).unwrap();
            assert_eq!(value["type"], "session.input_audio_buffer.append");
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(value["audio"].as_str().unwrap())
                .unwrap();
            assert_eq!(bytes, vec![0; size * 3]);
        }
    }

    #[test]
    fn independent_deltas_keep_repetitions_in_one_stable_window() {
        let config = config();
        let mut state = live_translation::State::default();
        let mut results = Vec::new();
        let mut translated = Vec::new();
        for (kind, delta, frame) in [
            ("output_transcript", "你好", 400),
            ("input_transcript", "hello", 200),
            ("output_audio", "SGVsbG8=", 400),
            ("input_transcript", " hello.", 400),
            ("input_transcript", " Value 3.", 1000),
            ("input_transcript", "14.", 1200),
            ("output_transcript", "数值3.14", 1400),
        ] {
            if let Some(CloudEvent::LiveTranslation { completed, translations, .. }) = normalize_event(&config, &json!({"type": format!("session.{kind}.delta"), "delta": delta, "elapsed_ms": frame}), &mut state).unwrap() {
                results.extend(completed);
                translated.extend(translations.into_iter().filter(|update| !update.pending));
            }
        }
        if let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = live_translation::finish(&config, &mut state)
        {
            results.extend(completed);
            translated.extend(translations);
        }
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].transcript.text, "hello hello. Value 3.14.");
        assert_eq!(translated.len(), 1);
        assert_eq!(translated[0].transcript.text, "hello hello. Value 3.14.");
        assert_eq!(translated[0].transcript.translation, "你好数值3.14");
        assert_eq!(
            translated[0].source_utterance_ids,
            results
                .iter()
                .map(|source| source.transcript.utterance_id.clone())
                .collect::<Vec<_>>()
        );
        assert!(results.iter().all(|r| r.provider == "openai"
            && r.model == "gpt-realtime-translate"
            && r.transcript.language.is_none()));
        assert!(live_translation::finish(&config, &mut state).is_none());
    }

    #[test]
    fn missing_frames_keep_a_stable_one_to_one_window() {
        let mut state = live_translation::State::default();
        for event in [
            json!({"type":"session.input_transcript.delta","delta":"Hello."}),
            json!({"type":"session.output_transcript.delta","delta":"你好"}),
        ] {
            normalize_event(&config(), &event, &mut state).unwrap();
        }
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            live_translation::finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].transcript.translation, "你好");
    }

    #[tokio::test(start_paused = true)]
    async fn source_idle_does_not_cut_text_before_model_alignment() {
        let mut state = live_translation::State::default();
        normalize_event(
            &config(),
            &json!({"type":"session.input_transcript.delta", "delta":"Hello.", "elapsed_ms":200}),
            &mut state,
        )
        .unwrap();
        tokio::time::advance(std::time::Duration::from_secs(2)).await;
        assert!(live_translation::poll(&config(), &mut state).is_none());
        assert_eq!(state.input, "Hello.");
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = normalize_event(
            &config(),
            &json!({"type":"session.output_transcript.delta", "delta":"你好", "elapsed_ms":400}),
            &mut state,
        )
        .unwrap()
        else {
            panic!()
        };
        assert_eq!(snapshot.translation, "你好");
        assert!(translations.is_empty());
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = live_translation::finish(&config(), &mut state)
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.translation, "你好");
        assert_eq!(
            translations[0].transcript.utterance_id,
            completed[0].transcript.utterance_id
        );
        assert!(!translations[0].pending);
    }

    #[tokio::test(start_paused = true)]
    async fn captured_arrival_timing_does_not_split_or_shift_sentences() {
        replay_capture(
            include_str!("fixtures/openai_translation_frames.json"),
            &[
                "桌子上的红盒子",
                "请开窗",
                "我今天待在家里因为我觉得很累",
                "明天我们三点见",
                "蓝色火车七点开走",
                "谢谢你",
                "谢谢你",
                "请带两个苹果和一瓶水",
            ],
        )
        .await;
    }

    #[tokio::test(start_paused = true)]
    async fn captured_translation_can_finish_after_the_next_two_sources_start() {
        replay_capture(
            include_str!("fixtures/openai_translation_delayed.json"),
            &[
                "你好,虽然会议原定在周一早上,我们还是决定改到周五下午,因为经理还在路上。",
                "请等一下。",
                "把门关上。",
                "我昨天买了一把红伞和一本蓝色笔记本,但忘了带回家。",
                "谢谢。",
                "谢谢。",
                "火车七点发车。",
                "请带两个苹果和一瓶水。",
            ],
        )
        .await;
    }

    async fn replay_capture(capture: &str, expected: &[&str]) {
        let events: Vec<Value> = serde_json::from_str(capture).unwrap();
        let mut state = live_translation::State::default();
        let config = config();
        let start = tokio::time::Instant::now();
        let first = events[0]["received_ms"].as_u64().unwrap();
        let mut originals = Vec::new();
        let mut updates = Vec::new();
        let mut visible_deltas = 0;
        let mut collect = |event| {
            if let Some(CloudEvent::LiveTranslation {
                completed,
                translations,
                ..
            }) = event
            {
                originals.extend(completed);
                updates.extend(translations.into_iter().filter(|t| !t.pending));
            }
        };
        for event in events {
            let due = start
                + std::time::Duration::from_millis(event["received_ms"].as_u64().unwrap() - first);
            while tokio::time::Instant::now() < due {
                tokio::time::sleep_until(
                    due.min(tokio::time::Instant::now() + std::time::Duration::from_millis(100)),
                )
                .await;
                collect(live_translation::poll(&config, &mut state));
            }
            let normalized = normalize_event(&config, &event, &mut state).unwrap();
            if event["type"] == "session.output_transcript.delta" {
                let Some(CloudEvent::LiveTranslation {
                    snapshot,
                    translations,
                    ..
                }) = &normalized
                else {
                    panic!("each nonempty translated delta must publish immediately");
                };
                visible_deltas += 1;
                assert!(snapshot
                    .translation
                    .ends_with(event["delta"].as_str().unwrap()));
                assert!(
                    translations.is_empty(),
                    "a visible delta must never wait for alignment"
                );
            }
            collect(normalized);
        }
        collect(live_translation::finish(&config, &mut state));
        println!("fixture replay: sources={}, immediately displayed deltas={}, native fallback groups={} (no model request)", originals.len(), visible_deltas, updates.len());
        // Without a model response, shutdown retains the whole stream as one group.
        assert_eq!(originals.len(), 1);
        assert_eq!(updates.len(), 1);
        assert_eq!(
            updates[0].source_utterance_ids,
            originals
                .iter()
                .map(|original| original.transcript.utterance_id.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            updates[0].transcript.translation.replace(' ', ""),
            expected.concat().replace(' ', "")
        );
    }

    #[test]
    fn a_missing_translation_falls_back_to_one_shared_window() {
        let mut state = live_translation::State::default();
        let mut updates = Vec::new();
        for (kind, text, frame) in [
            ("input", "First.", 200),
            ("output", "第一句", 400),
            ("input", " Missing.", 1000),
            ("input", " Third.", 2000),
            ("output", "第三句。还有补充。", 2400),
        ] {
            if let Some(CloudEvent::LiveTranslation { translations, .. }) = normalize_event(
                &config(), &json!({"type": format!("session.{kind}_transcript.delta"), "delta": text, "elapsed_ms": frame}), &mut state,
            ).unwrap() {
                updates.extend(translations.into_iter().filter(|t| !t.pending));
            }
        }
        if let Some(CloudEvent::LiveTranslation { translations, .. }) =
            live_translation::finish(&config(), &mut state)
        {
            updates.extend(translations);
        }
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].source_utterance_ids.len(), 1);
        assert_eq!(updates[0].transcript.text, "First. Missing. Third.");
        assert_eq!(
            updates[0].transcript.translation,
            "第一句第三句。还有补充。"
        );
    }

    #[test]
    fn recoverable_errors_keep_pending_text_and_report_details() {
        let config = config();
        let mut state = live_translation::State::default();
        normalize_event(
            &config,
            &json!({"type":"session.input_transcript.delta","delta":"Hello"}),
            &mut state,
        )
        .unwrap();
        let error = json!({"type":"error","error":{"type":"invalid_request_error","code":"bad_audio","param":"audio","message":"Invalid audio"}});
        assert!(normalize_event(&config, &error, &mut state)
            .unwrap()
            .is_none());
        let detail = error_detail(&error);
        assert!(detail.contains("bad_audio") && detail.contains("param=audio"));
        assert_eq!(state.input, "Hello");
        assert!(matches!(
            initialization_event(&error, &session_update(&config).unwrap()),
            InitializationEvent::Failed(_)
        ));
    }
}
