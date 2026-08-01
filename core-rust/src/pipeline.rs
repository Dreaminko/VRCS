//! 音频采集 → VAD → ASR → SQLite/WebSocket 字幕发布管线。

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::asr::AsrService;
use crate::audio::{AudioCapture, AudioError, CaptureSource};
use crate::config::VadConfig;
use crate::db::Database;
use crate::models::{now_iso8601, AudioDevice, Subtitle};
use crate::vad::{SpeechSegmenter, VadRuntimeState, VoiceDetector};

pub struct TranscriptionPipeline {
    source: CaptureSource,
    source_name: &'static str,
    vad_model_path: PathBuf,
    vad_runtime: VadRuntimeState,
    shutdown: watch::Receiver<bool>,
    task: Option<JoinHandle<()>>,
    stop: Option<watch::Sender<bool>>,
    device: Option<AudioDevice>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl TranscriptionPipeline {
    pub fn new(
        source: CaptureSource,
        source_name: &'static str,
        vad_model_path: PathBuf,
        vad_runtime: VadRuntimeState,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            source,
            source_name,
            vad_model_path,
            vad_runtime,
            shutdown,
            task: None,
            stop: None,
            device: None,
            last_error: Arc::new(Mutex::new(None)),
        }
    }

    pub fn running(&self) -> bool {
        self.task.as_ref().is_some_and(|task| !task.is_finished())
    }

    pub fn device(&self) -> Option<&AudioDevice> {
        if self.running() {
            self.device.as_ref()
        } else {
            None
        }
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().expect("pipeline error lock").clone()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn start(
        &mut self,
        sample_rate: u32,
        device_id: Option<i64>,
        process_name: Option<&str>,
        vad_config: &VadConfig,
        asr: Arc<Mutex<AsrService>>,
        db: Arc<Mutex<Database>>,
        subtitles_tx: broadcast::Sender<Subtitle>,
        history_limit: u32,
    ) -> Result<AudioDevice, AudioError> {
        if self.running() {
            return Err(AudioError::with_code(
                "capture.already_running",
                "Transcription is already running",
            ));
        }
        self.task.take();
        self.stop.take();
        self.device = None;
        *self.last_error.lock().expect("pipeline error lock") = None;

        let source_kind = self.source;
        let process_name = process_name.map(str::to_owned);
        let (mut capture, device) = tokio::task::spawn_blocking(move || {
            let mut capture = AudioCapture::new(sample_rate, source_kind);
            let device = capture.start(device_id, process_name.as_deref())?;
            Ok::<_, AudioError>((capture, device))
        })
        .await
        .map_err(|error| {
            AudioError::with_code(
                "audio.start_task_failed",
                format!("音频启动任务异常退出：{error}"),
            )
        })??;
        let detector =
            VoiceDetector::load_with_runtime(&self.vad_model_path, self.vad_runtime.clone());
        let segmenter = SpeechSegmenter::new(
            sample_rate,
            vad_config.silence_seconds,
            vad_config.max_speech_seconds,
        );
        let source = self.source_name;
        let last_error = Arc::clone(&self.last_error);
        let mut shutdown = self.shutdown.clone();
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        self.task = Some(tokio::spawn(async move {
            if let Err(error) = run(
                &mut capture,
                detector,
                segmenter,
                asr,
                db,
                subtitles_tx,
                history_limit,
                source,
                &mut shutdown,
                stop_rx,
            )
            .await
            {
                *last_error.lock().expect("pipeline error lock") = Some(error.clone());
                tracing::warn!(source, %error, "transcription pipeline stopped");
            }
        }));
        self.device = Some(device.clone());
        Ok(device)
    }

    pub async fn stop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
        self.device = None;
    }
}

impl Drop for TranscriptionPipeline {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn run(
    capture: &mut AudioCapture,
    mut detector: VoiceDetector,
    mut segmenter: SpeechSegmenter,
    asr: Arc<Mutex<AsrService>>,
    db: Arc<Mutex<Database>>,
    subtitles_tx: broadcast::Sender<Subtitle>,
    history_limit: u32,
    source: &'static str,
    shutdown: &mut watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let result = loop {
        if *shutdown.borrow() || *stop.borrow() {
            break Ok(());
        }
        let chunk = tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break Ok(());
            }
            changed = stop.changed() => {
                let _ = changed;
                break Ok(());
            }
            chunk = capture.read() => match chunk {
                Ok(chunk) => chunk,
                Err(error) => break Err(error.to_string()),
            },
        };
        let speech = detector.is_speech(&chunk);
        let Some(segment) = segmenter.push(&chunk, speech) else {
            continue;
        };
        if let Err(error) =
            transcribe_and_publish(segment, &asr, &db, &subtitles_tx, history_limit, source).await
        {
            break Err(error);
        }
    };
    capture.shutdown().await;
    result
}

