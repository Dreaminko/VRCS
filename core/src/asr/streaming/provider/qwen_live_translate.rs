//! Qwen 3.8 protocol: append-only deltas, explicit item links and speaker IDs.
use super::live_translation::{append_display_text, result};
use super::{CloudEvent, MAX_ACTIVE_TRANSCRIPTS, MAX_TRANSCRIPT_BYTES};
use crate::config::AsrConfig;
use crate::models::{LiveTranslation, LiveTranslationPreview, SpeakerIdentity};
use crate::providers::SERVICE_QWEN_LIVE_TRANSLATE;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};

pub(super) fn session_update(config: &AsrConfig) -> Result<Value, String> {
    let target = config
        .live_translation_target
        .as_deref()
        .ok_or("Select an automatic translation target for Qwen Live Translate")?;
    let language = crate::providers::qwen_translation_language(target)?;
    let mut translation = json!({"language":language});
    if !config.live_translation_phrases.is_empty() {
        translation["corpus"] = json!({"phrases":config.live_translation_phrases});
    }
    Ok(
        json!({"event_id": uuid::Uuid::new_v4().to_string(), "type":"session.update", "session": {
            "output_modalities":["text"],
            "translation":translation,
            "audio":{"input":{"turn_detection":{"type":"speaker_detection","threshold":0.5}}}
        }}),
    )
}

#[derive(Default)]
struct Source {
    // Preview text is append-only delta data. Completion text is stored separately.
    text: String,
    final_text: Option<String>,
    language: Option<String>,
    speaker: Option<SpeakerIdentity>,
    done: bool,
    published: bool,
}
#[derive(Default)]
struct Target {
    // text.done is intentionally ignored; only response.done supplies final_text.
    text: String,
    final_text: Option<String>,
    previewed_text: Option<String>,
    source_id: Option<String>,
    response_id: Option<String>,
    done: bool,
}
#[derive(Default)]
pub(super) struct State {
    session_id: String,
    sources: HashMap<String, Source>,
    targets: HashMap<String, Target>,
    order: VecDeque<String>,
    retired: VecDeque<String>,
    speaker_indices: HashMap<u64, u64>,
    active: Option<String>,
}

