use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::config::{AsrConfig, QWEN_MANAGED_BACKEND};
use crate::providers::{self, ServiceAdapter};

use super::{
    share_audio, spawn_cloud_recognition_session, spawn_managed_qwen_session, CloudEvent,
    CloudRecognitionSession, ModelManager, QwenRuntime, SharedAudio,
};

pub(super) fn validate_audio(samples: &[f32]) -> Result<(), String> {
    if samples.is_empty() {
        return Err("Question audio is empty".into());
    }
    if samples.iter().any(|sample| !sample.is_finite()) {
        return Err("Question audio contains non-finite samples".into());
    }
    let power = samples
        .iter()
        .map(|sample| f64::from(*sample).powi(2))
        .sum::<f64>()
        / samples.len() as f64;
    // Reject digital silence and negligible capture noise before contacting a model.
    if power <= 1e-8 {
        return Err("Question audio is silent".into());
    }
    Ok(())
}

pub(crate) fn validate_backend(config: &AsrConfig) -> Result<(), String> {
    if config.backend == QWEN_MANAGED_BACKEND {
        return Ok(());
    }
    let (_, service) = providers::recognition_service(&config.backend)
        .ok_or_else(|| format!("Unsupported question ASR backend: {}", config.backend))?;
    match service.adapter {
        ServiceAdapter::OpenAiAudioTranscriptions
        | ServiceAdapter::MimoTranscription
        | ServiceAdapter::QwenRealtime
        | ServiceAdapter::FunAsrRealtime => Ok(()),
        ServiceAdapter::QwenLiveTranslate
        | ServiceAdapter::OpenAiRealtimeTranslate
        | ServiceAdapter::GeminiLiveTranslate => {
            Err("Question ASR does not support live translation backends; a source transcript is required".into())
        }
        _ => Err(format!(
            "Unsupported question ASR backend {}: reliable end-of-audio completion is not available",
            config.backend
        )),
    }
}

pub(crate) type QuestionDraft = Arc<Mutex<String>>;

#[derive(Default)]
struct Transcript {
    utterances: Vec<(String, String, bool)>,
}

impl Transcript {
    fn update(&mut self, event: &CloudEvent) {
        let (id, text, final_text) = match event {
            CloudEvent::Partial {
                utterance_id, text, ..
            } => (utterance_id, text, false),
            CloudEvent::Final {
                utterance_id, text, ..
            } => (utterance_id, text, true),
            _ => return,
        };
        if let Some((_, previous, finalized)) =
            self.utterances.iter_mut().find(|(key, _, _)| key == id)
        {
            if final_text || !*finalized {
                *previous = text.trim().to_owned();
                *finalized = final_text;
            }
        } else {
            self.utterances
                .push((id.clone(), text.trim().to_owned(), final_text));
        }
    }

