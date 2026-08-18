use tokio::sync::{mpsc, Mutex};
use tokio::task::JoinHandle;

use crate::config::{ApiProfile, RecognitionServiceSettings};

use super::openai_audio_transcriptions;
use super::streaming::{CloudEvent, SegmentationMode};

const MAX_AUDIO_SECONDS: usize = 300;
const MAX_AUDIO_SAMPLES: usize = 16_000 * MAX_AUDIO_SECONDS;
const UPLOAD_QUEUE_CAPACITY: usize = 4;
const EVENT_QUEUE_CAPACITY: usize = 32;

struct UploadJob {
    utterance_id: String,
    samples: Vec<f32>,
}

pub struct SegmentedUploadSession {
    audio: Mutex<Vec<f32>>,
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
        let (jobs_tx, jobs_rx) = mpsc::channel(UPLOAD_QUEUE_CAPACITY);
        let (events_tx, events_rx) = mpsc::channel(EVENT_QUEUE_CAPACITY);
        let task = tokio::spawn(run_worker(
            profile, api_key, settings, language, jobs_rx, events_tx,
        ));
        Self {
            audio: Mutex::new(Vec::new()),
            jobs: jobs_tx,
            events: events_rx,
            task,
        }
    }

    pub async fn send(&self, samples: Vec<f32>) -> Result<(), String> {
        if samples.is_empty() {
            return Ok(());
        }
        let mut audio = self.audio.lock().await;
        if audio.len().saturating_add(samples.len()) > MAX_AUDIO_SAMPLES {
            return Err(format!(
                "Cloud transcription audio exceeds the {MAX_AUDIO_SECONDS}-second limit"
            ));
        }
        audio.extend(samples);
        Ok(())
    }

    pub async fn discard_pending(&self) {
        self.audio.lock().await.clear();
    }

    pub async fn commit(&self) -> Result<String, String> {
        let mut audio = self.audio.lock().await;
        if audio.is_empty() {
            return Err("Cannot commit empty cloud transcription audio".into());
        }
        let utterance_id = format!("segmented-utterance-{}", uuid::Uuid::new_v4());
        let samples = std::mem::take(&mut *audio);
        match self.jobs.try_send(UploadJob {
            utterance_id: utterance_id.clone(),
            samples,
        }) {
            Ok(()) => Ok(utterance_id),
            Err(mpsc::error::TrySendError::Full(job)) => {
                *audio = job.samples;
                Err("Cloud transcription upload queue is full".into())
            }
            Err(mpsc::error::TrySendError::Closed(job)) => {
                *audio = job.samples;
                Err("Cloud transcription session is closed".into())
            }
        }
    }

    pub fn segmentation_mode(&self) -> SegmentationMode {
        SegmentationMode::LocalCommit
    }

    pub async fn recv(&mut self) -> Option<CloudEvent> {
        self.events.recv().await
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
    profile: ApiProfile,
    api_key: String,
    settings: RecognitionServiceSettings,
    language: String,
    mut jobs: mpsc::Receiver<UploadJob>,
    events: mpsc::Sender<CloudEvent>,
) {
    let http = reqwest::Client::new();
    while let Some(job) = jobs.recv().await {
        let event = match openai_audio_transcriptions::transcribe(
            &http,
            &profile,
            &api_key,
            &settings,
            &language,
            &job.samples,
        )
        .await
        {
            Ok(text) => CloudEvent::Final {
                utterance_id: job.utterance_id,
                text,
                language: (language != "auto").then(|| language.clone()),
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
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            for index in 0..request_count {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_request(&mut stream).await;
                let body = format!(r#"{{"text":"result-{index}"}}"#);
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
    async fn commits_are_uploaded_once_and_completed_in_order() {
        let (profile, requests) = mock_profile(2).await;
        let session =
            SegmentedUploadSession::spawn(profile, String::new(), settings(), "auto".into());
        session.send(vec![0.1; 320]).await.unwrap();
        let first_id = session.commit().await.unwrap();
        session.send(vec![0.2; 320]).await.unwrap();
        let second_id = session.commit().await.unwrap();
        assert_ne!(first_id, second_id);

        let events = session.stop_and_drain().await;
        assert_eq!(requests.await.unwrap(), 2);
        assert_eq!(
            events,
            vec![
                CloudEvent::Final {
                    utterance_id: first_id,
                    text: "result-0".into(),
                    language: None,
                },
                CloudEvent::Final {
                    utterance_id: second_id,
                    text: "result-1".into(),
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
    async fn audio_buffer_has_a_bounded_size() {
        let session = SegmentedUploadSession::spawn(
            ApiProfile::default(),
            String::new(),
            RecognitionServiceSettings::default(),
            "auto".into(),
        );
        assert!(session
            .send(vec![0.0; MAX_AUDIO_SAMPLES + 1])
            .await
            .is_err());
        session.stop().await;
    }
}
