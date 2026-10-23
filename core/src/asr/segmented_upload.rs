use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::config::{ApiProfile, RecognitionServiceSettings};

use super::qwen_runtime::{QwenConnection, QwenRuntime};
use super::streaming::{CloudEvent, SegmentationMode};
use super::SharedAudio;
use super::{mimo, openai_audio_transcriptions};

const MAX_AUDIO_SECONDS: usize = 300;
const MAX_AUDIO_SAMPLES: usize = 16_000 * MAX_AUDIO_SECONDS;
const UPLOAD_QUEUE_CAPACITY: usize = 4;
const EVENT_QUEUE_CAPACITY: usize = 32;

struct UploadJob {
    utterance_id: String,
    samples: Vec<f32>,
    allow_empty: bool,
}

enum UploadTarget {
    Profile {
        profile: Box<ApiProfile>,
        api_key: String,
        settings: RecognitionServiceSettings,
    },
    ManagedQwen {
        connection: QwenConnection,
        runtime: Arc<QwenRuntime>,
    },
}

pub struct SegmentedUploadSession {
    audio: Mutex<Vec<f32>>,
    max_audio_samples: usize,
    jobs: mpsc::Sender<UploadJob>,
    events: mpsc::Receiver<CloudEvent>,
    task: JoinHandle<()>,
}

