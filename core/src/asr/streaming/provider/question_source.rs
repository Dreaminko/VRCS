use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::asr::streaming::CloudEvent;
use crate::config::AsrConfig;

use super::{Provider, MAX_TRANSCRIPT_BYTES};

#[derive(Default)]
struct Utterance {
    id: String,
    text: String,
    done: bool,
    language: Option<String>,
}

#[derive(Default)]
pub(super) struct State {
    utterances: Vec<Utterance>,
    indices: HashMap<String, usize>,
    event_ids: HashSet<String>,
}

impl State {
    fn update(
        &mut self,
        id: &str,
        text: &str,
        done: bool,
        language: Option<String>,
    ) -> Result<Option<CloudEvent>, String> {
        let index = if let Some(index) = self.indices.get(id) {
            *index
        } else {
            let index = self.utterances.len();
            self.utterances.push(Utterance {
                id: id.to_owned(),
                ..Utterance::default()
            });
            self.indices.insert(id.to_owned(), index);
            index
        };
        let utterance = &mut self.utterances[index];
        if utterance.done {
            return Ok(None);
        }
        if done {
            utterance.text = text.to_owned();
            utterance.done = true;
        } else {
            if utterance.text.len().saturating_add(text.len()) > MAX_TRANSCRIPT_BYTES {
                return Err("Question source transcript exceeded 65536 bytes".into());
            }
            utterance.text.push_str(text);
        }
        if language.is_some() {
            utterance.language = language;
        }
        let text = utterance.text.clone();
        let language = utterance.language.clone();
        Ok((!text.is_empty()).then_some(if done {
            CloudEvent::Final {
                utterance_id: utterance.id.clone(),
                text,
                language,
            }
        } else {
            CloudEvent::Partial {
                utterance_id: utterance.id.clone(),
                text,
                language,
            }
        }))
    }

    pub(super) fn source_len(&self) -> usize {
        self.utterances
            .iter()
            .map(|utterance| utterance.text.len())
            .sum()
    }
}

pub(super) fn normalize_event(
    provider: Provider,
    config: &AsrConfig,
    value: &Value,
    state: &mut State,
) -> Result<Option<CloudEvent>, String> {
    let kind = value["type"].as_str().unwrap_or_default();
    if !config.question_source {
        return Ok(None);
    }
    match provider {
        Provider::QwenLiveTranslate => normalize_qwen(config, kind, value, state),
        Provider::OpenAiLiveTranslate => normalize_openai(kind, value, state),
        _ => Err("Question source transcript is not available for this service".into()),
    }
}

pub(super) fn finish(provider: Provider, state: &mut State) -> Result<Vec<CloudEvent>, String> {
    if state.utterances.is_empty() {
        return Err("Question recognition returned no final source text".into());
    }
    match provider {
        Provider::OpenAiLiveTranslate => {
            let utterance = state
                .utterances
                .iter_mut()
                .find(|utterance| !utterance.done)
                .ok_or("Question recognition returned no open source transcript")?;
            utterance.done = true;
            Ok(vec![CloudEvent::Final {
                utterance_id: utterance.id.clone(),
                text: utterance.text.clone(),
                language: utterance.language.clone(),
            }])
        }
        Provider::QwenLiveTranslate => {
            if state.utterances.iter().any(|utterance| !utterance.done) {
                return Err(
                    "Question recognition finished with an incomplete source utterance".into(),
                );
            }
            Ok(Vec::new())
        }
        _ => Err("Question source completion is not available for this service".into()),
    }
}

fn normalize_qwen(
    config: &AsrConfig,
    kind: &str,
    value: &Value,
    state: &mut State,
) -> Result<Option<CloudEvent>, String> {
    if kind == "error" {
        return Err(value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Qwen Live Translate failed")
            .into());
    }
    if kind == "conversation.item.input_audio_transcription.failed" {
        return Err(value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("Qwen source transcription failed")
            .into());
    }
    let id = value["item_id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .unwrap_or("qwen-question");
    let language = value["language"]
        .as_str()
        .map(str::to_owned)
        .or_else(|| (config.language != "auto").then(|| config.language.clone()));
    let done = match kind {
        "conversation.item.input_audio_transcription.completed" => true,
        "conversation.item.input_audio_transcription.delta"
        | "conversation.item.input_audio_transcription.text" => false,
        _ => return Ok(None),
    };
    if done {
        let text = value["transcript"]
            .as_str()
            .or_else(|| value["text"].as_str())
            .unwrap_or_default();
        return state.update(id, text, true, language);
    }
    let text = value["delta"]
        .as_str()
        .or_else(|| value["text"].as_str())
        .unwrap_or_default();
    state.update(id, text, false, language)
}

fn normalize_openai(
    kind: &str,
    value: &Value,
    state: &mut State,
) -> Result<Option<CloudEvent>, String> {
    if kind == "error" {
        return Err(value
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("OpenAI Realtime Translation failed")
            .into());
    }
    if kind != "session.input_transcript.delta" {
        return Ok(None);
    }
    if let Some(id) = value["event_id"].as_str() {
        if !state.event_ids.insert(id.to_owned()) {
            return Ok(None);
        }
    }
    let text = value["delta"].as_str().unwrap_or_default();
    state.update("openai-question", text, false, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{SERVICE_OPENAI_REALTIME_TRANSLATE, SERVICE_QWEN_LIVE_TRANSLATE};
    use serde_json::json;

    fn config(backend: &str) -> AsrConfig {
        AsrConfig {
            backend: backend.into(),
            question_source: true,
            ..Default::default()
        }
    }

    #[test]
    fn qwen_ignores_translation_events_and_collects_complete_source_text() {
        let config = config(SERVICE_QWEN_LIVE_TRANSLATE);
        let mut state = State::default();
        for event in [
            json!({"type":"response.text.delta","item_id":"translation","delta":"误译"}),
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"one","delta":"What "}),
            json!({"type":"conversation.item.input_audio_transcription.delta","item_id":"two","delta":"is this?"}),
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"one","transcript":"What"}),
            json!({"type":"conversation.item.input_audio_transcription.completed","item_id":"two","transcript":"is this?"}),
        ] {
            normalize_event(Provider::QwenLiveTranslate, &config, &event, &mut state).unwrap();
        }
        assert!(finish(Provider::QwenLiveTranslate, &mut state)
            .unwrap()
            .is_empty());
        assert_eq!(state.utterances[0].text, "What");
        assert_eq!(state.utterances[1].text, "is this?");
    }

    #[test]
    fn openai_preserves_same_timestamp_deltas_and_deduplicates_event_ids() {
        let config = config(SERVICE_OPENAI_REALTIME_TRANSLATE);
        let mut state = State::default();
        for event in [
            json!({"type":"session.input_transcript.delta","event_id":"a","delta":"Hello","elapsed_ms":200}),
            json!({"type":"session.input_transcript.delta","event_id":"b","delta":" there","elapsed_ms":200}),
            json!({"type":"session.input_transcript.delta","event_id":"b","delta":" there","elapsed_ms":200}),
        ] {
            normalize_event(Provider::OpenAiLiveTranslate, &config, &event, &mut state).unwrap();
        }
        let events = finish(Provider::OpenAiLiveTranslate, &mut state).unwrap();
        assert!(matches!(
            events.as_slice(),
            [CloudEvent::Final { text, .. }] if text == "Hello there"
        ));
    }
}
