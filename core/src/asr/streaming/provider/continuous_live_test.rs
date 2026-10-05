//! Opt-in continuous translation capture using the production normalizers.
//! Records transcript fields only; credentials and audio payloads are excluded.
use super::*;
use futures_util::{SinkExt, StreamExt};
use serde_json::json;
use std::collections::{BTreeMap, HashSet};
use std::time::Duration;

#[derive(Default)]
struct Observation {
    originals: Vec<super::super::LiveTranslationResult>,
    latest: BTreeMap<String, super::super::LiveTranslationResult>,
    revisions: usize,
    publications: Vec<Value>,
}

impl Observation {
    fn collect(&mut self, event: Option<CloudEvent>, received_ms: u64) {
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = event
        else {
            return;
        };
        for result in completed {
            self.publications.push(json!({"received_ms":received_ms,"kind":"source","transcript":result.transcript,"pending":result.pending}));
            self.originals.push(result);
        }
        for result in translations {
            if result.pending {
                continue;
            }
            if self.latest.contains_key(&result.transcript.utterance_id) {
                self.revisions += 1;
            }
            self.publications.push(json!({"received_ms":received_ms,"kind":"translation","transcript":result.transcript}));
            self.latest
                .insert(result.transcript.utterance_id.clone(), result);
        }
    }
    fn verify_and_report(&self, raw_source: &str, raw_target: &str) -> Value {
        let ids: HashSet<_> = self
            .originals
            .iter()
            .map(|r| &r.transcript.utterance_id)
            .collect();
        assert!(!ids.is_empty(), "no completed source transcription");
        assert_eq!(ids.len(), self.originals.len(), "source published twice");
        assert!(
            self.latest.keys().all(|id| ids.contains(id)),
            "orphan translation update"
        );
        let source: String = self
            .originals
            .iter()
            .map(|r| r.transcript.text.as_str())
            .collect();
        let translated: String = self
            .originals
            .iter()
            .filter_map(|r| self.latest.get(&r.transcript.utterance_id))
            .map(|r| r.transcript.translation.as_str())
            .collect();
        // Source-only and translated lanes can finish in a different order.
        // Check character conservation here; semantic pairing is reviewed from
        // the individual groups in the report, not inferred from concatenation.
        let chars = |s: &str| {
            let mut v: Vec<_> = s.chars().filter(|c| !c.is_whitespace()).collect();
            v.sort_unstable();
            v
        };
        assert_eq!(
            chars(&source),
            chars(raw_source),
            "source text lost or duplicated"
        );
        let nonspace = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        assert_eq!(
            nonspace(&translated),
            nonspace(raw_target),
            "native translation reordered, lost or duplicated"
        );
        json!({"source_count":self.originals.len(),"translation_count":self.latest.len(),"revision_count":self.revisions,"raw_source":raw_source,"raw_target":raw_target,"publications":self.publications,"groups":self.originals.iter().map(|r| json!({"source":r.transcript,"translation":self.latest.get(&r.transcript.utterance_id).map(|t| &t.transcript.translation)})).collect::<Vec<_>>()})
    }
}

fn verify_source_order(observed: &Observation, recording: &[Value]) {
    let base = |language: &str| {
        language
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase()
    };
    let mut expected: BTreeMap<String, String> = BTreeMap::new();
    let mut language = String::new();
    for event in recording {
        if let Some(next) = event
            .pointer("/serverContent/inputTranscription/languageCode")
            .and_then(Value::as_str)
        {
            language = base(next);
        }
        let text = if event["type"] == "session.input_transcript.delta" {
            event["delta"].as_str()
        } else {
            event
                .pointer("/serverContent/inputTranscription/text")
                .and_then(Value::as_str)
        };
        if let Some(text) = text {
            expected
                .entry(language.clone())
                .or_default()
                .extend(text.chars().filter(|c| !c.is_whitespace()));
        }
    }
    let mut actual: BTreeMap<String, String> = BTreeMap::new();
    for row in &observed.originals {
        let language = base(row.transcript.language.as_deref().unwrap_or_default());
        actual
            .entry(language)
            .or_default()
            .extend(row.transcript.text.chars().filter(|c| !c.is_whitespace()));
    }
    assert_eq!(
        actual, expected,
        "source order or language ownership changed"
    );
}

