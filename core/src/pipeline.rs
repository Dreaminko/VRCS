//! 音频采集 → VAD → ASR → SQLite/WebSocket 字幕发布管线。

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::asr::{
    share_audio, spawn_cloud_recognition_session, CloudEvent, CloudRecognitionSession,
    SegmentationMode,
};
use crate::audio::{AudioCapture, AudioError, CaptureSource};
use crate::config::{AsrConfig, VadConfig};
use crate::models::{AudioDevice, LiveTranscription};
use crate::smart_turn::{SmartTurnRuntime, COMPLETION_THRESHOLD};
use crate::vad::{SpeechSegmenter, VadRuntimeState, VoiceDetector};

mod dependencies;
mod recognition_lifecycle;

const AUDIO_LEVEL_PUBLISH_INTERVAL: Duration = Duration::from_millis(80);
const PARTIAL_PUBLISH_INTERVAL: Duration = Duration::from_millis(80);
const SMART_TURN_TIMEOUT: Duration = Duration::from_secs(1);

pub(crate) use dependencies::PipelineDependencies;
use recognition_lifecycle::RecognitionLifecycle;

#[derive(Clone, Default)]
pub(crate) struct AsrEchoGuard {
    signatures: Vec<String>,
    repeated_world: Option<String>,
}

impl AsrEchoGuard {
    pub(crate) fn new(signatures: Vec<String>, repeated_world: Option<String>) -> Self {
        let mut signatures = signatures
            .into_iter()
            .map(|text| normalize_echo_text(&text))
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>();
        signatures.sort_unstable();
        signatures.dedup();
        Self {
            signatures,
            repeated_world: repeated_world.map(|world| normalize_echo_text(&world)),
        }
    }

    fn is_echo(&self, text: &str) -> bool {
        let text = normalize_echo_text(text);
        !text.is_empty()
            && (self.signatures.iter().any(|signature| signature == &text)
                || self.is_repeated_world(&text, false))
    }

    fn suppresses_partial(&self, text: &str) -> bool {
        let text = normalize_echo_text(text);
        (text.contains(':')
            && self
                .signatures
                .iter()
                .any(|signature| signature.starts_with(&text)))
            || self.is_repeated_world(&text, true)
    }

    fn is_repeated_world(&self, text: &str, allow_trailing_prefix: bool) -> bool {
        let Some(world) = self
            .repeated_world
            .as_deref()
            .filter(|world| !world.is_empty())
        else {
            return false;
        };
        let mut remaining = text;
        let mut count = 0;
        loop {
            let Some(rest) = remaining.strip_prefix(world) else {
                return allow_trailing_prefix && count >= 1 && world.starts_with(remaining);
            };
            count += 1;
            remaining = rest.trim_start_matches([
                ' ', ',', '，', ';', '；', '/', '|', '.', '。', '!', '！', '?', '？', ':', '：',
            ]);
            if remaining.is_empty() {
                return count >= 2;
            }
        }
    }
}

fn normalize_echo_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim_end_matches(['.', '。', '!', '！', '?', '？'])
        .to_lowercase()
}

pub struct TranscriptionPipeline {
    source: CaptureSource,
    source_name: &'static str,
    vad_model_path: PathBuf,
    vad_runtime: VadRuntimeState,
    smart_turn_runtime: SmartTurnRuntime,
    shutdown: watch::Receiver<bool>,
    task: Option<JoinHandle<()>>,
    stop: Option<watch::Sender<bool>>,
    device: Option<AudioDevice>,
    last_error: Arc<Mutex<Option<String>>>,
    discard_on_stop: Arc<AtomicBool>,
}