fn text(value: &Value, field: &str) -> String {
    value[field].as_str().unwrap_or_default().to_owned()
}
fn set_text(
    current: &mut String,
    final_text: &mut Option<String>,
    value: &Value,
    done: bool,
    field: &str,
) -> Result<(), String> {
    if let Some(value) = value[field].as_str() {
        if done {
            *final_text = Some(value.to_owned());
        } else {
            current.push_str(value);
        }
    }
    if current.len() > MAX_TRANSCRIPT_BYTES
        || final_text
            .as_ref()
            .is_some_and(|text| text.len() > MAX_TRANSCRIPT_BYTES)
    {
        return Err("Qwen translation transcript limit reached".into());
    }
    Ok(())
}
impl State {
    fn source(&mut self, id: &str) -> &mut Source {
        if !self.sources.contains_key(id) {
            self.order.push_back(id.to_owned());
        }
        self.sources.entry(id.to_owned()).or_default()
    }
    fn retire(&mut self, id: String) {
        self.retired.push_back(id);
        while self.retired.len() > 128 {
            self.retired.pop_front();
        }
    }
    fn transcript(&self, config: &AsrConfig, id: &str, source: &Source) -> LiveTranslation {
        LiveTranslation {
            utterance_id: format!("qwen-source-{}-{id}", self.session_id),
            source_utterance_id: None,
            conversation_preview: None,
            text: source.final_text.as_ref().unwrap_or(&source.text).clone(),
            language: source.language.clone(),
            speaker: source.speaker.clone(),
            translation: String::new(),
            target_language: config.live_translation_target.clone().unwrap_or_default(),
        }
    }
    fn collect(&mut self, config: &AsrConfig, flush: bool) -> Option<CloudEvent> {
        let mut completed = Vec::new();
        let mut translations = Vec::new();
        let mut remove = Vec::new();
        let mut published = Vec::new();
        for id in &self.order {
            let source = &self.sources[id];
            if !source.done && !flush {
                continue;
            }
            let mut transcript = self.transcript(config, id, source);
            if !source.published && !transcript.text.trim().is_empty() {
                completed.push(result(config, transcript.clone(), true));
                published.push(id.clone());
            }
            let target = self
                .targets
                .iter_mut()
                .find(|(_, target)| target.source_id.as_deref() == Some(id.as_str()));
            if let Some((target_id, target)) = target {
                if target.done || flush {
                    transcript.translation =
                        target.final_text.as_ref().unwrap_or(&target.text).clone();
                    if !transcript.text.trim().is_empty() {
                        translations.push(result(config, transcript, false));
                    }
                    remove.push((id.clone(), Some(target_id.clone())));
                } else if (!target.text.is_empty() || target.previewed_text.is_some())
                    && target.previewed_text.as_ref() != Some(&target.text)
                {
                    // The original is stored independently. Stream into that row
                    // without saving a translation until response.done confirms it.
                    transcript.translation = target.text.clone();
                    translations.push(result(config, transcript, true));
                    target.previewed_text = Some(target.text.clone());
                }
            } else if flush {
                if !transcript.text.trim().is_empty() {
                    translations.push(result(config, transcript, false));
                }
                remove.push((id.clone(), None));
            }
        }
        for id in published {
            self.sources.get_mut(&id).unwrap().published = true;
        }
        let active = self.active.clone().unwrap_or_default();
        let source_id = self
            .targets
            .get(&active)
            .and_then(|target| target.source_id.as_deref())
            .unwrap_or(&active)
            .to_owned();
        for (id, target) in remove {
            self.sources.remove(&id);
            self.order.retain(|value| *value != id);
            self.retire(id);
            if let Some(id) = target {
                self.targets.remove(&id);
                self.retire(id);
            }
        }
        let mut snapshot = if let Some(source) = self.sources.get(&source_id) {
            let mut snapshot = self.transcript(config, &source_id, source);
            snapshot.text = source.text.clone();
            snapshot
        } else {
            LiveTranslation {
                utterance_id: String::new(),
                source_utterance_id: None,
                conversation_preview: None,
                text: String::new(),
                language: None,
                speaker: None,
                translation: String::new(),
                target_language: config.live_translation_target.clone().unwrap_or_default(),
            }
        };
        if self.sources.contains_key(&source_id) {
            snapshot.source_utterance_id = Some(snapshot.utterance_id.clone());
        }
        snapshot.utterance_id = format!("qwen-preview-{}-{source_id}", self.session_id);
        if let Some(target) = self.targets.get(&active).or_else(|| {
            self.targets
                .values()
                .find(|target| target.source_id.as_deref() == Some(source_id.as_str()))
        }) {
            snapshot.translation = target.text.clone();
        }
        let original = std::mem::take(&mut snapshot.text);
        let translated = std::mem::take(&mut snapshot.translation);
        if !flush {
            snapshot.conversation_preview = Some(LiveTranslationPreview {
                text: original.clone(),
                translation: translated.clone(),
            });
            append_display_text(&mut snapshot.text, &original);
            append_display_text(&mut snapshot.translation, &translated);
        }
        (!completed.is_empty()
            || !translations.is_empty()
            || !snapshot.text.is_empty()
            || !snapshot.translation.is_empty())
        .then_some(CloudEvent::LiveTranslation {
            service: SERVICE_QWEN_LIVE_TRANSLATE.into(),
            snapshot: Box::new(snapshot),
            completed,
            translations,
        })
    }
    pub(super) fn finish(&mut self, config: &AsrConfig) -> Option<CloudEvent> {
        let event = self.collect(config, true);
        *self = Self::default();
        event
    }
    pub(super) fn source_len(&self) -> usize {
        self.sources.values().map(|source| source.text.len()).sum()
    }
}