fn transcript_event(provider: Provider, value: &Value, received_ms: u64) -> Option<Value> {
    if provider == Provider::OpenAiLiveTranslate {
        if !matches!(
            value["type"].as_str(),
            Some(
                "session.input_transcript.delta"
                    | "session.output_transcript.delta"
                    | "session.closed"
            )
        ) {
            return None;
        }
        let mut event = json!({"received_ms":received_ms,"type":value["type"]});
        for field in ["delta", "elapsed_ms", "event_id"] {
            if let Some(v) = value.get(field) {
                event[field] = v.clone();
            }
        }
        Some(event)
    } else {
        let content = value.get("serverContent")?;
        let mut event = json!({"received_ms":received_ms,"serverContent":{}});
        for field in ["inputTranscription", "outputTranscription"] {
            if let Some(part) = content.get(field) {
                let mut safe = json!({});
                for key in ["text", "languageCode", "finished"] {
                    if let Some(v) = part.get(key) {
                        safe[key] = v.clone();
                    }
                }
                event["serverContent"][field] = safe;
            }
        }
        for field in ["turnComplete", "generationComplete", "interrupted"] {
            if let Some(v) = content.get(field) {
                event["serverContent"][field] = v.clone();
            }
        }
        (!event["serverContent"].as_object().unwrap().is_empty()).then_some(event)
    }
}