    fn draft(&self) -> String {
        self.utterances
            .iter()
            .map(|(_, text, _)| text.as_str())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// A single worker owns the recognizer for the entire press/release interval.
/// Batch previews re-upload cumulative PCM at most once per 750 ms, with one
/// request in flight. Longer questions cost more compute/bandwidth than realtime.
pub(crate) async fn recognize_live_question(
    config: AsrConfig,
    reader: crate::vision::QuestionAudioReader,
    silence_seconds: f64,
    runtime: Arc<QwenRuntime>,
    manager: Arc<ModelManager>,
    draft: QuestionDraft,
) -> Result<String, String> {
    validate_backend(&config)?;
    let session = if config.backend == QWEN_MANAGED_BACKEND {
        spawn_managed_qwen_session(config, runtime, manager).await?
    } else {
        spawn_cloud_recognition_session(config, silence_seconds).await?
    };
    recognize_live_session(session, reader, draft).await
}

pub(super) async fn recognize_live_session(
    session: CloudRecognitionSession,
    mut reader: crate::vision::QuestionAudioReader,
    draft: QuestionDraft,
) -> Result<String, String> {
    let mut session = QuestionSession(session);
    match &mut session.0 {
        CloudRecognitionSession::Realtime(stream) => {
            let mut transcript = Transcript::default();
            let events = stream
                .transcribe_question_live(reader, |event| {
                    transcript.update(event);
                    *draft.lock().expect("question draft lock") = transcript.draft();
                })
                .await?;
            merge_finals(events)
        }
        CloudRecognitionSession::Segmented(upload) => {
            let mut samples = Vec::new();
            let mut last_preview = Instant::now();
            while let Some(chunk) = reader.next().await? {
                if chunk.iter().any(|sample| !sample.is_finite()) {
                    return Err("Question audio contains non-finite samples".into());
                }
                samples.extend_from_slice(&chunk);
                if last_preview.elapsed() >= Duration::from_millis(750)
                    && validate_audio(&samples).is_ok()
                    && reader.released_at().is_none()
                {
                    let preview =
                        recognize_snapshot(upload, share_audio(samples.clone()), true).await?;
                    *draft.lock().expect("question draft lock") = preview;
                    last_preview = Instant::now();
                }
            }
            validate_audio(&samples)?;
            // Always recognize the exact released window. A preview is never final.
            let text = recognize_snapshot(upload, share_audio(samples), false).await?;
            *draft.lock().expect("question draft lock") = text.clone();
            Ok(text)
        }
    }
}

async fn recognize_snapshot(
    upload: &mut super::segmented_upload::SegmentedUploadSession,
    samples: SharedAudio,
    preview: bool,
) -> Result<String, String> {
    upload.send(samples).await?;
    let expected_id = if preview {
        upload.commit_preview().await?
    } else {
        upload.commit().await?
    };
    let mut events = Vec::new();
    loop {
        let event = upload
            .recv()
            .await
            .ok_or("Question recognition closed without a final result")?;
        let complete = matches!(&event, CloudEvent::Final { utterance_id, .. } if utterance_id == &expected_id);
        if let CloudEvent::Failed { code, detail, .. } = &event {
            return Err(format!("{code}: {detail}"));
        }
        events.push(event);
        if complete {
            return merge_finals_with_empty(events, preview);
        }
    }
}

struct QuestionSession(CloudRecognitionSession);

impl Drop for QuestionSession {
    fn drop(&mut self) {
        self.0.cancel();
    }
}

#[cfg(test)]
pub(super) async fn recognize_session(
    session: CloudRecognitionSession,
    samples: SharedAudio,
) -> Result<String, String> {
    // Keep ownership here while awaiting so timeout/drop always cancels the worker.
    let mut session = QuestionSession(session);
    let events = match &mut session.0 {
        CloudRecognitionSession::Realtime(stream) => {
            stream.transcribe_question_audio(samples).await?
        }
        CloudRecognitionSession::Segmented(upload) => {
            upload.send(samples).await?;
            let expected_id = upload.commit().await?;
            let mut events = Vec::new();
            loop {
                let event = upload.recv().await.ok_or_else(|| {
                    "Question recognition closed without a final result".to_string()
                })?;
                let complete = matches!(&event, CloudEvent::Final { utterance_id, .. } if utterance_id == &expected_id);
                if let CloudEvent::Failed { code, detail, .. } = &event {
                    return Err(format!("{code}: {detail}"));
                }
                events.push(event);
                if complete {
                    break events;
                }
            }
        }
    };
    merge_finals(events)
}

fn merge_finals(events: Vec<CloudEvent>) -> Result<String, String> {
    merge_finals_with_empty(events, false)
}

fn merge_finals_with_empty(events: Vec<CloudEvent>, allow_empty: bool) -> Result<String, String> {
    let mut utterances: Vec<(String, Option<String>)> = Vec::new();
    for event in events {
        let (id, text) = match event {
            CloudEvent::Partial { utterance_id, .. } => (utterance_id, None),
            CloudEvent::Final {
                utterance_id, text, ..
            } => (utterance_id, Some(text.trim().to_owned())),
            CloudEvent::Failed { code, detail, .. } => return Err(format!("{code}: {detail}")),
            CloudEvent::LiveTranslation { .. } => {
                return Err(
                    "Question recognition returned translation instead of source text".into(),
                );
            }
        };
        if let Some((_, previous)) = utterances
            .iter_mut()
            .find(|(previous_id, _)| previous_id == &id)
        {
            if text.is_some() {
                *previous = text;
            }
        } else {
            utterances.push((id, text));
        }
    }
    if utterances.iter().any(|(_, text)| text.is_none()) {
        return Err("Question recognition finished with an incomplete utterance".into());
    }
    let text = utterances
        .into_iter()
        .filter_map(|(_, text)| text.filter(|text| !text.is_empty()))
        .collect::<Vec<_>>()
        .join(" ");
    if text.is_empty() && !allow_empty {
        return Err("Question recognition returned no final source text".into());
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn final_event(id: &str, text: &str) -> CloudEvent {
        CloudEvent::Final {
            utterance_id: id.into(),
            text: text.into(),
            language: None,
        }
    }

    #[test]
    fn draft_replaces_partials_and_keeps_finalized_utterances() {
        let mut transcript = Transcript::default();
        for text in ["wrong", "correct"] {
            transcript.update(&CloudEvent::Partial {
                utterance_id: "first".into(),
                text: text.into(),
                language: None,
            });
        }
        assert_eq!(transcript.draft(), "correct");
        transcript.update(&final_event("first", "final"));
        transcript.update(&CloudEvent::Partial {
            utterance_id: "first".into(),
            text: "stale".into(),
            language: None,
        });
        transcript.update(&CloudEvent::Partial {
            utterance_id: "second".into(),
            text: "tail".into(),
            language: None,
        });
        assert_eq!(transcript.draft(), "final tail");
    }

    #[test]
    fn finals_replace_duplicates_and_preserve_utterance_order() {
        assert_eq!(
            merge_finals(vec![
                CloudEvent::Partial {
                    utterance_id: "first".into(),
                    text: "preview".into(),
                    language: None
                },
                final_event("second", " world "),
                final_event("first", "old"),
                final_event("first", "hello"),
            ])
            .unwrap(),
            "hello world"
        );
    }

    #[test]
    fn incomplete_empty_and_failed_results_are_errors() {
        assert!(merge_finals(vec![CloudEvent::Partial {
            utterance_id: "one".into(),
            text: "preview".into(),
            language: None,
        }])
        .is_err());
        assert!(merge_finals(vec![final_event("one", " ")]).is_err());
        assert!(merge_finals(vec![
            final_event("one", "valid"),
            CloudEvent::Failed {
                utterance_id: None,
                reset_session: true,
                code: "failure".into(),
                detail: "broken".into(),
            }
        ])
        .unwrap_err()
        .contains("broken"));
    }

    #[test]
    fn preview_allows_only_empty_completed_source_text() {
        assert_eq!(
            merge_finals_with_empty(vec![final_event("one", " ")], true).unwrap(),
            ""
        );
        assert!(merge_finals_with_empty(
            vec![CloudEvent::Partial {
                utterance_id: "one".into(),
                text: String::new(),
                language: None,
            }],
            true,
        )
        .is_err());
        assert!(merge_finals_with_empty(
            vec![CloudEvent::Failed {
                utterance_id: None,
                reset_session: false,
                code: "fatal".into(),
                detail: "Transcription response did not contain text: malformed response".into(),
            }],
            true,
        )
        .unwrap_err()
        .contains("malformed response"));
    }

    #[test]
    fn rejects_empty_silent_and_invalid_pcm() {
        assert!(validate_audio(&[]).is_err());
        assert!(validate_audio(&[0.0; 320]).is_err());
        assert!(validate_audio(&[1e-6; 320]).is_err());
        assert!(validate_audio(&[f32::NAN]).is_err());
        assert!(validate_audio(&[f32::INFINITY]).is_err());
        assert!(validate_audio(&[0.1; 320]).is_ok());
    }

    #[test]
    fn rejects_translation_and_unreliable_realtime_backends() {
        for backend in [
            providers::SERVICE_QWEN_LIVE_TRANSLATE,
            providers::SERVICE_OPENAI_REALTIME,
        ] {
            let config = AsrConfig {
                backend: backend.into(),
                ..AsrConfig::default()
            };
            assert!(validate_backend(&config).is_err());
        }
    }
}