pub(super) fn normalize_event(
    config: &AsrConfig,
    value: &Value,
    state: &mut State,
) -> Result<Option<CloudEvent>, String> {
    if state.session_id.is_empty() {
        state.session_id = uuid::Uuid::new_v4().to_string();
    }
    let kind = value["type"].as_str().unwrap_or_default();
    let id = text(value, "item_id");
    if !id.is_empty() && state.retired.contains(&id) {
        return Ok(None);
    }
    match kind {
        "error" => {
            return Err(value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("Qwen Live Translate failed")
                .into())
        }
        "input_audio_buffer.speech_started" => {
            if id.is_empty() {
                return Ok(None);
            }
            // Qwen AI also labels source message items as "assistant". A
            // speech/transcription event identifies the source unambiguously.
            state.targets.remove(&id);
            let speaker = value["speaker_id"].as_u64().map(|provider_id| {
                let next = state.speaker_indices.len() as u64;
                let index = *state.speaker_indices.entry(provider_id).or_insert(next);
                SpeakerIdentity {
                    id: format!("qwen-{}-{provider_id}", state.session_id),
                    index,
                }
            });
            // Register speech even when diarization metadata is absent. A late
            // completion for the previous sentence must not close this preview
            // before its first transcription delta arrives.
            let source = state.source(&id);
            if speaker.is_some() {
                source.speaker = speaker;
            }
            state.active = Some(id);
        }
        "conversation.item.created" => {
            let item_id = value
                .pointer("/item/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if item_id.is_empty() || state.retired.iter().any(|id| id == item_id) {
                return Ok(None);
            }
            if value.pointer("/item/role").and_then(Value::as_str) == Some("assistant") {
                if state.sources.contains_key(item_id) {
                    return Ok(None);
                }
                let source_id = value["previous_item_id"]
                    .as_str()
                    .filter(|id| !id.is_empty())
                    .map(str::to_owned);
                state.targets.entry(item_id.into()).or_default().source_id = source_id;
                state.active = Some(item_id.into());
            }
        }
        "conversation.item.input_audio_transcription.delta"
        | "conversation.item.input_audio_transcription.completed" => {
            if id.is_empty() {
                return Ok(None);
            }
            let done = kind.ends_with(".completed");
            state.targets.remove(&id);
            let source = state.source(&id);
            if source.done {
                return Ok(None);
            }
            set_text(
                &mut source.text,
                &mut source.final_text,
                value,
                done,
                if done { "transcript" } else { "delta" },
            )?;
            source.language = value["language"]
                .as_str()
                .map(str::to_owned)
                .or(source.language.take())
                .or_else(|| (config.language != "auto").then(|| config.language.clone()));
            source.done = done;
            if !done {
                state.active = Some(id);
            }
        }
        "conversation.item.input_audio_transcription.failed" => {
            let source = state.sources.remove(&id);
            state.order.retain(|value| *value != id);
            state
                .targets
                .retain(|_, target| target.source_id.as_deref() != Some(&id));
            state.retire(id.clone());
            return Ok(Some(CloudEvent::Failed {
                utterance_id: source.map(|_| format!("qwen-source-{}-{id}", state.session_id)),
                reset_session: false,
                code: "asr.cloud_error".into(),
                detail: value
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("Qwen source transcription failed")
                    .into(),
            }));
        }
        "response.text.delta" | "response.audio_transcript.delta" => {
            if id.is_empty() {
                return Ok(None);
            }
            let target = state.targets.entry(id.clone()).or_default();
            if target.done {
                return Ok(None);
            }
            target.response_id = value["response_id"]
                .as_str()
                .map(str::to_owned)
                .or(target.response_id.take());
            set_text(
                &mut target.text,
                &mut target.final_text,
                value,
                false,
                "delta",
            )?;
            state.active = Some(id);
        }
        "response.output_item.added" => {
            let item_id = value
                .pointer("/item/id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if item_id.is_empty() || state.retired.iter().any(|id| id == item_id) {
                return Ok(None);
            }
            state.targets.entry(item_id.into()).or_default().response_id =
                value["response_id"].as_str().map(str::to_owned);
        }
        "response.done" => {
            let response = &value["response"];
            let response_id = response["id"].as_str();
            let failed = response["status"]
                .as_str()
                .is_some_and(|status| status != "completed");
            if let Some(output) = response["output"].as_array() {
                for item in output {
                    let item_id = text(item, "id");
                    if item_id.is_empty() || state.retired.contains(&item_id) {
                        continue;
                    }
                    let target = state.targets.entry(item_id.clone()).or_default();
                    if !target.done {
                        let full: String = item["content"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|part| {
                                part["text"].as_str().or(part["transcript"].as_str())
                            })
                            .collect();
                        if full.len() > MAX_TRANSCRIPT_BYTES {
                            return Err("Qwen translation transcript limit reached".into());
                        }
                        target.final_text = Some(full);
                        target.done = true;
                    }
                    if failed {
                        target.final_text = Some(String::new());
                    }
                }
            }
            for target in state.targets.values_mut().filter(|target| {
                target.response_id.as_deref() == response_id && response_id.is_some()
            }) {
                target.done = true;
                target.final_text.get_or_insert_with(String::new);
                if failed {
                    target.final_text = Some(String::new());
                }
            }
        }
        _ => return Ok(None), // Raw audio and timing events are not transcript text.
    }
    if state.sources.len() > MAX_ACTIVE_TRANSCRIPTS || state.targets.len() > MAX_ACTIVE_TRANSCRIPTS
    {
        return Err("Too many pending Qwen translation items; reconnect recognition".into());
    }
    Ok(state.collect(config, false))
}

