//! 音频采集 → VAD → ASR → SQLite/WebSocket 字幕发布管线。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::asr::{spawn_streaming_session, CloudEvent, SegmentationMode, StreamingSession};
use crate::audio::{AudioCapture, AudioError, CaptureSource};
use crate::config::{AsrConfig, VadConfig};
use crate::models::{AudioDevice, LiveTranscription};
use crate::vad::{SpeechSegmenter, VadRuntimeState, VoiceDetector};

mod dependencies;

pub(crate) use dependencies::PipelineDependencies;

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

    pub async fn start(
        &mut self,
        sample_rate: u32,
        device_id: Option<i64>,
        process_name: Option<&str>,
        trigger_threshold_dbfs: Option<f32>,
        vad_config: &VadConfig,
        asr_config: AsrConfig,
        dependencies: PipelineDependencies,
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
                format!("Audio startup task exited unexpectedly: {error}"),
            )
        })??;
        let cloud = if asr_config.backend == "local_whisper" {
            None
        } else {
            match spawn_streaming_session(asr_config.clone(), vad_config.silence_seconds).await {
                Ok(session) => Some(session),
                Err(error) if asr_config.cloud_failure_policy == "local" => {
                    dependencies.publish_live(LiveTranscription::Failed {
                        source: self.source_name.into(),
                        code: "asr.cloud_connect_failed".into(),
                        detail: error,
                    });
                    None
                }
                Err(error) => {
                    capture.shutdown().await;
                    return Err(AudioError::with_code("asr.cloud_connect_failed", error));
                }
            }
        };
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
                dependencies,
                source,
                cloud,
                asr_config.cloud_failure_policy == "local",
                sample_rate,
                trigger_threshold_dbfs,
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

async fn run(
    capture: &mut AudioCapture,
    mut detector: VoiceDetector,
    mut segmenter: SpeechSegmenter,
    dependencies: PipelineDependencies,
    source: &'static str,
    mut cloud: Option<StreamingSession>,
    local_fallback: bool,
    sample_rate: u32,
    trigger_threshold_dbfs: Option<f32>,
    shutdown: &mut watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let mut pre_roll = VecDeque::<(Vec<f32>, bool)>::new();
    let mut pre_roll_samples = 0usize;
    let pre_roll_limit = (sample_rate as f64 * 0.2) as usize;
    let mut streaming = false;
    let mut trigger_chunks = 0usize;
    let mut result = loop {
        if *shutdown.borrow() || *stop.borrow() {
            break Ok(());
        }
        let input = tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                break Ok(());
            }
            changed = stop.changed() => {
                let _ = changed;
                break Ok(());
            }
            event = async {
                match cloud.as_mut() {
                    Some(session) => session.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match event {
                    Some(event) => PipelineInput::Cloud(event),
                    None if local_fallback => {
                        cloud = None;
                        continue;
                    }
                    None => break Err("Cloud recognition session is closed".into()),
                }
            }
            chunk = capture.read() => match chunk {
                Ok(chunk) => PipelineInput::Audio(chunk),
                Err(error) => break Err(error.to_string()),
            },
        };
        let PipelineInput::Audio(chunk) = input else {
            if let PipelineInput::Cloud(event) = input {
                match event {
                    CloudEvent::Partial {
                        utterance_id,
                        text,
                        language,
                    } => {
                        dependencies.publish_live(LiveTranscription::Partial {
                            utterance_id,
                            source: source.into(),
                            text,
                            language,
                        });
                    }
                    CloudEvent::Final { text, language, .. } => {
                        dependencies.publish_text(text, language, source).await?;
                    }
                    CloudEvent::Failed { code, detail } => {
                        dependencies.publish_live(LiveTranscription::Failed {
                            source: source.into(),
                            code,
                            detail,
                        });
                        if local_fallback {
                            if let Some(session) = cloud.take() {
                                session.stop().await;
                            }
                        }
                    }
                }
            }
            continue;
        };
        let (rms_dbfs, peak_dbfs) = audio_level_dbfs(&chunk);
        let vad_speech = detector.is_speech(&chunk);
        let trigger_speech = thresholded_speech(vad_speech, rms_dbfs, trigger_threshold_dbfs);
        if trigger_threshold_dbfs.is_some() {
            dependencies.publish_live(LiveTranscription::AudioLevel {
                source: source.into(),
                rms_dbfs,
                peak_dbfs,
                speech: trigger_speech,
            });
        }
        if !streaming {
            if !confirm_trigger(&mut trigger_chunks, trigger_speech) {
                buffer_pre_roll(
                    &mut pre_roll,
                    &mut pre_roll_samples,
                    pre_roll_limit,
                    chunk,
                    vad_speech,
                );
                continue;
            }
            streaming = true;
            trigger_chunks = 0;
            while let Some((buffered, speech)) = pre_roll.pop_front() {
                if let Some(session) = cloud.as_ref() {
                    session.send(buffered.clone()).await?;
                }
                segmenter.push(&buffered, speech);
            }
            pre_roll_samples = 0;
        }

        if let Some(session) = cloud.as_ref() {
            session.send(chunk.clone()).await?;
        }
        let was_active = segmenter.is_active();
        let segment = segmenter.push(&chunk, vad_speech);
        let ended = was_active && !segmenter.is_active();
        if !ended {
            continue;
        }

        streaming = false;
        if let Some(session) = cloud.as_ref() {
            if session.segmentation_mode() == SegmentationMode::LocalCommit {
                session.commit().await?;
            }
        } else if let Some(segment) = segment {
            if let Err(error) = dependencies.transcribe_and_publish(segment, source).await {
                break Err(error);
            }
        }
    };
    if let Some(session) = cloud {
        for event in session.stop_and_drain().await {
            match event {
                CloudEvent::Final { text, language, .. } => {
                    if let Err(error) = dependencies.publish_text(text, language, source).await {
                        result = Err(error);
                        break;
                    }
                }
                CloudEvent::Partial {
                    utterance_id,
                    text,
                    language,
                } => {
                    dependencies.publish_live(LiveTranscription::Partial {
                        utterance_id,
                        source: source.into(),
                        text,
                        language,
                    });
                }
                CloudEvent::Failed { code, detail } => {
                    dependencies.publish_live(LiveTranscription::Failed {
                        source: source.into(),
                        code,
                        detail,
                    });
                }
            }
        }
    }
    capture.shutdown().await;
    result
}