async fn transcribe_and_publish(
    segment: Vec<f32>,
    asr: &Arc<Mutex<AsrService>>,
    db: &Arc<Mutex<Database>>,
    subtitles_tx: &broadcast::Sender<Subtitle>,
    history_limit: u32,
    source: &'static str,
) -> Result<(), String> {
    let rms = (segment.iter().map(|sample| sample * sample).sum::<f32>()
        / segment.len().max(1) as f32)
        .sqrt();
    let peak = segment.iter().copied().map(f32::abs).fold(0.0, f32::max);
    tracing::debug!(
        source,
        samples = segment.len(),
        duration_seconds = segment.len() as f64 / 16_000.0,
        rms,
        peak,
        "sending speech segment to ASR"
    );
    let transcriber = Arc::clone(asr);
    let transcription = tokio::task::spawn_blocking(move || {
        transcriber.lock().expect("asr lock").transcribe(&segment)
    })
    .await
    .map_err(|error| format!("识别任务异常退出：{error}"))??;
    tracing::debug!(
        source,
        text_length = transcription.text.chars().count(),
        language = transcription.language.as_deref().unwrap_or("unknown"),
        "ASR transcription completed"
    );
    if transcription.text.is_empty() {
        return Ok(());
    }

    let subtitle = Subtitle {
        id: None,
        text: transcription.text,
        language: transcription.language,
        started_at: None,
        ended_at: None,
        source: source.into(),
        created_at: now_iso8601(),
    };
    let database = Arc::clone(db);
    let saved = tokio::task::spawn_blocking(move || {
        database
            .lock()
            .map_err(|_| "数据库锁不可用".to_string())?
            .add_subtitle(&subtitle, history_limit)
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("字幕存储任务异常退出：{error}"))??;
    let _ = subtitles_tx.send(saved);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::{AsrEngine, Transcription};
    use crate::config::AsrConfig;

    struct FakeEngine;

    impl AsrEngine for FakeEngine {
        fn transcribe(
            &mut self,
            _samples: &[f32],
            _language: Option<&str>,
        ) -> Result<Transcription, String> {
            Ok(Transcription {
                text: "こんにちは".into(),
                language: Some("ja".into()),
            })
        }
    }

    #[tokio::test]
    async fn transcription_is_persisted_and_broadcast() {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Mutex::new(
            Database::open(&directory.path().join("test.db")).unwrap(),
        ));
        let config = AsrConfig {
            model: "tiny".into(),
            language: "auto".into(),
            device: "cpu".into(),
            compute_type: "int8".into(),
        };
        let asr = Arc::new(Mutex::new(AsrService::with_engine(
            config,
            Box::new(FakeEngine),
        )));
        let (tx, mut rx) = broadcast::channel(4);

        transcribe_and_publish(vec![0.0; 512], &asr, &db, &tx, 10, "microphone")
            .await
            .unwrap();

        let published = rx.recv().await.unwrap();
        assert_eq!(published.text, "こんにちは");
        assert_eq!(published.source, "microphone");
        let history = db.lock().unwrap().subtitle_history(10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].language.as_deref(), Some("ja"));
    }

    #[tokio::test]
    async fn stop_waits_for_running_blocking_work() {
        let (_shutdown_tx, shutdown_rx) = watch::channel(false);
        let mut pipeline = TranscriptionPipeline::new(
            CaptureSource::Speaker,
            "speaker",
            PathBuf::new(),
            VadRuntimeState::default(),
            shutdown_rx,
        );
        let (stop_tx, mut stop_rx) = watch::channel(false);
        let (stop_seen_tx, stop_seen_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        pipeline.stop = Some(stop_tx);
        pipeline.task = Some(tokio::spawn(async move {
            stop_rx.changed().await.unwrap();
            stop_seen_tx.send(()).unwrap();
            tokio::task::spawn_blocking(move || release_rx.recv().unwrap())
                .await
                .unwrap();
        }));

        let stopping = tokio::spawn(async move { pipeline.stop().await });
        stop_seen_rx.await.unwrap();
        assert!(!stopping.is_finished());
        release_tx.send(()).unwrap();
        stopping.await.unwrap();
    }
}