#[cfg(test)]
#[path = "qwen_live_translate/live_test.rs"]
mod live_test;

#[cfg(test)]
mod tests {
    use super::*;
    fn config() -> AsrConfig {
        AsrConfig {
            backend: SERVICE_QWEN_LIVE_TRANSLATE.into(),
            live_translation_target: Some("zh-Hant".into()),
            ..Default::default()
        }
    }
    fn event(state: &mut State, value: Value) -> Option<CloudEvent> {
        normalize_event(&config(), &value, state).unwrap()
    }
    fn link(state: &mut State, source: &str, target: &str) {
        event(
            state,
            json!({"type":"conversation.item.created","previous_item_id":source,"item":{"id":target,"role":"assistant"}}),
        );
    }
    fn source(state: &mut State, id: &str, transcript: &str) -> Option<CloudEvent> {
        event(
            state,
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":id,"transcript":transcript,"language":"en"}),
        )
    }
    fn target(state: &mut State, id: &str, transcript: &str) -> Option<CloudEvent> {
        event(
            state,
            json!({"type":"response.done","response":{"id":format!("response-{id}"),"status":"completed","output":[{"id":id,"content":[{"text":transcript}]}]}}),
        )
    }
    #[test]
    fn native_results_record_the_selected_platform_in_history_metadata() {
        for provider in [
            crate::providers::QWEN_AI_PROVIDER,
            crate::providers::ALIBABA_PROVIDER,
        ] {
            let mut config = config();
            config.active_profile_id = Some("selected".into());
            config.api_profiles.push(crate::config::ApiProfile {
                id: "selected".into(),
                provider: provider.into(),
                ..Default::default()
            });
            let mut state = State::default();
            let Some(CloudEvent::LiveTranslation { completed, .. }) = normalize_event(&config, &json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"source","transcript":"Hello."}), &mut state).unwrap() else {panic!()};
            assert_eq!(completed[0].provider, provider);
        }
    }

    #[test]
    fn recorded_qwen_ai_speakers_pair_all_text_without_accumulating_source_items() {
        let frames: Vec<Value> =
            serde_json::from_str(include_str!("qwen_live_translate/qwen_ai_speakers.json"))
                .unwrap();
        let mut state = State::default();
        let (mut sources, mut translations) = (Vec::new(), Vec::new());
        for frame in frames {
            if let Some(CloudEvent::LiveTranslation {
                completed,
                translations: done,
                ..
            }) = event(&mut state, frame)
            {
                sources.extend(completed);
                translations.extend(done.into_iter().filter(|result| !result.pending));
            }
        }
        assert_eq!(sources.len(), 3);
        assert_eq!(translations.len(), 3);
        assert_eq!(
            sources
                .iter()
                .map(|s| s.transcript.speaker.as_ref().unwrap().index)
                .collect::<Vec<_>>(),
            [0, 1, 0]
        );
        assert_eq!(sources[0].transcript.speaker, sources[2].transcript.speaker);
        for (source, target) in sources.iter().zip(&translations) {
            assert_eq!(
                source.transcript.utterance_id,
                target.transcript.utterance_id
            );
            assert_eq!(source.transcript.speaker, target.transcript.speaker);
            assert!(!target.transcript.translation.trim().is_empty());
        }
        assert!(state.sources.is_empty() && state.targets.is_empty());
        assert!(state.finish(&config()).is_none());
    }

    #[test]
    fn platform_endpoints_keep_qwen_ai_and_model_studio_separate() {
        use crate::config::ApiProfile;
        let mut profile = ApiProfile {
            provider: crate::providers::QWEN_AI_PROVIDER.into(),
            ..Default::default()
        };
        let request = super::super::qwen::build_request(&config(), &profile, "test-key").unwrap();
        assert_eq!(request.uri().to_string(), "wss://maas.qianwenaiapi.com/api-ws/v1/realtime?model=qwen3.8-livetranslate-flash-realtime");
        profile.provider = crate::providers::ALIBABA_PROVIDER.into();
        profile.workspace_id = Some("ws-example".into());
        for (region, host) in [
            ("china_beijing", "cn-beijing"),
            ("singapore", "ap-southeast-1"),
        ] {
            profile.region = Some(region.into());
            let request =
                super::super::qwen::build_request(&config(), &profile, "test-key").unwrap();
            assert_eq!(request.uri().to_string(), format!("wss://ws-example.{host}.maas.aliyuncs.com/api-ws/v1/realtime?model=qwen3.8-livetranslate-flash-realtime"));
        }
    }

    #[test]
    fn glossary_mapping_is_native_runtime_only_and_keeps_text_only_output() {
        let mut config = config();
        assert!(session_update(&config)
            .unwrap()
            .pointer("/session/translation/corpus")
            .is_none());
        config.live_translation_phrases = std::collections::BTreeMap::from([
            ("VRChat".into(), "VRChat".into()),
            ("report".into(), "星河档案".into()),
        ]);
        let value = session_update(&config).unwrap();
        assert_eq!(
            value.pointer("/session/translation/corpus/phrases"),
            Some(&json!({"VRChat":"VRChat","report":"星河档案"}))
        );
        assert_eq!(value["session"]["output_modalities"], json!(["text"]));
        let serialized = serde_json::to_value(&config).unwrap();
        assert!(serialized.get("live_translation_phrases").is_none());
        let restored: AsrConfig = serde_json::from_value(serialized).unwrap();
        assert!(restored.live_translation_phrases.is_empty());
    }

    #[test]
    fn setup_uses_38_modalities_diarization_and_wire_language() {
        let value = session_update(&config()).unwrap();
        assert_eq!(value["session"]["output_modalities"], json!(["text"]));
        assert_eq!(value["session"]["translation"]["language"], "zh");
        assert_eq!(
            value.pointer("/session/audio/input/turn_detection/type"),
            Some(&json!("speaker_detection"))
        );
        assert!(value["session"].get("input_audio_transcription").is_none());
        assert!(value["session"].get("same_language_skip_options").is_none());
        for (preset, wire) in [
            ("yue-Hant", "yue"),
            ("pt-BR", "pt"),
            ("fil", "fil"),
            ("nb", "nb"),
        ] {
            let mut config = config();
            config.live_translation_target = Some(preset.into());
            assert_eq!(
                session_update(&config).unwrap()["session"]["translation"]["language"],
                wire
            );
        }
        assert!(session_update(&AsrConfig::default()).is_err());
    }
    #[test]
    fn deltas_preview_and_final_snapshots_replace_without_echoing_audio() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        for delta in ["Hel", "lo"] {
            event(
                &mut state,
                json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s1","delta":delta}),
            );
        }
        for delta in ["你", "好"] {
            let Some(CloudEvent::LiveTranslation {
                snapshot,
                completed,
                translations,
                ..
            }) = event(
                &mut state,
                json!({"type":"response.text.delta","item_id":"t1","response_id":"r1","delta":delta}),
            )
            else {
                panic!()
            };
            assert!(snapshot.translation.ends_with(delta));
            assert!(completed.is_empty() && translations.is_empty());
        }
        assert!(event(
            &mut state,
            json!({"type":"response.audio.delta","item_id":"t1","delta":"AA=="})
        )
        .is_none());
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = source(&mut state, "s1", "Hello!")
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, "Hello!");
        assert_eq!(translations.len(), 1);
        assert!(translations[0].pending);
        assert_eq!(translations[0].transcript.translation, "你好");
        event(
            &mut state,
            json!({"type":"response.text.done","item_id":"t1","response_id":"r1","text":"你好！"}),
        );
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            target(&mut state, "t1", "你好！")
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.translation, "你好！");
        assert_eq!(
            translations[0].transcript.utterance_id,
            completed[0].transcript.utterance_id
        );
        assert!(source(&mut state, "s1", "Hello!").is_none());
        assert!(target(&mut state, "t1", "你好！").is_none());
        assert!(state.finish(&config()).is_none());
    }
    #[test]
    fn final_response_replaces_streamed_previews_without_early_persistence() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        source(&mut state, "s1", "The train leaves at 7:05, not 7:50.");
        let preview = "火车将在七点五十分出发";
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t1","response_id":"response-t1","delta":preview}),
        )
        else {
            panic!()
        };
        assert_eq!(snapshot.translation, preview);
        assert!(translations[0].pending);
        assert_eq!(translations[0].transcript.translation, preview);
        assert!(event(&mut state, json!({"type":"response.text.done","item_id":"t1","response_id":"response-t1","text":"火车将在七点零五分出发。"})).is_none());
        assert_eq!(state.targets["t1"].text, preview);
        let final_text = "火车七点零五分出发，而不是七点五十分。";
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = target(&mut state, "t1", final_text)
        else {
            panic!()
        };
        assert_eq!(translations.len(), 1);
        assert_eq!(translations[0].transcript.translation, final_text);
        assert!(!translations[0].pending);
        assert!(snapshot.translation.is_empty());
        assert!(state.finish(&config()).is_none());
    }

    #[test]
    fn completion_snapshots_do_not_enter_either_preview_lane() {
        for source_first in [true, false] {
            let mut state = State::default();
            link(&mut state, "s1", "t1");
            event(
                &mut state,
                json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s1","delta":"original delta"}),
            );
            event(
                &mut state,
                json!({"type":"response.text.delta","item_id":"t1","delta":"target delta"}),
            );
            let next = if source_first {
                source(&mut state, "s1", "corrected original final");
                event(
                    &mut state,
                    json!({"type":"response.text.delta","item_id":"t1","delta":" suffix"}),
                )
            } else {
                assert!(event(&mut state, json!({"type":"response.text.done","item_id":"t1","text":"corrected target final"})).is_none());
                event(
                    &mut state,
                    json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s1","delta":" suffix"}),
                )
            };
            let Some(CloudEvent::LiveTranslation {
                snapshot,
                completed,
                translations,
                ..
            }) = next
            else {
                panic!()
            };
            let preview = snapshot.conversation_preview.unwrap();
            assert_eq!(
                preview.text,
                if source_first {
                    "original delta"
                } else {
                    "original delta suffix"
                }
            );
            assert_eq!(
                preview.translation,
                if source_first {
                    "target delta suffix"
                } else {
                    "target delta"
                }
            );
            assert!(!snapshot.text.contains("corrected"));
            assert!(!snapshot.translation.contains("corrected"));
            assert!(completed.is_empty());
            assert!(translations
                .iter()
                .all(|r| r.pending && !r.transcript.translation.contains("corrected")));
            if !source_first {
                source(&mut state, "s1", "corrected original final");
            }
            let Some(CloudEvent::LiveTranslation { translations, .. }) =
                target(&mut state, "t1", "confirmed target final")
            else {
                panic!()
            };
            assert_eq!(translations[0].transcript.text, "corrected original final");
            assert_eq!(
                translations[0].transcript.translation,
                "confirmed target final"
            );
            assert!(!translations[0].pending);
        }
    }

    #[test]
    fn done_only_output_is_published_as_final_without_synthetic_previews() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        assert!(event(&mut state, json!({"type":"response.audio_transcript.done","item_id":"t1","transcript":"done-only target"})).is_none());
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            completed,
            translations,
            ..
        }) = source(&mut state, "s1", "done-only original")
        else {
            panic!()
        };
        assert!(snapshot.text.is_empty() && snapshot.translation.is_empty());
        assert_eq!(completed[0].transcript.text, "done-only original");
        assert!(translations.is_empty());
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = event(
            &mut state,
            json!({"type":"response.done","response":{"status":"completed","output":[{"id":"t1","content":[{"text":"done-only target"}]}]}}),
        )
        else {
            panic!()
        };
        assert!(snapshot.text.is_empty() && snapshot.translation.is_empty());
        assert_eq!(translations[0].transcript.translation, "done-only target");
        assert!(!translations[0].pending);
    }

    #[test]
    fn text_done_is_ignored_and_missing_response_text_is_not_saved_from_deltas() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        source(&mut state, "s1", "original final");
        event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t1","response_id":"r1","delta":"delta"}),
        );
        assert!(event(&mut state, json!({"type":"response.text.done","item_id":"t1","response_id":"r1","text":"duplicate complete snapshot"})).is_none());
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t1","response_id":"r1","delta":" suffix"}),
        )
        else {
            panic!()
        };
        assert_eq!(snapshot.translation, "delta suffix");
        assert_eq!(translations[0].transcript.translation, "delta suffix");
        assert!(translations[0].pending);
        let Some(CloudEvent::LiveTranslation { translations, .. }) = event(
            &mut state,
            json!({"type":"response.done","response":{"id":"r1","status":"completed","output":[]}}),
        ) else {
            panic!()
        };
        assert!(translations[0].transcript.translation.is_empty());
        assert!(!translations[0].pending);
    }

    #[test]
    fn late_previous_completion_keeps_a_new_speech_preview_open_without_speaker_id() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        source(&mut state, "s1", "first original");
        event(
            &mut state,
            json!({"type":"input_audio_buffer.speech_started","item_id":"s2"}),
        );
        let Some(CloudEvent::LiveTranslation { snapshot, .. }) =
            target(&mut state, "t1", "first translation")
        else {
            panic!()
        };
        assert!(snapshot.text.is_empty() && snapshot.translation.is_empty());
        assert_eq!(
            snapshot.source_utterance_id,
            Some(format!("qwen-source-{}-s2", state.session_id))
        );
        let preview_id = snapshot.utterance_id;
        let Some(CloudEvent::LiveTranslation { snapshot, .. }) = event(
            &mut state,
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s2","delta":"second original delta"}),
        ) else {
            panic!()
        };
        assert_eq!(snapshot.utterance_id, preview_id);
        assert_eq!(snapshot.text, "second original delta");
    }

    #[test]
    fn late_response_done_preserves_the_newer_delta_preview() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        source(&mut state, "s1", "first original");
        link(&mut state, "s2", "t2");
        event(
            &mut state,
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s2","delta":"second original delta"}),
        );
        event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t2","delta":"second target delta"}),
        );
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            translations,
            ..
        }) = target(&mut state, "t1", "first target final")
        else {
            panic!()
        };
        assert_eq!(translations[0].transcript.translation, "first target final");
        assert_eq!(snapshot.text, "second original delta");
        assert_eq!(snapshot.translation, "second target delta");
    }

    #[test]
    fn long_deltas_keep_full_conversation_preview_and_stream_into_the_original_row() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        let original = "A long technical sentence with a conditional clause. ".repeat(8);
        let translated = "包含条件从句的技术长句。".repeat(30);
        event(
            &mut state,
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s1","delta":original}),
        );
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            completed,
            translations,
            ..
        }) = event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t1","delta":translated}),
        )
        else {
            panic!()
        };
        assert!(completed.is_empty() && translations.is_empty());
        let preview = snapshot.conversation_preview.unwrap();
        assert_eq!(preview.text, original);
        assert_eq!(preview.translation, translated);
        assert!(snapshot.text.chars().count() <= 160);
        assert!(snapshot.translation.chars().count() <= 160);
        let Some(CloudEvent::LiveTranslation {
            snapshot,
            completed,
            translations,
            ..
        }) = source(&mut state, "s1", &original)
        else {
            panic!()
        };
        assert_eq!(completed.len(), 1);
        assert_eq!(translations.len(), 1);
        assert!(translations[0].pending);
        assert_eq!(translations[0].transcript.translation, translated);
        assert_eq!(
            snapshot.source_utterance_id.as_deref(),
            Some(completed[0].transcript.utterance_id.as_str())
        );
        let Some(CloudEvent::LiveTranslation { translations, .. }) = event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t1","delta":"尾句。"}),
        ) else {
            panic!()
        };
        assert_eq!(
            translations[0].transcript.translation,
            format!("{translated}尾句。")
        );
        assert!(translations[0].pending);
        let Some(CloudEvent::LiveTranslation { translations, .. }) =
            target(&mut state, "t1", "修订后的完整译文。")
        else {
            panic!()
        };
        assert_eq!(translations.len(), 1);
        assert!(!translations[0].pending);
        assert_eq!(translations[0].transcript.translation, "修订后的完整译文。");
    }

    #[test]
    fn translation_before_source_and_late_link_are_paired_by_id() {
        let mut state = State::default();
        target(&mut state, "t2", "再见。");
        source(&mut state, "s1", "Hello.");
        source(&mut state, "s2", "Goodbye.");
        let Some(CloudEvent::LiveTranslation { translations, .. }) = event(
            &mut state,
            json!({"type":"conversation.item.created","previous_item_id":"s2","item":{"id":"t2","role":"assistant"}}),
        ) else {
            panic!()
        };
        assert_eq!(translations[0].transcript.text, "Goodbye.");
        assert_eq!(translations[0].transcript.translation, "再见。");
        assert!(state.sources.contains_key("s1"));
    }
    #[test]
    fn alternating_speakers_survive_out_of_order_translation_and_session_reset() {
        let mut state = State::default();
        for (id, index) in [("s1", 0), ("s2", 1)] {
            event(
                &mut state,
                json!({"type":"input_audio_buffer.speech_started","item_id":id,"speaker_id":index}),
            );
            let Some(CloudEvent::LiveTranslation { completed, .. }) =
                source(&mut state, id, "Hello.")
            else {
                panic!()
            };
            assert_eq!(
                completed[0].transcript.speaker.as_ref().unwrap().index,
                index
            );
        }
        link(&mut state, "s1", "t1");
        link(&mut state, "s2", "t2");
        let Some(CloudEvent::LiveTranslation {
            translations: second,
            ..
        }) = target(&mut state, "t2", "你好2。")
        else {
            panic!()
        };
        let Some(CloudEvent::LiveTranslation {
            translations: first,
            ..
        }) = target(&mut state, "t1", "你好1。")
        else {
            panic!()
        };
        assert_eq!(second[0].transcript.speaker.as_ref().unwrap().index, 1);
        let identity = first[0].transcript.speaker.as_ref().unwrap();
        assert_eq!(identity.index, 0);
        state.finish(&config());
        event(
            &mut state,
            json!({"type":"input_audio_buffer.speech_started","item_id":"s3","speaker_id":0}),
        );
        let Some(CloudEvent::LiveTranslation { completed, .. }) = source(&mut state, "s3", "Next.")
        else {
            panic!()
        };
        assert_ne!(
            completed[0].transcript.speaker.as_ref().unwrap().id,
            identity.id
        );
    }
    #[test]
    fn missing_speaker_is_not_inherited_and_failed_output_is_not_success() {
        let mut state = State::default();
        event(
            &mut state,
            json!({"type":"input_audio_buffer.speech_started","item_id":"s1","speaker_id":2}),
        );
        source(&mut state, "s1", "First.");
        let Some(CloudEvent::LiveTranslation { completed, .. }) =
            source(&mut state, "s2", "Second.")
        else {
            panic!()
        };
        assert!(completed[0].transcript.speaker.is_none());
        link(&mut state, "s2", "t2");
        event(
            &mut state,
            json!({"type":"response.text.delta","item_id":"t2","response_id":"r2","delta":"部分译文"}),
        );
        let Some(CloudEvent::LiveTranslation { translations, .. }) = event(
            &mut state,
            json!({"type":"response.done","response":{"id":"r2","status":"incomplete","output":[]}}),
        ) else {
            panic!()
        };
        assert!(translations[0].transcript.translation.is_empty());
        assert!(!translations[0].pending);
    }
    #[test]
    fn close_preserves_received_tail_and_finishes_missing_output_once() {
        let mut state = State::default();
        link(&mut state, "s1", "t1");
        event(
            &mut state,
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"s1","delta":" Hello🙂 "}),
        );
        event(
            &mut state,
            json!({"type":"response.audio_transcript.delta","item_id":"t1","delta":" 你好🙂 "}),
        );
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = state.finish(&config())
        else {
            panic!()
        };
        assert_eq!(completed[0].transcript.text, " Hello🙂 ");
        assert_eq!(translations[0].transcript.translation, " 你好🙂 ");
        assert!(state.finish(&config()).is_none());
        source(&mut state, "s2", "No output.");
        let Some(CloudEvent::LiveTranslation {
            completed,
            translations,
            ..
        }) = state.finish(&config())
        else {
            panic!()
        };
        assert!(completed.is_empty());
        assert!(translations[0].transcript.translation.is_empty());
    }
}
