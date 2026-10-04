//! Explicit opt-in test. Credentials are read from the environment, never logged.
use super::*;
use crate::asr::streaming::provider::qwen;
use crate::config::ApiProfile;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
#[ignore = "requires QWEN_LIVE_API_KEY and QWEN_LIVE_PCM (mono PCM16, 16 kHz)"]
async fn actual_speech_translation_and_speakers() {
    let key = std::env::var("QWEN_LIVE_API_KEY").expect("QWEN_LIVE_API_KEY required");
    let pcm = std::fs::read(std::env::var("QWEN_LIVE_PCM").expect("QWEN_LIVE_PCM required"))
        .expect("read test audio");
    assert!(pcm.len() > 32000 && pcm.len() % 2 == 0);
    let config = AsrConfig {
        backend: SERVICE_QWEN_LIVE_TRANSLATE.into(),
        live_translation_target: Some(
            std::env::var("QWEN_LIVE_TARGET").unwrap_or("zh-Hans".into()),
        ),
        ..Default::default()
    };
    let profile = ApiProfile {
        provider: std::env::var("QWEN_LIVE_PROVIDER")
            .unwrap_or(crate::providers::QWEN_AI_PROVIDER.into()),
        region: std::env::var("QWEN_LIVE_REGION").ok(),
        workspace_id: std::env::var("QWEN_LIVE_WORKSPACE").ok(),
        ..Default::default()
    };
    let request = qwen::build_request(&config, &profile, &key).unwrap();
    let connected = tokio::time::timeout(
        Duration::from_secs(30),
        tokio_tungstenite::connect_async(request),
    )
    .await
    .expect("connection timed out");
    let (socket, _) = match connected {
        Ok(result) => result,
        Err(tokio_tungstenite::tungstenite::Error::Http(response)) => {
            panic!("WebSocket handshake failed: {}", response.status())
        }
        Err(_) => panic!("WebSocket connection failed"),
    };
    let (mut writer, mut reader) = socket.split();
    writer
        .send(Message::Text(
            session_update(&config).unwrap().to_string().into(),
        ))
        .await
        .unwrap();
    let mut state = State::default();
    let (mut sources, mut translations, mut speakers) = (Vec::new(), Vec::new(), HashSet::new());
    let mut writer = Some(writer);
    let mut sender = None;
    let started = std::time::Instant::now();
    let mut raw = Vec::new();
    let mut finished = false;
    while let Some(message) = tokio::time::timeout(Duration::from_secs(45), reader.next())
        .await
        .expect("server event timeout")
    {
        let Message::Text(message) = message.expect("receive failed") else {
            continue;
        };
        let value: Value = serde_json::from_str(&message).unwrap();
        let kind = value["type"].as_str().unwrap_or_default();
        if kind == "error" {
            let detail = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            panic!("Server error: {}", detail.replace(&key, "[redacted]"));
        }
        if kind == "session.updated" && sender.is_none() {
            let audio = pcm.clone();
            let mut writer = writer.take().unwrap();
            sender = Some(tokio::spawn(async move {
                for chunk in audio.chunks(3200) {
                    use base64::Engine;
                    let encoded = base64::engine::general_purpose::STANDARD.encode(chunk);
                    writer
                        .send(Message::Text(
                            json!({"type":"input_audio_buffer.append","audio":encoded})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                writer.send(qwen::finish_message()).await.unwrap();
                println!("session.finish sent at {}ms", started.elapsed().as_millis());
                writer
            }));
        }
        if kind.starts_with("conversation.item.")
            || kind.starts_with("response.")
            || kind == "input_audio_buffer.speech_started"
        {
            // Only transcript protocol events are recorded; no headers, keys,
            // session configuration or input audio are retained.
            if kind != "response.audio.delta" {
                raw.push(json!({"elapsed_ms":started.elapsed().as_millis(),"event":value.clone()}));
            }
        }
        if let Some(CloudEvent::LiveTranslation {
            completed,
            translations: done,
            ..
        }) = normalize_event(&config, &value, &mut state).unwrap()
        {
            for result in completed {
                println!(
                    "source {}ms speaker={:?}: {}",
                    started.elapsed().as_millis(),
                    result.transcript.speaker.as_ref().map(|s| s.index),
                    result.transcript.text
                );
                if let Some(speaker) = &result.transcript.speaker {
                    speakers.insert(speaker.index);
                }
                sources.push(result.transcript);
            }
            for result in done {
                println!(
                    "translation {}ms: {}",
                    started.elapsed().as_millis(),
                    result.transcript.translation
                );
                translations.push(result.transcript);
            }
        }
        if kind == "session.finished" {
            finished = true;
            break;
        }
    }
    if let Some(sender) = sender {
        sender.await.unwrap().close().await.unwrap();
    }
    if let Ok(path) = std::env::var("QWEN_LIVE_RECORDING") {
        std::fs::write(path, serde_json::to_vec_pretty(&raw).unwrap()).unwrap();
    }
    assert!(finished, "session.finish was not acknowledged");
    assert!(!sources.is_empty(), "no final source transcription");
    assert_eq!(sources.len(), translations.len(), "unpaired final items");
    assert!(translations
        .iter()
        .all(|item| !item.translation.trim().is_empty()));
    assert!(!speakers.is_empty(), "speaker IDs missing");
    for source in &sources {
        let target = translations
            .iter()
            .find(|t| t.utterance_id == source.utterance_id)
            .unwrap();
        assert_eq!(source.speaker, target.speaker);
    }
    assert!(
        state.finish(&config).is_none(),
        "unflushed tail after session.finished"
    );
    println!(
        "Verified {} paired items and {} distinct speaker IDs",
        sources.len(),
        speakers.len()
    );
}