impl TranscriptionPipeline {
    pub fn new(
        source: CaptureSource,
        source_name: &'static str,
        vad_model_path: PathBuf,
        vad_runtime: VadRuntimeState,
        smart_turn_runtime: SmartTurnRuntime,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            source,
            source_name,
            vad_model_path,
            vad_runtime,
            smart_turn_runtime,
            shutdown,
            task: None,
            stop: None,
            device: None,
            last_error: Arc::new(Mutex::new(None)),
            discard_on_stop: Arc::new(AtomicBool::new(false)),
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
        trigger_threshold_dbfs: Option<f32>,
        vad_config: &VadConfig,
        asr_config: AsrConfig,
        asr_echo_guard: AsrEchoGuard,
        dependencies: PipelineDependencies,
    ) -> Result<AudioDevice, AudioError> {
        if self.running() {
            return Err(AudioError::with_code(
                "capture.already_running",
                "Transcription is already running",
            ));
        }
        let smart_turn = if vad_config.endpointing == "smart_turn"
            && asr_config.backend != crate::providers::SERVICE_FUN_ASR_REALTIME
        {
            match self.smart_turn_runtime.prepare().await {
                Ok(()) => Some(self.smart_turn_runtime.clone()),
                Err(error) => {
                    tracing::warn!(%error, "Smart Turn is unavailable; using silence endpointing");
                    None
                }
            }
        } else {
            None
        };
        self.task.take();
        self.stop.take();
        self.device = None;
        *self.last_error.lock().expect("pipeline error lock") = None;
        self.discard_on_stop.store(false, Ordering::SeqCst);

        let startup_started = Instant::now();
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
        let audio_ms = startup_started.elapsed().as_millis();
        let cloud_started = Instant::now();
        let cloud = if asr_config.backend == "local_whisper" {
            None
        } else {
            match spawn_cloud_recognition_session(asr_config.clone(), vad_config.silence_seconds)
                .await
            {
                Ok(session) => Some(session),
                Err(error) if asr_config.cloud_failure_policy == "local" => {
                    dependencies.publish_live(LiveTranscription::Failed {
                        utterance_id: None,
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
        let cloud_ms = cloud_started.elapsed().as_millis();
        let vad_started = Instant::now();
        let detector =
            VoiceDetector::load_with_runtime(&self.vad_model_path, self.vad_runtime.clone());
        let vad_ms = vad_started.elapsed().as_millis();
        let segmenter = if smart_turn.is_some() {
            SpeechSegmenter::smart(
                sample_rate,
                vad_config.silence_seconds,
                vad_config.max_speech_seconds,
            )
        } else {
            SpeechSegmenter::new(
                sample_rate,
                vad_config.silence_seconds,
                vad_config.max_speech_seconds,
            )
        };
        let source = self.source_name;

        let last_error = Arc::clone(&self.last_error);
        let discard_on_stop = Arc::clone(&self.discard_on_stop);
        let mut shutdown = self.shutdown.clone();
        let (stop_tx, stop_rx) = watch::channel(false);
        self.stop = Some(stop_tx);
        self.task = Some(tokio::spawn(async move {
            if let Err(error) = run(
                &mut capture,
                detector,
                segmenter,
                PipelineRunContext {
                    dependencies,
                    source,
                    cloud,
                    local_fallback: asr_config.cloud_failure_policy == "local",
                    echo_guard: asr_echo_guard,
                    sample_rate,
                    trigger_threshold_dbfs,
                    discard_on_stop,
                    smart_turn,
                },
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
        tracing::info!(
            source = self.source_name,
            audio_ms,
            cloud_ms,
            vad_ms,
            total_ms = startup_started.elapsed().as_millis(),
            "transcription pipeline ready"
        );
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

    pub async fn stop_discarding_results(&mut self) {
        self.discard_on_stop.store(true, Ordering::SeqCst);
        self.stop().await;
    }
}

impl Drop for TranscriptionPipeline {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(true);
        }
    }
}

struct PipelineRunContext {
    dependencies: PipelineDependencies,
    source: &'static str,
    cloud: Option<CloudRecognitionSession>,
    local_fallback: bool,
    echo_guard: AsrEchoGuard,
    sample_rate: u32,
    trigger_threshold_dbfs: Option<f32>,
    discard_on_stop: Arc<AtomicBool>,
    smart_turn: Option<SmartTurnRuntime>,
}

struct PipelineState {
    pre_roll: VecDeque<(Vec<f32>, bool)>,
    pre_roll_samples: usize,
    pre_roll_limit: usize,
    streaming: bool,
    trigger_chunks: usize,
    last_audio_level_published_at: Option<Instant>,
    last_partial_published_at: Option<Instant>,
    lifecycle: RecognitionLifecycle,
}

impl PipelineState {
    fn new(sample_rate: u32) -> Self {
        Self {
            pre_roll: VecDeque::new(),
            pre_roll_samples: 0,
            pre_roll_limit: (sample_rate as f64 * 0.2) as usize,
            streaming: false,
            trigger_chunks: 0,
            last_audio_level_published_at: None,
            last_partial_published_at: None,
            lifecycle: RecognitionLifecycle::default(),
        }
    }
}

#[derive(Clone, Copy)]
enum PartialPublication {
    Throttled(Instant),
    Immediate,
}

enum PipelineEvent {
    Cloud {
        event: CloudEvent,
        partial_publication: PartialPublication,
        stop_cloud_on_failure: bool,
    },
    AudioAnalyzed {
        chunk: Vec<f32>,
        rms_dbfs: f32,
        peak_dbfs: f32,
        vad_speech: bool,
        trigger_speech: bool,
        publish_audio_level: bool,
        now: Instant,
    },
    SegmentEnded {
        segment: Option<Vec<f32>>,
        backend: RecognitionBackend,
    },
    CloudCommitted(Option<String>),
}

#[derive(Clone, Copy)]
enum RecognitionBackend {
    Local,
    Cloud(SegmentationMode),
}

enum PipelineEffect {
    PublishPartial {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    PublishFinal {
        utterance_id: String,
        text: String,
        language: Option<String>,
    },
    PublishFailed {
        utterance_id: Option<String>,
        code: String,
        detail: String,
    },
    PublishAudioLevel {
        rms_dbfs: f32,
        peak_dbfs: f32,
        speech: bool,
    },
    StopCloud,
    FlushPreRoll(Vec<(Vec<f32>, bool)>),
    ProcessAudio {
        chunk: Vec<f32>,
        speech: bool,
    },
    CommitCloud {
        has_segment: bool,
    },
    TranscribeLocal(Vec<f32>),
}

fn reduce_pipeline_event(
    state: &mut PipelineState,
    event: PipelineEvent,
    echo_guard: &AsrEchoGuard,
) -> Vec<PipelineEffect> {
    match event {
        PipelineEvent::Cloud {
            event,
            partial_publication,
            stop_cloud_on_failure,
        } => reduce_cloud_event(
            state,
            event,
            partial_publication,
            stop_cloud_on_failure,
            echo_guard,
        ),
        PipelineEvent::AudioAnalyzed {
            chunk,
            rms_dbfs,
            peak_dbfs,
            vad_speech,
            trigger_speech,
            publish_audio_level,
            now,
        } => reduce_audio_event(
            state,
            chunk,
            AudioAnalysis {
                rms_dbfs,
                peak_dbfs,
                vad_speech,
                trigger_speech,
                publish_audio_level,
                now,
            },
        ),
        PipelineEvent::SegmentEnded { segment, backend } => {
            state.streaming = false;
            match (backend, segment) {
                (RecognitionBackend::Cloud(SegmentationMode::LocalCommit), segment) => {
                    vec![PipelineEffect::CommitCloud {
                        has_segment: segment.is_some(),
                    }]
                }
                (RecognitionBackend::Cloud(SegmentationMode::ServerVad), _) => Vec::new(),
                (RecognitionBackend::Local, Some(segment)) => {
                    vec![PipelineEffect::TranscribeLocal(segment)]
                }
                (RecognitionBackend::Local, None) => Vec::new(),
            }
        }
        PipelineEvent::CloudCommitted(Some(utterance_id)) => {
            state.lifecycle.begin(&utterance_id);
            Vec::new()
        }
        PipelineEvent::CloudCommitted(None) => Vec::new(),
    }
}

fn reduce_cloud_event(
    state: &mut PipelineState,
    event: CloudEvent,
    partial_publication: PartialPublication,
    stop_cloud_on_failure: bool,
    echo_guard: &AsrEchoGuard,
) -> Vec<PipelineEffect> {
    match event {
        CloudEvent::Partial {
            utterance_id,
            text,
            language,
        } => {
            if echo_guard.suppresses_partial(&text) {
                return Vec::new();
            }
            let publication_ready = match partial_publication {
                PartialPublication::Throttled(now) => {
                    should_publish_partial(&mut state.last_partial_published_at, now)
                }
                PartialPublication::Immediate => true,
            };
            if !publication_ready || !state.lifecycle.accept_partial(&utterance_id) {
                return Vec::new();
            }
            vec![PipelineEffect::PublishPartial {
                utterance_id,
                text,
                language,
            }]
        }
        CloudEvent::Final {
            utterance_id,
            text,
            language,
        } => state
            .lifecycle
            .accept_final(&utterance_id)
            .then_some(PipelineEffect::PublishFinal {
                utterance_id,
                text,
                language,
            })
            .into_iter()
            .collect(),
        CloudEvent::Failed {
            utterance_id,
            reset_session,
            code,
            detail,
        } => {
            let utterance_id = if reset_session {
                state.lifecycle.reset();
                None
            } else {
                let utterance_id = state.lifecycle.failure_id(utterance_id.as_deref());
                if let Some(utterance_id) = &utterance_id {
                    state.lifecycle.terminate(utterance_id);
                }
                utterance_id
            };
            let mut effects = vec![PipelineEffect::PublishFailed {
                utterance_id,
                code,
                detail,
            }];
            if stop_cloud_on_failure {
                effects.push(PipelineEffect::StopCloud);
            }
            effects
        }
    }
}

struct AudioAnalysis {
    rms_dbfs: f32,
    peak_dbfs: f32,
    vad_speech: bool,
    trigger_speech: bool,
    publish_audio_level: bool,
    now: Instant,
}

fn reduce_audio_event(
    state: &mut PipelineState,
    chunk: Vec<f32>,
    analysis: AudioAnalysis,
) -> Vec<PipelineEffect> {
    let mut effects = Vec::new();
    if analysis.publish_audio_level
        && should_publish_audio_level(&mut state.last_audio_level_published_at, analysis.now)
    {
        effects.push(PipelineEffect::PublishAudioLevel {
            rms_dbfs: analysis.rms_dbfs,
            peak_dbfs: analysis.peak_dbfs,
            speech: analysis.trigger_speech,
        });
    }
    if !state.streaming {
        if !confirm_trigger(&mut state.trigger_chunks, analysis.trigger_speech) {
            buffer_pre_roll(
                &mut state.pre_roll,
                &mut state.pre_roll_samples,
                state.pre_roll_limit,
                chunk,
                analysis.vad_speech,
            );
            return effects;
        }
        state.streaming = true;
        state.trigger_chunks = 0;
        state.pre_roll_samples = 0;
        let pre_roll = std::mem::take(&mut state.pre_roll);
        if !pre_roll.is_empty() {
            effects.push(PipelineEffect::FlushPreRoll(pre_roll.into()));
        }
    }
    effects.push(PipelineEffect::ProcessAudio {
        chunk,
        speech: analysis.vad_speech,
    });
    effects
}

struct PipelineEffectRunner<'a> {
    dependencies: &'a PipelineDependencies,
    source: &'static str,
    echo_guard: &'a AsrEchoGuard,
    cloud: &'a mut Option<CloudRecognitionSession>,
    segmenter: &'a mut SpeechSegmenter,
    smart_turn: Option<&'a SmartTurnRuntime>,
    smart_turn_tx: &'a mpsc::UnboundedSender<SmartTurnResult>,
}

struct SmartTurnResult {
    generation: u64,
    result: Result<f32, String>,
    elapsed: Duration,
}

fn smart_turn_should_complete(result: &Result<f32, String>) -> bool {
    matches!(result, Ok(probability) if *probability > COMPLETION_THRESHOLD)
}

impl PipelineEffectRunner<'_> {
    async fn execute(
        &mut self,
        state: &mut PipelineState,
        effects: Vec<PipelineEffect>,
    ) -> Result<(), String> {
        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            let follow_up = self.execute_one(effect).await?;
            if let Some(event) = follow_up {
                effects.extend(reduce_pipeline_event(state, event, self.echo_guard));
            }
        }
        Ok(())
    }

    async fn execute_one(
        &mut self,
        effect: PipelineEffect,
    ) -> Result<Option<PipelineEvent>, String> {
        match effect {
            PipelineEffect::PublishPartial {
                utterance_id,
                text,
                language,
            } => self.dependencies.publish_live(LiveTranscription::Partial {
                utterance_id,
                source: self.source.into(),
                text,
                language,
            }),
            PipelineEffect::PublishFinal {
                utterance_id,
                text,
                language,
            } => {
                publish_cloud_final(
                    self.dependencies,
                    self.source,
                    self.echo_guard,
                    utterance_id,
                    text,
                    language,
                )
                .await?;
            }
            PipelineEffect::PublishFailed {
                utterance_id,
                code,
                detail,
            } => self.dependencies.publish_live(LiveTranscription::Failed {
                utterance_id,
                source: self.source.into(),
                code,
                detail,
            }),
            PipelineEffect::PublishAudioLevel {
                rms_dbfs,
                peak_dbfs,
                speech,
            } => self
                .dependencies
                .publish_live(LiveTranscription::AudioLevel {
                    source: self.source.into(),
                    rms_dbfs,
                    peak_dbfs,
                    speech,
                }),
            PipelineEffect::StopCloud => {
                if let Some(session) = self.cloud.take() {
                    session.stop().await;
                }
            }
            PipelineEffect::FlushPreRoll(chunks) => {
                for (chunk, speech) in chunks {
                    if let Some(session) = self.cloud.as_ref() {
                        let chunk = share_audio(chunk);
                        session.send(Arc::clone(&chunk)).await?;
                        self.segmenter.push(chunk.as_slice(), speech);
                    } else {
                        self.segmenter.push(&chunk, speech);
                    }
                }
            }
            PipelineEffect::ProcessAudio { chunk, speech } => {
                let was_active = self.segmenter.is_active();
                let segment = if let Some(session) = self.cloud.as_ref() {
                    let chunk = share_audio(chunk);
                    session.send(Arc::clone(&chunk)).await?;
                    self.segmenter.push(chunk.as_slice(), speech)
                } else {
                    self.segmenter.push(&chunk, speech)
                };
                if was_active && !self.segmenter.is_active() {
                    let backend = self
                        .cloud
                        .as_ref()
                        .map_or(RecognitionBackend::Local, |session| {
                            RecognitionBackend::Cloud(session.segmentation_mode())
                        });
                    return Ok(Some(PipelineEvent::SegmentEnded { segment, backend }));
                }
                if let Some((generation, audio)) = self.segmenter.take_candidate() {
                    let runtime = self
                        .smart_turn
                        .expect("smart endpointing requires a Smart Turn runtime")
                        .clone();
                    let tx = self.smart_turn_tx.clone();
                    tokio::spawn(async move {
                        let started = Instant::now();
                        let result =
                            tokio::time::timeout(SMART_TURN_TIMEOUT, runtime.predict(audio))
                                .await
                                .map_err(|_| "Smart Turn inference timed out".to_string())
                                .and_then(|result| result);
                        let _ = tx.send(SmartTurnResult {
                            generation,
                            result,
                            elapsed: started.elapsed(),
                        });
                    });
                }
            }
            PipelineEffect::CommitCloud { has_segment } => {
                let utterance_id = match self.cloud.as_ref() {
                    Some(session) => session.commit(has_segment).await?,
                    None => None,
                };
                return Ok(Some(PipelineEvent::CloudCommitted(utterance_id)));
            }
            PipelineEffect::TranscribeLocal(segment) => {
                self.dependencies
                    .transcribe_and_publish(segment, self.source)
                    .await?;
            }
        }
        Ok(None)
    }
}

async fn run(
    capture: &mut AudioCapture,
    mut detector: VoiceDetector,
    mut segmenter: SpeechSegmenter,
    context: PipelineRunContext,
    shutdown: &mut watch::Receiver<bool>,
    mut stop: watch::Receiver<bool>,
) -> Result<(), String> {
    let PipelineRunContext {
        dependencies,
        source,
        mut cloud,
        local_fallback,
        echo_guard,
        sample_rate,
        trigger_threshold_dbfs,
        discard_on_stop,
        smart_turn,
    } = context;
    let mut state = PipelineState::new(sample_rate);
    let (smart_turn_tx, mut smart_turn_rx) = mpsc::unbounded_channel();
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
            result = smart_turn_rx.recv(), if smart_turn.is_some() => match result {
                Some(result) => PipelineInput::SmartTurn(result),
                None => continue,
            },
        };
        let event = match input {
            PipelineInput::Cloud(event) => PipelineEvent::Cloud {
                event,
                partial_publication: PartialPublication::Throttled(Instant::now()),
                stop_cloud_on_failure: local_fallback,
            },
            PipelineInput::Audio(chunk) => {
                let (rms_dbfs, peak_dbfs) = audio_level_dbfs(&chunk);
                let vad_speech = detector.is_speech(&chunk);
                PipelineEvent::AudioAnalyzed {
                    chunk,
                    rms_dbfs,
                    peak_dbfs,
                    vad_speech,
                    trigger_speech: thresholded_speech(
                        vad_speech,
                        rms_dbfs,
                        trigger_threshold_dbfs,
                    ),
                    publish_audio_level: trigger_threshold_dbfs.is_some(),
                    now: Instant::now(),
                }
            }
            PipelineInput::SmartTurn(result) => {
                let inference_ms = result.elapsed.as_millis();
                let complete = smart_turn_should_complete(&result.result);
                match result.result {
                    Ok(probability) if complete => {
                        tracing::debug!(
                            probability,
                            inference_ms,
                            decision = "complete",
                            "Smart Turn endpoint prediction"
                        );
                    }
                    Ok(probability) => {
                        tracing::debug!(
                            probability,
                            inference_ms,
                            decision = "incomplete_wait",
                            "Smart Turn endpoint prediction"
                        );
                    }
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            inference_ms,
                            decision = "unknown_wait",
                            "Smart Turn failed; waiting for continuation or hard silence"
                        );
                    }
                }
                if !complete {
                    continue;
                }
                let Some(segment) = segmenter.complete_candidate(result.generation) else {
                    continue;
                };
                let backend = cloud.as_ref().map_or(RecognitionBackend::Local, |session| {
                    RecognitionBackend::Cloud(session.segmentation_mode())
                });
                PipelineEvent::SegmentEnded { segment, backend }
            }
        };
        let effects = reduce_pipeline_event(&mut state, event, &echo_guard);
        let mut runner = PipelineEffectRunner {
            dependencies: &dependencies,
            source,
            echo_guard: &echo_guard,
            cloud: &mut cloud,
            segmenter: &mut segmenter,
            smart_turn: smart_turn.as_ref(),
            smart_turn_tx: &smart_turn_tx,
        };
        if let Err(error) = runner.execute(&mut state, effects).await {
            break Err(error);
        }
    };
    if let Some(session) = cloud {
        if discard_on_stop.load(Ordering::SeqCst) {
            session.stop().await;
            finish_recognition_session(&dependencies, source, &mut state.lifecycle);
            capture.shutdown().await;
            return result;
        }
        for event in session.stop_and_drain().await {
            let effects = reduce_pipeline_event(
                &mut state,
                PipelineEvent::Cloud {
                    event,
                    partial_publication: PartialPublication::Immediate,
                    stop_cloud_on_failure: false,
                },
                &echo_guard,
            );
            let mut no_cloud = None;
            let mut runner = PipelineEffectRunner {
                dependencies: &dependencies,
                source,
                echo_guard: &echo_guard,
                cloud: &mut no_cloud,
                segmenter: &mut segmenter,
                smart_turn: None,
                smart_turn_tx: &smart_turn_tx,
            };
            if let Err(error) = runner.execute(&mut state, effects).await {
                result = Err(error);
                break;
            }
        }
    }
    finish_recognition_session(&dependencies, source, &mut state.lifecycle);
    capture.shutdown().await;
    result
}

async fn publish_cloud_final(
    dependencies: &PipelineDependencies,
    source: &'static str,
    echo_guard: &AsrEchoGuard,
    utterance_id: String,
    text: String,
    language: Option<String>,
) -> Result<(), String> {
    if echo_guard.is_echo(&text) {
        tracing::debug!(source, "discarded echoed ASR context");
        dependencies.cancel_recognition(&utterance_id, source, "filtered");
        return Ok(());
    }
    dependencies
        .publish_text(text, language, source, utterance_id)
        .await
}

fn finish_recognition_session(
    dependencies: &PipelineDependencies,
    source: &str,
    lifecycle: &mut RecognitionLifecycle,
) {
    for utterance_id in lifecycle.terminate_all() {
        dependencies.cancel_recognition(&utterance_id, source, "stopped");
    }
    dependencies.reset_recognition(source);
    lifecycle.reset();
}

enum PipelineInput {
    Audio(Vec<f32>),
    Cloud(CloudEvent),
    SmartTurn(SmartTurnResult),
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

fn should_publish_audio_level(last_published_at: &mut Option<Instant>, now: Instant) -> bool {
    should_publish_at_interval(last_published_at, now, AUDIO_LEVEL_PUBLISH_INTERVAL)
}

fn should_publish_partial(last_published_at: &mut Option<Instant>, now: Instant) -> bool {
    should_publish_at_interval(last_published_at, now, PARTIAL_PUBLISH_INTERVAL)
}

fn should_publish_at_interval(
    last_published_at: &mut Option<Instant>,
    now: Instant,
    interval: Duration,
) -> bool {
    if last_published_at.is_some_and(|last| now.duration_since(last) < interval) {
        return false;
    }
    *last_published_at = Some(now);
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::asr::{AsrEngine, AsrService, Transcription};
    use crate::config::AsrConfig;
    use crate::db::Database;
    use crate::domain_events::DomainEventHub;
    use tokio::sync::broadcast;

    struct FakeEngine;

    #[test]
    fn smart_turn_only_completes_on_a_positive_prediction() {
        assert!(smart_turn_should_complete(&Ok(0.75)));
        assert!(!smart_turn_should_complete(&Ok(COMPLETION_THRESHOLD)));
        assert!(!smart_turn_should_complete(&Err("timed out".into())));
    }

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
    fn audio_level_publication_is_rate_limited() {
        let start = Instant::now();
        let mut last_published_at = None;
        assert!(should_publish_audio_level(&mut last_published_at, start));
        assert!(!should_publish_audio_level(
            &mut last_published_at,
            start + AUDIO_LEVEL_PUBLISH_INTERVAL - Duration::from_millis(1),
        ));
        assert!(should_publish_audio_level(
            &mut last_published_at,
            start + AUDIO_LEVEL_PUBLISH_INTERVAL,
        ));
    }

    #[test]
    fn vrcx_echo_guard_blocks_generated_context_without_blocking_real_mentions() {
        let guard = AsrEchoGuard::new(
            vec![
                "World: Club_GoH2.0".into(),
                "World: Club_GoH2.0; Names: Alice, Bob".into(),
                "Club_GoH2.0\nAlice\nBob".into(),
            ],
            Some("Club_GoH2.0".into()),
        );

        assert!(guard.is_echo("World: Club_GoH2.0."));
        assert!(guard.is_echo("world: club_goh2.0; names: alice, bob"));
        assert!(guard.is_echo("Club_GoH2.0\nAlice\nBob"));
        assert!(guard.is_echo("Club_GoH2.0 Club_GoH2.0"));
        assert!(guard.is_echo("Club_GoH2.0Club_GoH2.0"));
        assert!(guard.is_echo("Club_GoH2.0，Club_GoH2.0"));
        assert!(guard.suppresses_partial("World: Club_"));
        assert!(guard.suppresses_partial("Club_GoH2.0Club_GoH2"));
        assert!(!guard.is_echo("Club_GoH2.0"));
        assert!(!guard.is_echo("We are in Club_GoH2.0"));
        assert!(!guard.is_echo("Club_GoH2.0 is a nice world"));
        assert!(!guard.suppresses_partial("Club_GoH2.0"));
    }

    #[test]
    fn partial_publication_is_rate_limited() {
        let start = Instant::now();
        let mut last_published_at = None;
        assert!(should_publish_partial(&mut last_published_at, start));
        assert!(!should_publish_partial(
            &mut last_published_at,
            start + PARTIAL_PUBLISH_INTERVAL - Duration::from_millis(1),
        ));
        assert!(should_publish_partial(
            &mut last_published_at,
            start + PARTIAL_PUBLISH_INTERVAL,
        ));
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

    #[test]
    fn cloud_reducer_filters_and_throttles_partials() {
        let start = Instant::now();
        let guard = AsrEchoGuard::new(vec!["World: Example".into()], Some("Example".into()));
        let mut state = PipelineState::new(16_000);
        let partial = |utterance_id: &str, text: &str, now| PipelineEvent::Cloud {
            event: CloudEvent::Partial {
                utterance_id: utterance_id.into(),
                text: text.into(),
                language: Some("en".into()),
            },
            partial_publication: PartialPublication::Throttled(now),
            stop_cloud_on_failure: false,
        };

        let effects =
            reduce_pipeline_event(&mut state, partial("echo", "World: Examp", start), &guard);
        assert!(effects.is_empty());
        let effects = reduce_pipeline_event(&mut state, partial("one", "hello", start), &guard);
        assert_eq!(effects.len(), 1);
        let effects = reduce_pipeline_event(
            &mut state,
            partial(
                "two",
                "too soon",
                start + PARTIAL_PUBLISH_INTERVAL - Duration::from_millis(1),
            ),
            &guard,
        );
        assert!(effects.is_empty());
        let effects = reduce_pipeline_event(
            &mut state,
            partial("two", "ready", start + PARTIAL_PUBLISH_INTERVAL),
            &guard,
        );
        assert_eq!(effects.len(), 1);
    }

    #[test]
    fn cloud_reducer_rejects_duplicate_finals_and_resets_failures() {
        let guard = AsrEchoGuard::default();
        let mut state = PipelineState::new(16_000);
        let final_event = || PipelineEvent::Cloud {
            event: CloudEvent::Final {
                utterance_id: "utterance-final".into(),
                text: "done".into(),
                language: None,
            },
            partial_publication: PartialPublication::Immediate,
            stop_cloud_on_failure: false,
        };

        assert_eq!(
            reduce_pipeline_event(&mut state, final_event(), &guard).len(),
            1
        );
        assert!(reduce_pipeline_event(&mut state, final_event(), &guard).is_empty());
        assert!(state.lifecycle.accept_partial("utterance-active"));
        let effects = reduce_pipeline_event(
            &mut state,
            PipelineEvent::Cloud {
                event: CloudEvent::Failed {
                    utterance_id: Some("utterance-active".into()),
                    reset_session: true,
                    code: "asr.reset".into(),
                    detail: "reset".into(),
                },
                partial_publication: PartialPublication::Immediate,
                stop_cloud_on_failure: true,
            },
            &guard,
        );
        assert_eq!(effects.len(), 2);
        assert!(matches!(
            effects.as_slice(),
            [
                PipelineEffect::PublishFailed {
                    utterance_id: None,
                    ..
                },
                PipelineEffect::StopCloud
            ]
        ));
        assert_eq!(state.lifecycle.failure_id(None), None);
    }

    #[test]
    fn audio_reducer_flushes_pre_roll_after_confirmed_trigger() {
        let guard = AsrEchoGuard::default();
        let mut state = PipelineState::new(100);
        let audio = |chunk, trigger_speech| PipelineEvent::AudioAnalyzed {
            chunk,
            rms_dbfs: -20.0,
            peak_dbfs: -10.0,
            vad_speech: true,
            trigger_speech,
            publish_audio_level: false,
            now: Instant::now(),
        };

        assert!(reduce_pipeline_event(&mut state, audio(vec![1.0; 10], true), &guard).is_empty());
        let effects = reduce_pipeline_event(&mut state, audio(vec![2.0; 10], true), &guard);
        assert!(state.streaming);
        assert_eq!(state.pre_roll_samples, 0);
        assert!(matches!(
            effects.as_slice(),
            [PipelineEffect::FlushPreRoll(chunks), PipelineEffect::ProcessAudio { .. }]
                if chunks.len() == 1 && chunks[0].0 == vec![1.0; 10]
        ));
    }

    fn test_dependencies(events: DomainEventHub) -> PipelineDependencies {
        let directory = tempfile::tempdir().unwrap();
        let db = Arc::new(Mutex::new(
            Database::open(&directory.path().join("test.db")).unwrap(),
        ));
        let asr = Arc::new(Mutex::new(AsrService::with_engine(
            AsrConfig::default(),
            Box::new(FakeEngine),
        )));
        let (subtitles, _) = broadcast::channel(4);
        let (live, _) = broadcast::channel(4);
        let (catalog, _) = broadcast::channel(4);
        let (translations, _) = broadcast::channel(4);
        let output = crate::subtitle_output::SubtitleLifecyclePublisher::with_domain_events(
            subtitles,
            translations,
            crate::osc::OscChatboxDispatcher::new(crate::config::OscConfig::default()),
            events,
        );
        let translation = crate::translation::TranslationDispatcher::new(
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::clone(&db),
            catalog.clone(),
            output.clone(),
            crate::vrcx::VrcxIntegration::new(tokio::sync::watch::channel(false).1),
        );
        PipelineDependencies::new(
            asr,
            db,
            live,
            catalog,
            translation,
            Arc::new(std::sync::RwLock::new(crate::config::AppConfig::default())),
            Arc::new(std::sync::RwLock::new(
                crate::language_session::ActiveLanguageSession::Global,
            )),
            output,
        )
    }

    #[tokio::test]
    async fn session_finish_cancels_active_utterances_then_resets_the_source() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        let dependencies = test_dependencies(events);
        let mut lifecycle = RecognitionLifecycle::default();
        assert!(lifecycle.accept_partial("utterance-active"));

        finish_recognition_session(&dependencies, "speaker", &mut lifecycle);

        let cancelled = receiver.recv().await.unwrap();
        let reset = receiver.recv().await.unwrap();
        assert_eq!(cancelled.event_type, "asr.cancelled");
        assert_eq!(cancelled.message_id, "utterance-active");
        assert_eq!(reset.event_type, "asr.reset");
        assert_eq!(reset.source, "speaker");
    }

    #[tokio::test]
    async fn empty_cloud_finals_publish_a_cancellation() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        let dependencies = test_dependencies(events);

        dependencies
            .publish_text("   ".into(), None, "speaker", "utterance-empty".into())
            .await
            .unwrap();

        let event = receiver.recv().await.unwrap();
        assert_eq!(event.event_type, "asr.cancelled");
        assert_eq!(event.message_id, "utterance-empty");
        assert_eq!(event.payload["reason"], "empty");
    }

    #[tokio::test]
    async fn filtered_cloud_finals_publish_a_cancellation() {
        let events = DomainEventHub::new();
        let mut receiver = events.subscribe();
        let dependencies = test_dependencies(events);
        let guard = AsrEchoGuard::new(vec!["World: Example".into()], Some("Example".into()));

        publish_cloud_final(
            &dependencies,
            "speaker",
            &guard,
            "utterance-filtered".into(),
            "World: Example.".into(),
            None,
        )
        .await
        .unwrap();

        let event = receiver.recv().await.unwrap();
        assert_eq!(event.event_type, "asr.cancelled");
        assert_eq!(event.message_id, "utterance-filtered");
        assert_eq!(event.payload["reason"], "filtered");
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
        let (catalog_tx, mut catalog_rx) = broadcast::channel(4);
        let (translation_tx, _) = broadcast::channel(4);
        let output = crate::subtitle_output::SubtitleLifecyclePublisher::new(
            tx,
            translation_tx,
            crate::osc::OscChatboxDispatcher::new(crate::config::OscConfig::default()),
        );
        let translation = crate::translation::TranslationDispatcher::new(
            Arc::new(crate::translation::TranslationService::new().unwrap()),
            Arc::clone(&db),
            catalog_tx.clone(),
            output.clone(),
            crate::vrcx::VrcxIntegration::new(tokio::sync::watch::channel(false).1),
        );
        let dependencies = PipelineDependencies::new(
            asr,
            Arc::clone(&db),
            live_tx,
            catalog_tx,
            translation,
            Arc::new(std::sync::RwLock::new(crate::config::AppConfig::default())),
            Arc::new(std::sync::RwLock::new(
                crate::language_session::ActiveLanguageSession::Global,
            )),
            output,
        );

        dependencies
            .transcribe_and_publish(vec![0.0; 512], "microphone")
            .await
            .unwrap();

        let published = rx.recv().await.unwrap();
        assert_eq!(published.text, "こんにちは");
        assert_eq!(published.source, "microphone");
        let catalog = catalog_rx.recv().await.unwrap();
        assert_eq!(catalog.conversations.len(), 1);
        assert_eq!(catalog.conversations[0].subtitle_count, 1);
        assert_eq!(
            catalog.conversations[0].automatic_title.as_deref(),
            Some("こんにちは")
        );
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
            SmartTurnRuntime::new(PathBuf::new()),
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