enum PipelineInput {
    Audio(Vec<f32>),
    Cloud(CloudEvent),
}

const TRIGGER_CONFIRM_CHUNKS: usize = 2;

fn confirm_trigger(chunks: &mut usize, triggered: bool) -> bool {
    *chunks = if triggered { *chunks + 1 } else { 0 };
    *chunks >= TRIGGER_CONFIRM_CHUNKS
}

fn buffer_pre_roll(
    pre_roll: &mut VecDeque<(Vec<f32>, bool)>,
    samples: &mut usize,
    limit: usize,
    chunk: Vec<f32>,
    speech: bool,
) {
    *samples += chunk.len();
    pre_roll.push_back((chunk, speech));
    while *samples > limit {
        if let Some((removed, _)) = pre_roll.pop_front() {
            *samples = samples.saturating_sub(removed.len());
        }
    }
}

const MIN_AUDIO_LEVEL_DBFS: f32 = -80.0;

pub(crate) fn audio_level_dbfs(samples: &[f32]) -> (f32, f32) {
    if samples.is_empty() {
        return (MIN_AUDIO_LEVEL_DBFS, MIN_AUDIO_LEVEL_DBFS);
    }
    let mean_square =
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    let rms = mean_square.sqrt();
    let peak = samples.iter().copied().map(f32::abs).fold(0.0, f32::max);
    (amplitude_dbfs(rms), amplitude_dbfs(peak))
}

fn amplitude_dbfs(amplitude: f32) -> f32 {
    (20.0 * amplitude.max(0.0001).log10()).clamp(MIN_AUDIO_LEVEL_DBFS, 0.0)
}

fn thresholded_speech(
    vad_speech: bool,
    rms_dbfs: f32,
    trigger_threshold_dbfs: Option<f32>,
) -> bool {
    trigger_threshold_dbfs.map_or(vad_speech, |threshold| vad_speech && rms_dbfs >= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::{AsrEngine, AsrService, Transcription};
    use crate::config::AsrConfig;
    use crate::db::Database;
    use tokio::sync::broadcast;

    struct FakeEngine;

    #[test]
    fn audio_level_uses_dbfs_and_a_finite_silence_floor() {
        assert_eq!(audio_level_dbfs(&[0.0; 512]), (-80.0, -80.0));
        let (rms, peak) = audio_level_dbfs(&[0.01; 512]);
        assert!((rms + 40.0).abs() < 0.001);
        assert!((peak + 40.0).abs() < 0.001);
    }

    #[test]
    fn microphone_trigger_requires_vad_and_the_configured_level() {
        assert!(!thresholded_speech(true, -46.0, Some(-45.0)));
        assert!(!thresholded_speech(false, -30.0, Some(-45.0)));
        assert!(thresholded_speech(true, -45.0, Some(-45.0)));
        assert!(thresholded_speech(true, -80.0, None));
    }

    #[test]
    fn microphone_start_requires_two_consecutive_trigger_chunks() {
        let mut chunks = 0;
        assert!(!confirm_trigger(&mut chunks, true));
        assert!(!confirm_trigger(&mut chunks, false));
        assert!(!confirm_trigger(&mut chunks, true));
        assert!(confirm_trigger(&mut chunks, true));
    }

    #[test]
    fn pre_roll_keeps_only_the_configured_tail() {
        let mut chunks = VecDeque::new();
        let mut samples = 0;
        buffer_pre_roll(&mut chunks, &mut samples, 10, vec![0.0; 6], false);
        buffer_pre_roll(&mut chunks, &mut samples, 10, vec![0.0; 6], true);
        assert_eq!(chunks.len(), 1);
        assert_eq!(samples, 6);
        assert!(chunks.front().unwrap().1);
    }

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
            local: crate::config::LocalAsrConfig {
                model: "tiny".into(),
                device: "cpu".into(),
                compute_type: "int8".into(),
            },
            ..AsrConfig::default()
        };
        let asr = Arc::new(Mutex::new(AsrService::with_engine(
            config,
            Box::new(FakeEngine),
        )));
        let (tx, mut rx) = broadcast::channel(4);
        let (live_tx, _) = broadcast::channel(4);
        let (translation_tx, _) = broadcast::channel(4);
        let output = crate::subtitle_output::SubtitleLifecyclePublisher::new(
            tx,
            translation_tx,
            crate::osc::OscChatboxDispatcher::new(crate::config::OscConfig::default()),
        );
        let translation = crate::translation::TranslationDispatcher::new(
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::clone(&db),
            output.clone(),
        );
        let dependencies = PipelineDependencies::new(
            asr,
            Arc::clone(&db),
            live_tx,
            10,
            translation,
            crate::config::TranslationConfig::default(),
            Vec::new(),
            output,
        );

        dependencies
            .transcribe_and_publish(vec![0.0; 512], "microphone")
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