async fn run(provider: Provider, key_env: &str, service: &str) {
    let key = std::env::var(key_env)
        .ok()
        .or_else(|| {
            std::env::var("CONTINUOUS_LIVE_API_KEY_FILE")
                .ok()
                .and_then(|p| std::fs::read_to_string(p).ok())
        })
        .expect("test API key required");
    let pcm =
        std::fs::read(std::env::var("CONTINUOUS_LIVE_PCM").expect("CONTINUOUS_LIVE_PCM required"))
            .expect("read PCM16 audio");
    assert!(pcm.len() >= 32000 && pcm.len() <= 32000 * 120 && pcm.len().is_multiple_of(2));
    let mut config = AsrConfig {
        backend: service.into(),
        live_translation_target: Some(
            std::env::var("CONTINUOUS_LIVE_TARGET").unwrap_or("zh-Hans".into()),
        ),
        ..Default::default()
    };
    if let Ok(model) = std::env::var("CONTINUOUS_LIVE_MODEL") {
        config.service_settings.get_mut(service).unwrap().model = model;
    }
    let profile = ApiProfile {
        id: "continuous-live-test".into(),
        provider: if provider == Provider::GeminiLiveTranslate {
            providers::GEMINI_PROVIDER
        } else {
            providers::OPENAI_PROVIDER
        }
        .into(),
        ..Default::default()
    };
    config.active_profile_id = Some(profile.id.clone());
    config.api_profiles.push(profile.clone());
    // Reuse production initialization, including JSON carried in binary frames
    // and heartbeats. Never Debug-format an error containing the credential URL.
    let (socket, _) =
        super::super::connect_initialized(provider, &config, &profile, 0.5, key.trim())
            .await
            .unwrap_or_else(|detail| {
                panic!(
                    "live initialization failed: {}",
                    detail.replace(key.trim(), "[redacted]")
                )
            });
    let (mut writer, mut reader) = socket.split();
    let started = tokio::time::Instant::now();
    let audio_ms = pcm.len() as u64 / 32;
    let (done_tx, mut done_rx) = tokio::sync::watch::channel(None::<u64>);
    let sender = tokio::spawn(async move {
        for (index, chunk) in pcm.chunks(3200).enumerate() {
            tokio::time::sleep_until(started + Duration::from_millis(index as u64 * 100)).await;
            let samples: Vec<_> = chunk
                .as_chunks::<2>()
                .0
                .iter()
                .map(|bytes| i16::from_le_bytes(*bytes) as f32 / 32768.0)
                .collect();
            writer
                .send(provider.audio_message(&samples))
                .await
                .expect("send audio");
        }
        writer
            .send(provider.finish_message(None).unwrap())
            .await
            .expect("send finish");
        done_tx
            .send(Some(started.elapsed().as_millis() as u64))
            .unwrap();
        writer
    });
    let mut state = NormalizationState::default();
    let mut observation = Observation::default();
    let mut recording = Vec::new();
    let (mut raw_source, mut raw_target) = (String::new(), String::new());
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let deadline = started + Duration::from_millis(audio_ms + 25_000);
    let mut finish_deadline = None;
    let mut finish_sent_ms = None;
    let mut closed = false;
    loop {
        tokio::select! {
            _ = tick.tick() => {
                observation.collect(provider.poll_translation(&config, &mut state), started.elapsed().as_millis() as u64);
                if tokio::time::Instant::now() >= deadline || finish_deadline.is_some_and(|d| tokio::time::Instant::now() >= d) { break; }
            }
            changed = done_rx.changed(), if done_rx.borrow().is_none() => {
                changed.expect("audio sender stopped");
                finish_sent_ms = *done_rx.borrow();
                finish_deadline = Some(tokio::time::Instant::now() + Duration::from_secs(15));
            }
            message = reader.next() => {
                let Some(message) = message else { break };
                let value: Value = match message.expect("receive live event") {
                    Message::Text(message) => serde_json::from_str(&message).unwrap(),
                    Message::Binary(message) => serde_json::from_slice(&message).unwrap(),
                    Message::Close(_) => break,
                    _ => continue,
                };
                let ms = started.elapsed().as_millis() as u64;
                if let Some(event) = transcript_event(provider, &value, ms) { recording.push(event); }
                if provider == Provider::OpenAiLiveTranslate {
                    match value["type"].as_str() {
                        Some("session.input_transcript.delta") => raw_source.push_str(value["delta"].as_str().unwrap_or_default()),
                        Some("session.output_transcript.delta") => raw_target.push_str(value["delta"].as_str().unwrap_or_default()),
                        _ => {},
                    }
                } else {
                    raw_source.push_str(value.pointer("/serverContent/inputTranscription/text").and_then(Value::as_str).unwrap_or_default());
                    raw_target.push_str(value.pointer("/serverContent/outputTranscription/text").and_then(Value::as_str).unwrap_or_default());
                }
                let event = provider.normalize_event(&config, &value, &mut state).unwrap_or_else(|detail| panic!("normalization failed: {}", detail.replace(key.trim(), "[redacted]")));
                observation.collect(event, ms);
                if provider.is_finished(&value) { closed = true; break; }
            }
        }
    }
    observation.collect(
        provider.finish_translation(&config, &mut state),
        started.elapsed().as_millis() as u64,
    );
    if let Ok(path) = std::env::var("CONTINUOUS_LIVE_RECORDING") {
        std::fs::write(path, serde_json::to_vec_pretty(&recording).unwrap()).unwrap();
    }
    let mut writer = sender.await.expect("audio sender failed");
    finish_sent_ms = finish_sent_ms.or(*done_rx.borrow());
    let _ = writer.close().await;
    verify_source_order(&observation, &recording);
    let mut report = observation.verify_and_report(&raw_source, &raw_target);
    report["audio_duration_ms"] = json!(audio_ms);
    report["finish_sent_ms"] = json!(finish_sent_ms);
    report["last_transcript_ms"] = recording
        .last()
        .map(|v| v["received_ms"].clone())
        .unwrap_or(Value::Null);
    report["events_after_finish"] =
        json!(recording
            .iter()
            .filter(|v| finish_sent_ms
                .is_some_and(|finish| v["received_ms"].as_u64().unwrap() > finish))
            .count());
    if let Ok(path) = std::env::var("CONTINUOUS_LIVE_REPORT") {
        std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    if provider == Provider::OpenAiLiveTranslate {
        assert!(closed, "session.close not acknowledged");
    }
    assert!(
        provider.finish_translation(&config, &mut state).is_none(),
        "close published twice"
    );
    println!(
        "Verified {} source groups, {} translation groups, {} revisions from {} transcript events",
        observation.originals.len(),
        observation.latest.len(),
        observation.revisions,
        recording.len()
    );
}

#[tokio::test]
#[ignore = "requires GEMINI_API_KEY and CONTINUOUS_LIVE_PCM (mono PCM16, 16 kHz)"]
async fn actual_gemini_continuous_translation() {
    run(
        Provider::GeminiLiveTranslate,
        "GEMINI_API_KEY",
        providers::SERVICE_GEMINI_LIVE_TRANSLATE,
    )
    .await;
}

#[tokio::test]
#[ignore = "requires OPENAI_API_KEY and CONTINUOUS_LIVE_PCM (mono PCM16, 16 kHz)"]
async fn actual_openai_continuous_translation() {
    run(
        Provider::OpenAiLiveTranslate,
        "OPENAI_API_KEY",
        providers::SERVICE_OPENAI_REALTIME_TRANSLATE,
    )
    .await;
}

async fn replay_gemini(recording: &str) -> Observation {
    let events: Vec<Value> = serde_json::from_str(recording).unwrap();
    let config = AsrConfig {
        backend: providers::SERVICE_GEMINI_LIVE_TRANSLATE.into(),
        live_translation_target: Some("zh-Hans".into()),
        ..Default::default()
    };
    let provider = Provider::GeminiLiveTranslate;
    let mut state = NormalizationState::default();
    let mut observed = Observation::default();
    let (mut source, mut target) = (String::new(), String::new());
    let start = tokio::time::Instant::now();
    for event in &events {
        let ms = event["received_ms"].as_u64().unwrap();
        let due = start + Duration::from_millis(ms);
        while tokio::time::Instant::now() < due {
            tokio::time::advance(
                (due - tokio::time::Instant::now()).min(Duration::from_millis(100)),
            )
            .await;
            observed.collect(provider.poll_translation(&config, &mut state), ms);
        }
        source.push_str(
            event
                .pointer("/serverContent/inputTranscription/text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        target.push_str(
            event
                .pointer("/serverContent/outputTranscription/text")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        );
        observed.collect(
            provider
                .normalize_event(&config, event, &mut state)
                .unwrap(),
            ms,
        );
    }
    for _ in 0..60 {
        tokio::time::advance(Duration::from_millis(100)).await;
        observed.collect(
            provider.poll_translation(&config, &mut state),
            start.elapsed().as_millis() as u64,
        );
    }
    observed.collect(
        provider.finish_translation(&config, &mut state),
        start.elapsed().as_millis() as u64,
    );
    assert!(provider.finish_translation(&config, &mut state).is_none());
    verify_source_order(&observed, &events);
    observed.verify_and_report(&source, &target);
    observed
}

#[tokio::test(start_paused = true)]
async fn recorded_gemini_english_keeps_repeated_short_sentences_and_anchors() {
    let observed = replay_gemini(include_str!("fixtures/gemini_continuous_english.json")).await;
    for group in &observed.originals {
        let source = group.transcript.text.to_ascii_lowercase();
        let target = &observed.latest[&group.transcript.utterance_id]
            .transcript
            .translation;
        for (spoken, translated) in [
            ("meeting", "会议"),
            ("door", "门"),
            ("umbrella", "伞"),
            ("thank you", "谢谢"),
            ("train", "火车"),
            ("apples", "苹果"),
        ] {
            if source.contains(spoken) {
                assert!(
                    target.contains(translated),
                    "wrong native pairing: {source:?} => {target:?}"
                );
            }
        }
    }
    assert_eq!(
        observed
            .originals
            .iter()
            .map(|r| r
                .transcript
                .text
                .to_ascii_lowercase()
                .matches("thank you")
                .count())
            .sum::<usize>(),
        2
    );
    assert_eq!(
        observed
            .latest
            .values()
            .map(|r| r.transcript.translation.matches("谢谢").count())
            .sum::<usize>(),
        2
    );
}

#[tokio::test(start_paused = true)]
async fn recorded_gemini_mixed_language_completes_source_only_rows_without_echo() {
    let observed = replay_gemini(include_str!("fixtures/gemini_continuous_mixed.json")).await;
    let chinese: Vec<_> = observed
        .originals
        .iter()
        .filter(|r| {
            r.transcript
                .language
                .as_deref()
                .is_some_and(|l| providers::same_live_translation_language(l, "zh-Hans"))
        })
        .collect();
    assert!(!chinese.is_empty());
    for row in chinese {
        assert!(!row.pending);
        assert!(!observed.latest.contains_key(&row.transcript.utterance_id));
        // Source-only completion is independent of final session flushing.
        let publication = observed
            .publications
            .iter()
            .find(|p| {
                p["kind"] == "source"
                    && p["transcript"]["utterance_id"] == row.transcript.utterance_id
            })
            .unwrap();
        assert!(publication["received_ms"].as_u64().unwrap() < 25_000);
    }
    for row in observed
        .originals
        .iter()
        .filter(|r| r.transcript.language.as_deref() == Some("en"))
    {
        assert!(row.pending);
        assert!(!observed.latest[&row.transcript.utterance_id]
            .transcript
            .translation
            .is_empty());
    }
}