impl SegmentedUploadSession {
    pub(crate) fn spawn(
        profile: ApiProfile,
        api_key: String,
        settings: RecognitionServiceSettings,
        language: String,
    ) -> Self {
        let max_audio_samples = if profile.provider == crate::providers::MIMO_PROVIDER {
            mimo::MAX_AUDIO_SAMPLES
        } else {
            MAX_AUDIO_SAMPLES
        };
        let (jobs_tx, jobs_rx) = mpsc::channel(UPLOAD_QUEUE_CAPACITY);
        let (events_tx, events_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let task = tokio::spawn(run_worker(
            UploadTarget::Profile {
                profile: Box::new(profile),
                api_key,
                settings,
            },
            language,
            jobs_rx,
            events_tx,
        ));
        Self {
            audio: Mutex::new(Vec::new()),
            max_audio_samples,
            jobs: jobs_tx,
            events: events_rx,
            task,
        }
    }

    pub(crate) fn spawn_managed_qwen(
        connection: QwenConnection,
        runtime: Arc<QwenRuntime>,
        language: String,
    ) -> Self {
        let (jobs_tx, jobs_rx) = mpsc::channel(UPLOAD_QUEUE_CAPACITY);
        let (events_tx, events_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let task = tokio::spawn(run_worker(
            UploadTarget::ManagedQwen {
                connection,
                runtime,
            },
            language,
            jobs_rx,
            events_tx,
        ));
        Self {
            audio: Mutex::new(Vec::new()),
            max_audio_samples: MAX_AUDIO_SAMPLES,
            jobs: jobs_tx,
            events: events_rx,
            task,
        }
    }

    pub async fn send(&self, samples: SharedAudio) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        let mut audio = self.audio.lock().await;
        if audio.len().saturating_add(samples.len()) > self.max_audio_samples {
            return Err(format!(
                "Transcription audio exceeds the {:.1}-second upload limit",
                self.max_audio_samples as f64 / 16_000.0
            ));
        }
        audio.extend_from_slice(samples.as_slice());
        Ok(())
    }

    pub async fn discard_pending(&self) {
        self.audio.lock().await.clear();
    }

    pub async fn commit(&self) -> Result<String, String> {
        self.commit_with_empty(false).await
    }

    pub(super) async fn commit_preview(&self) -> Result<String, String> {
        self.commit_with_empty(true).await
    }

    async fn commit_with_empty(&self, allow_empty: bool) -> Result<String, String> {
        let mut audio = self.audio.lock().await;
        if audio.is_empty() {
            return Err("Cannot commit empty transcription audio".into());
        }
        let utterance_id = format!("segmented-utterance-{}", uuid::Uuid::new_v4());
        let samples = std::mem::take(&mut *audio);
        match self.jobs.try_send(UploadJob {
            utterance_id: utterance_id.clone(),
            samples,
            allow_empty,
        }) {
            Ok(()) => Ok(utterance_id),
            Err(mpsc::error::TrySendError::Full(job)) => {
                *audio = job.samples;
                Err("Transcription upload queue is full".into())
            }
            Err(mpsc::error::TrySendError::Closed(job)) => {
                *audio = job.samples;
                Err("Transcription session is closed".into())
            }
        }
    }

    pub fn segmentation_mode(&self) -> SegmentationMode {
        SegmentationMode::LocalCommit
    }

    pub async fn recv(&mut self) -> Option<CloudEvent> {
        self.events.recv().await
    }

    pub(crate) fn cancel(&self) {
        self.task.abort();
    }

    pub async fn stop(self) {
        drop(self.jobs);
        self.task.abort();
        let _ = self.task.await;
    }

    pub async fn stop_and_drain(mut self) -> Vec<CloudEvent> {
        drop(self.jobs);
        let mut task = self.task;
        let mut drained = Vec::new();
        loop {
            tokio::select! {
                _ = &mut task => break,
                event = self.events.recv() => match event {
                    Some(event) => drained.push(event),
                    None => {
                        let _ = task.await;
                        break;
                    }
                },
            }
        }
        while let Ok(event) = self.events.try_recv() {
            drained.push(event);
        }
        drained
    }
}

async fn run_worker(
    mut target: UploadTarget,
    language: String,
    mut jobs: mpsc::Receiver<UploadJob>,
    events: mpsc::Sender<CloudEvent>,
) {
    let http = match &target {
        UploadTarget::Profile { profile, .. } => {
            Some(openai_audio_transcriptions::http_client(profile))
        }
        UploadTarget::ManagedQwen { .. } => None,
    };
    while let Some(job) = jobs.recv().await {
        let result = match &mut target {
            UploadTarget::Profile {
                profile,
                api_key,
                settings,
            } => {
                let http = http.as_ref().expect("profile client exists");
                if profile.provider == crate::providers::MIMO_PROVIDER {
                    mimo::transcribe(
                        http,
                        profile,
                        api_key,
                        settings,
                        &language,
                        &job.samples,
                        job.allow_empty,
                    )
                    .await
                } else {
                    openai_audio_transcriptions::transcribe_result_with_empty(
                        http,
                        profile,
                        api_key,
                        settings,
                        &language,
                        &job.samples,
                        job.allow_empty,
                    )
                    .await
                }
            }
            UploadTarget::ManagedQwen {
                connection,
                runtime,
            } => match runtime.reconnect(connection).await {
                Ok(current) => {
                    *connection = current;
                    let result = openai_audio_transcriptions::transcribe_managed_qwen(
                        connection,
                        &language,
                        &job.samples,
                    )
                    .await;
                    match runtime.connection().await {
                        Ok(Some(current)) if current.generation == connection.generation => result,
                        _ => Err("Managed Qwen ASR runtime changed during transcription".into()),
                    }
                }
                Err(error) => Err(error),
            },
        };
        let event = match result {
            Ok(result) => CloudEvent::Final {
                utterance_id: job.utterance_id,
                text: result.text,
                language: result.language,
            },
            Err(detail) => CloudEvent::Failed {
                utterance_id: Some(job.utterance_id),
                reset_session: false,
                code: "asr.cloud_transcription_failed".into(),
                detail,
            },
        };
        if events.send(event).await.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ApiAuthMode, ApiProfile, RecognitionServiceSettings};
    use crate::providers::OPENAI_COMPATIBLE_PROVIDER;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn mock_profile(request_count: usize) -> (ApiProfile, tokio::task::JoinHandle<usize>) {
        mock_profile_with_bodies(
            (0..request_count)
                .map(|index| format!(r#"{{"text":"language Chinese<asr_text>result-{index}"}}"#))
                .collect(),
        )
        .await
    }

    async fn mock_profile_with_bodies(
        bodies: Vec<String>,
    ) -> (ApiProfile, tokio::task::JoinHandle<usize>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let request_count = bodies.len();
            for body in bodies {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            }
            request_count
        });
        (
            ApiProfile {
                provider: OPENAI_COMPATIBLE_PROVIDER.into(),
                base_url: Some(format!("http://{address}/v1")),
                auth_mode: ApiAuthMode::None,
                ..ApiProfile::default()
            },
            task,
        )
    }

    async fn read_request(stream: &mut tokio::net::TcpStream) {
        let mut request = Vec::new();
        let mut buffer = [0u8; 4096];
        let mut expected = None;
        loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if expected.is_none() {
                if let Some(header_end) = request.windows(4).position(|value| value == b"\r\n\r\n")
                {
                    let headers = String::from_utf8_lossy(&request[..header_end]);
                    let content_length = headers
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(str::trim)
                                .map(str::to_owned)
                        })
                        .and_then(|value| value.parse::<usize>().ok())
                        .unwrap();
                    expected = Some(header_end + 4 + content_length);
                }
            }
            if expected.is_some_and(|length| request.len() >= length) {
                return;
            }
        }
    }

    fn settings() -> RecognitionServiceSettings {
        RecognitionServiceSettings {
            model: "whisper-test".into(),
            context: String::new(),
        }
    }

    #[tokio::test]
    async fn isolated_question_upload_returns_only_final_source_text() {
        let (mut profile, requests) = mock_profile(1).await;
        profile.provider = crate::providers::QWEN_LOCAL_PROVIDER.into();
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        let result = super::super::question::recognize_session(
            super::super::CloudRecognitionSession::Segmented(session),
            super::super::share_audio(vec![0.1; 320]),
        )
        .await
        .unwrap();
        assert_eq!(result, "result-0");
        assert_eq!(requests.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn empty_question_preview_then_final_succeeds() {
        for provider in [
            crate::providers::QWEN_LOCAL_PROVIDER,
            crate::providers::OPENAI_COMPATIBLE_PROVIDER,
        ] {
            let (mut profile, requests) = mock_profile_with_bodies(vec![
                r#"{"text":" "}"#.into(),
                r#"{"text":"final question"}"#.into(),
            ])
            .await;
            profile.provider = provider.into();
            let session =
                SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
            let tap = crate::vision::MicrophoneTap::default();
            tap.started();
            let start = std::time::Instant::now();
            let mut window = tap.begin(2).unwrap();
            let draft = Arc::new(std::sync::Mutex::new("waiting for preview".to_owned()));
            let mut recognition = Box::pin(super::super::question::recognize_live_session(
                super::super::CloudRecognitionSession::Segmented(session),
                window.reader(),
                draft.clone(),
            ));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(recognition.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            tokio::time::sleep(std::time::Duration::from_millis(800)).await;
            let preview_end = std::time::Instant::now();
            tap.push(&[0.1; 12800], start, preview_end);
            tokio::time::timeout(std::time::Duration::from_secs(1), async {
                loop {
                    tokio::select! {
                        result = &mut recognition => panic!("preview ended the question: {result:?}"),
                        _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {}
                    }
                    if draft.lock().unwrap().is_empty() {
                        break;
                    }
                }
            })
            .await
            .unwrap();
            window.release();
            tap.push(
                &[0.2; 320],
                preview_end,
                std::time::Instant::now() + std::time::Duration::from_millis(20),
            );
            let result = tokio::time::timeout(std::time::Duration::from_secs(1), recognition)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(result, "final question");
            assert_eq!(*draft.lock().unwrap(), "final question");
            assert_eq!(requests.await.unwrap(), 2);
        }
    }

    #[tokio::test]
    async fn released_question_skips_preview_and_uploads_final_once() {
        let (profile, requests) =
            mock_profile_with_bodies(vec![r#"{"text":"final question"}"#.into()]).await;
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        let tap = crate::vision::MicrophoneTap::default();
        tap.started();
        let start = std::time::Instant::now();
        let mut window = tap.begin(2).unwrap();
        let mut recognition = Box::pin(super::super::question::recognize_live_session(
            super::super::CloudRecognitionSession::Segmented(session),
            window.reader(),
            Arc::new(std::sync::Mutex::new(String::new())),
        ));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(recognition.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        window.release();
        tap.push(&[0.1; 12800], start, std::time::Instant::now());
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), recognition)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(result, "final question");
        assert_eq!(requests.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn empty_released_question_still_fails() {
        let (mut profile, requests) = mock_profile_with_bodies(vec![r#"{"text":""}"#.into()]).await;
        profile.provider = crate::providers::QWEN_LOCAL_PROVIDER.into();
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        let tap = crate::vision::MicrophoneTap::default();
        tap.started();
        let start = std::time::Instant::now();
        let mut window = tap.begin(1).unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        window.release();
        tap.push(&[0.1; 32], start, std::time::Instant::now());
        let error = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            super::super::question::recognize_live_session(
                super::super::CloudRecognitionSession::Segmented(session),
                window.reader(),
                Arc::new(std::sync::Mutex::new(String::new())),
            ),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("no final source text"));
        assert_eq!(requests.await.unwrap(), 1);
    }

    #[tokio::test]
    async fn question_drop_aborts_the_upload_worker() {
        let session = SegmentedUploadSession::spawn(
            ApiProfile::default(),
            String::new(),
            settings(),
            "auto".into(),
        );
        let worker = session.task.abort_handle();
        let mut future = Box::pin(super::super::question::recognize_session(
            super::super::CloudRecognitionSession::Segmented(session),
            super::super::share_audio(vec![0.1; 320]),
        ));
        // Poll exactly once to install the scope guard, without timing sleeps.
        std::future::poll_fn(|cx| {
            let _ = std::future::Future::poll(future.as_mut(), cx);
            std::task::Poll::Ready(())
        })
        .await;
        drop(future);
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !worker.is_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn qwen_auto_language_reaches_final_events() {
        let (mut profile, requests) = mock_profile(1).await;
        profile.provider = crate::providers::QWEN_LOCAL_PROVIDER.into();
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        session
            .send(super::super::share_audio(vec![0.1; 320]))
            .await
            .unwrap();
        let id = session.commit().await.unwrap();
        let events = session.stop_and_drain().await;
        assert_eq!(requests.await.unwrap(), 1);
        assert_eq!(
            events,
            vec![CloudEvent::Final {
                utterance_id: id,
                text: "result-0".into(),
                language: Some("zh".into()),
            }]
        );
    }

    #[tokio::test]
    async fn commits_are_uploaded_once_and_completed_in_order() {
        let (profile, requests) = mock_profile(2).await;
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        session
            .send(super::super::share_audio(vec![0.1; 320]))
            .await
            .unwrap();
        let first_id = session.commit().await.unwrap();
        session
            .send(super::super::share_audio(vec![0.2; 320]))
            .await
            .unwrap();
        let second_id = session.commit().await.unwrap();
        assert_ne!(first_id, second_id);

        let events = session.stop_and_drain().await;
        assert_eq!(requests.await.unwrap(), 2);
        assert_eq!(
            events,
            vec![
                CloudEvent::Final {
                    utterance_id: first_id,
                    text: "language Chinese<asr_text>result-0".into(),
                    language: None,
                },
                CloudEvent::Final {
                    utterance_id: second_id,
                    text: "language Chinese<asr_text>result-1".into(),
                    language: None,
                },
            ]
        );
    }

    #[tokio::test]
    async fn empty_audio_cannot_be_committed() {
        let session = SegmentedUploadSession::spawn(
            ApiProfile::default(),
            String::new(),
            RecognitionServiceSettings::default(),
            "auto".into(),
        );
        assert!(session.commit().await.unwrap_err().contains("empty"));
        session.stop().await;
    }

    #[tokio::test]
    async fn mimo_audio_buffer_respects_the_base64_upload_limit() {
        let session = SegmentedUploadSession::spawn(
            ApiProfile {
                provider: crate::providers::MIMO_PROVIDER.into(),
                ..ApiProfile::default()
            },
            "test-key".into(),
            RecognitionServiceSettings {
                model: "mimo-v2.5-asr".into(),
                context: String::new(),
            },
            "auto".into(),
        );
        session
            .send(super::super::share_audio(
                vec![0.0; super::super::mimo::MAX_AUDIO_SAMPLES],
            ))
            .await
            .unwrap();
        assert!(session
            .send(super::super::share_audio(vec![0.0]))
            .await
            .is_err());
        session.discard_pending().await;
        session
            .send(super::super::share_audio(vec![0.1; 320]))
            .await
            .unwrap();
        session.stop().await;
    }

    #[tokio::test]
    async fn audio_buffer_has_a_bounded_size() {
        let session = SegmentedUploadSession::spawn(
            ApiProfile::default(),
            String::new(),
            RecognitionServiceSettings::default(),
            "auto".into(),
        );
        assert!(session
            .send(super::super::share_audio(vec![0.0; MAX_AUDIO_SAMPLES + 1]))
            .await
            .is_err());
        session.stop().await;
    }
}
