//! Opt-in live and recorded protocol tests. Credentials are never logged.
use super::*;
use crate::asr::streaming::provider::qwen;
use crate::config::ApiProfile;
use futures_util::{SinkExt, StreamExt};
use std::collections::HashSet;
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

#[test]
#[ignore = "requires QWEN_LIVE_REPLAY_DIR containing captured *-events.json files"]
fn recorded_stream_previews_and_final_text() {
    let directory = std::env::var("QWEN_LIVE_REPLAY_DIR").expect("recording directory required");
    let mut sessions = 0;
    let mut final_count = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("-events.json")
        {
            continue;
        }
        let frames: Vec<Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let config = AsrConfig {
            backend: SERVICE_QWEN_LIVE_TRANSLATE.into(),
            live_translation_target: Some("zh-Hans".into()),
            ..Default::default()
        };
        let mut state = State::default();
        let mut originals = HashSet::new();
        let (mut expected, mut saved) = (Vec::new(), Vec::new());
        let (mut previews, mut row_updates, mut longest) = (0, 0, 0);
        for frame in frames {
            let value = frame.get("event").unwrap_or(&frame);
            if value["type"] == "response.done" {
                for item in value["response"]["output"].as_array().unwrap() {
                    for content in item["content"].as_array().unwrap() {
                        if let Some(text) = content["text"].as_str() {
                            expected.push(text.to_owned());
                        }
                    }
                }
            }
            if let Some(CloudEvent::LiveTranslation {
                snapshot,
                completed,
                translations,
                ..
            }) = normalize_event(&config, value, &mut state).unwrap()
            {
                originals.extend(completed.iter().map(|r| r.transcript.utterance_id.clone()));
                if let Some(preview) = snapshot.conversation_preview {
                    longest = longest.max(preview.translation.chars().count());
                    assert!(snapshot.text.chars().count() <= 160);
                    assert!(snapshot.translation.chars().count() <= 160);
                    if value["type"] == "response.text.delta" {
                        assert!(!preview.translation.is_empty());
                        previews += 1;
                    }
                }
                for result in translations {
                    assert!(originals.contains(&result.transcript.utterance_id));
                    if result.pending {
                        row_updates += 1;
                    } else {
                        assert_ne!(value["type"], "response.text.delta");
                        assert_ne!(value["type"], "response.text.done");
                        saved.push(result.transcript.translation);
                    }
                }
            }
        }
        expected.sort();
        saved.sort();
        assert!(!saved.is_empty());
        assert_eq!(saved, expected, "final text differs in {}", path.display());
        assert_eq!(originals.len(), saved.len());
        assert!(previews > 0 && row_updates > 0);
        assert!(state.finish(&config).is_none());
        println!("{}: {previews} delta previews, {row_updates} row updates, {} saved results, {longest} preview characters", path.file_name().unwrap().to_string_lossy(), saved.len());
        sessions += 1;
        final_count += saved.len();
    }
    assert!(sessions > 0);
    println!("Verified {sessions} recorded sessions and {final_count} complete translations");
}

#[tokio::test]
#[ignore = "requires Qwen credentials and QWEN_LIVE_PCM (mono PCM16, 16 kHz)"]
async fn actual_speech_translation_and_speakers() {
    let key = std::env::var("QWEN_LIVE_API_KEY").unwrap_or_else(|_| {
        std::fs::read_to_string(
            std::env::var("QWEN_LIVE_API_KEY_FILE").expect("QWEN_LIVE_API_KEY or file required"),
        )
        .expect("read test credential")
        .trim()
        .to_owned()
    });
    let pcm = std::fs::read(std::env::var("QWEN_LIVE_PCM").expect("QWEN_LIVE_PCM required"))
        .expect("read test audio");
    assert!(pcm.len() > 32000 && pcm.len() % 2 == 0);
    let mut config = AsrConfig {
        backend: SERVICE_QWEN_LIVE_TRANSLATE.into(),
        live_translation_target: Some(
            std::env::var("QWEN_LIVE_TARGET").unwrap_or("zh-Hans".into()),
        ),
        ..Default::default()
    };
    if let Ok(path) = std::env::var("QWEN_LIVE_GLOSSARY") {
        config.live_translation_phrases =
            serde_json::from_slice(&std::fs::read(path).expect("read test glossary"))
                .expect("glossary must be a JSON source-to-target map");
    }
    let profile = ApiProfile {
        provider: std::env::var("QWEN_LIVE_PROVIDER")
            .unwrap_or(crate::providers::QWEN_AI_PROVIDER.into()),
        region: std::env::var("QWEN_LIVE_REGION").ok(),
        workspace_id: std::env::var("QWEN_LIVE_WORKSPACE").ok(),
        ..Default::default()
    };
    let request = qwen::build_request(&config, &profile, &key).unwrap();
    let connected = tokio::time::timeout(Duration::from_secs(30), async {
        if let Ok(address) = std::env::var("QWEN_LIVE_CONNECT_ADDR") {
            // Test-only TCP routing for an unreachable DNS address. The
            // original request still controls the Host and verified TLS name.
            let socket = tokio::net::TcpStream::connect(address)
                .await
                .map_err(tokio_tungstenite::tungstenite::Error::Io)?;
            tokio_tungstenite::client_async_tls(request, socket).await
        } else {
            tokio_tungstenite::connect_async(request).await
        }
    })
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
    let mut observations = Vec::new();
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
        assert_ne!(
            kind, "response.audio.delta",
            "text-only sessions must not generate audio"
        );
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
            snapshot,
            ..
        }) = normalize_event(&config, &value, &mut state).unwrap()
        {
            observations.push(json!({
                "elapsed_ms": started.elapsed().as_millis(),
                "trigger": kind,
                "snapshot": snapshot,
                "completed": completed.iter().map(|r| &r.transcript).collect::<Vec<_>>(),
                "translations": done.iter().filter(|r| !r.pending).map(|r| &r.transcript).collect::<Vec<_>>(),
                "translation_previews": done.iter().filter(|r| r.pending).map(|r| &r.transcript).collect::<Vec<_>>()
            }));
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
            for result in done.into_iter().filter(|result| !result.pending) {
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
    if let Ok(path) = std::env::var("QWEN_LIVE_OBSERVATION") {
        std::fs::write(path, serde_json::to_vec_pretty(&observations).unwrap()).unwrap();
    }
    assert!(finished, "session.finish was not acknowledged");
    assert!(!sources.is_empty(), "no final source transcription");
    assert_eq!(sources.len(), translations.len(), "unpaired final items");
    assert!(translations
        .iter()
        .all(|item| !item.translation.trim().is_empty()));
    if let Ok(expected) = std::env::var("QWEN_LIVE_EXPECT_TERM") {
        assert!(
            translations
                .iter()
                .any(|item| item.translation.contains(&expected)),
            "native output did not use glossary target: {expected}"
        );
    }
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
