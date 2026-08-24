//! Silero ONNX 流式 VAD、能量检测回退与语音分段。

use std::mem::take;
use std::path::Path;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use ndarray::{Array1, Array2, ArrayD};
use ort::session::Session;
use ort::value::Value;
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const SAMPLE_RATE: i64 = 16_000;
const FRAME_SAMPLES: usize = 512;
const CONTEXT_SAMPLES: usize = 64;
const ENERGY_REFERENCE_RMS: f32 = 0.04;
const DEFAULT_THRESHOLD: f32 = 0.5;
const DEFAULT_NEGATIVE_THRESHOLD: f32 = DEFAULT_THRESHOLD - 0.15;
const DEFAULT_MIN_SPEECH_SECONDS: f64 = 0.25;
const MIN_SILENCE_AT_TARGET_SECONDS: f64 = 0.1;
const MAX_SPEECH_BOUNDARY_LOOKBACK_SECONDS: f64 = 1.5;
const MAX_SPEECH_BOUNDARY_LOOKBACK_FRACTION: f64 = 0.4;
const SILENCE_MAX_EXTENSION_SECONDS: f64 = 6.0;
const SMART_TURN_HARD_SILENCE_SECONDS: f64 = 3.0;
const SMART_TURN_MAX_EXTENSION_SECONDS: f64 = 8.0;
const SMART_TURN_ABSOLUTE_MAX_SECONDS: f64 = 30.0;
pub const MODEL_VERSION: &str = "v6.2.1";
const MODEL_REVISION: &str = "7e30209a3e901f9842f81b225f3e93d8199902b1";
const MODEL_URL: &str = "https://raw.githubusercontent.com/snakers4/silero-vad/7e30209a3e901f9842f81b225f3e93d8199902b1/src/silero_vad/data/silero_vad.onnx";
const MODEL_BYTES: u64 = 2_327_524;
const MODEL_SHA256: &str = "1a153a22f4509e292a94e67d6f9b85e8deb25b4988682b7e174c65279d8788e3";

#[derive(Clone, Debug, Default)]
pub struct VadRuntimeState {
    backend: Arc<AtomicU8>,
}

impl VadRuntimeState {
    pub fn backend(&self) -> &'static str {
        if self.backend.load(Ordering::Relaxed) == 1 {
            "silero-onnx"
        } else {
            "energy"
        }
    }

    pub fn model_version(&self) -> Option<&'static str> {
        (self.backend() == "silero-onnx").then_some(MODEL_VERSION)
    }

    fn set_silero(&self) {
        self.backend.store(1, Ordering::Relaxed);
    }

    fn set_energy(&self) {
        self.backend.store(0, Ordering::Relaxed);
    }
}

#[derive(Debug)]
pub struct VoiceDetector {
    threshold: f32,
    speech_active: bool,
    backend: DetectorBackend,
    runtime: VadRuntimeState,
}

#[derive(Debug)]
enum DetectorBackend {
    Silero(Box<SileroVad>),
    Energy,
}

#[derive(Debug)]
struct SileroVad {
    session: Session,
    state: ArrayD<f32>,
    context: Array1<f32>,
}

impl Default for VoiceDetector {
    fn default() -> Self {
        Self::energy(DEFAULT_THRESHOLD, VadRuntimeState::default())
    }
}

impl VoiceDetector {
    #[cfg(test)]
    pub fn load(model_path: &Path) -> Self {
        Self::load_with_runtime(model_path, VadRuntimeState::default())
    }

    pub fn load_with_runtime(model_path: &Path, runtime: VadRuntimeState) -> Self {
        if !model_path.is_file() {
            tracing::warn!(
                path = %model_path.display(),
                "Silero VAD model not found; using energy fallback"
            );
            return Self::energy(DEFAULT_THRESHOLD, runtime);
        }
        match model_file_is_valid_sync(model_path) {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(
                    path = %model_path.display(),
                    version = MODEL_VERSION,
                    "Silero VAD model validation failed; using energy fallback"
                );
                return Self::energy(DEFAULT_THRESHOLD, runtime);
            }
            Err(error) => {
                tracing::warn!(
                    path = %model_path.display(),
                    %error,
                    "Silero VAD model validation failed; using energy fallback"
                );
                return Self::energy(DEFAULT_THRESHOLD, runtime);
            }
        }

        match SileroVad::load(model_path) {
            Ok(model) => {
                runtime.set_silero();
                Self {
                    threshold: DEFAULT_THRESHOLD,
                    speech_active: false,
                    backend: DetectorBackend::Silero(Box::new(model)),
                    runtime,
                }
            }
            Err(error) => {
                tracing::warn!(
                    path = %model_path.display(),
                    %error,
                    "Silero VAD initialization failed; using energy fallback"
                );
                Self::energy(DEFAULT_THRESHOLD, runtime)
            }
        }
    }

    fn energy(threshold: f32, runtime: VadRuntimeState) -> Self {
        runtime.set_energy();
        Self {
            threshold,
            speech_active: false,
            backend: DetectorBackend::Energy,
            runtime,
        }
    }

    #[cfg(test)]
    pub fn backend(&self) -> &'static str {
        match self.backend {
            DetectorBackend::Silero(_) => "silero-onnx",
            DetectorBackend::Energy => "energy",
        }
    }

    #[allow(dead_code)]
    pub fn is_speech(&mut self, samples: &[f32]) -> bool {
        let probability = match &mut self.backend {
            DetectorBackend::Silero(model) => match model.predict(samples) {
                Ok(probability) => probability,
                Err(error) => {
                    tracing::warn!(%error, "Silero VAD inference failed; using energy fallback");
                    self.backend = DetectorBackend::Energy;
                    self.runtime.set_energy();
                    energy_probability(samples)
                }
            },
            DetectorBackend::Energy => energy_probability(samples),
        };
        self.speech_active = if self.speech_active {
            probability >= DEFAULT_NEGATIVE_THRESHOLD
        } else {
            probability >= self.threshold
        };
        self.speech_active
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        self.speech_active = false;
        if let DetectorBackend::Silero(model) = &mut self.backend {
            model.reset();
        }
    }
}

pub async fn ensure_model(model_path: &Path) -> Result<(), String> {
    if model_file_is_valid(model_path).await? {
        return Ok(());
    }
    let parent = model_path.parent().ok_or_else(|| {
        format!(
            "Silero model path has no parent directory: {}",
            model_path.display()
        )
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Failed to create Silero model directory: {error}"))?;
    let partial = model_path.with_extension("onnx.part");
    let _ = tokio::fs::remove_file(&partial).await;

    tracing::info!(
        version = MODEL_VERSION,
        revision = MODEL_REVISION,
        "downloading Silero VAD model"
    );
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .read_timeout(Duration::from_secs(60))
        .build()
        .map_err(|error| error.to_string())?;
    let response = client
        .get(MODEL_URL)
        .send()
        .await
        .map_err(|error| format!("Silero model download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Silero model download failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length != MODEL_BYTES)
    {
        return Err("Silero model download size does not match the pinned version".into());
    }

    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|error| format!("Failed to create Silero temporary file: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Silero model download failed: {error}"))?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "Silero model size overflow".to_string())?;
        if downloaded > MODEL_BYTES {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err("Silero model download exceeds the pinned version size".into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("Failed to write Silero model: {error}"))?;
    }
    file.flush()
        .await
        .map_err(|error| format!("Failed to flush Silero model: {error}"))?;
    file.sync_all()
        .await
        .map_err(|error| format!("Failed to persist Silero model: {error}"))?;
    drop(file);

    let digest = format!("{:x}", hasher.finalize());
    if let Err(error) = validate_model_metadata(downloaded, &digest) {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    if model_path.exists() {
        tokio::fs::remove_file(model_path)
            .await
            .map_err(|error| format!("Failed to replace invalid Silero model: {error}"))?;
    }
    tokio::fs::rename(&partial, model_path)
        .await
        .map_err(|error| format!("Failed to install Silero model: {error}"))?;
    tracing::info!(version = MODEL_VERSION, "Silero VAD model ready");
    Ok(())
}

async fn model_file_is_valid(path: &Path) -> Result<bool, String> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_file() && metadata.len() == MODEL_BYTES => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Failed to inspect Silero model: {error}")),
    };
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("Failed to read Silero model: {error}"))?;
    debug_assert_eq!(metadata.len(), bytes.len() as u64);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok(validate_model_metadata(bytes.len() as u64, &digest).is_ok())
}

fn validate_model_metadata(bytes: u64, sha256: &str) -> Result<(), String> {
    if bytes != MODEL_BYTES {
        return Err(format!(
            "Silero {MODEL_VERSION} model size mismatch: expected {MODEL_BYTES} bytes, found {bytes} bytes"
        ));
    }
    if sha256 != MODEL_SHA256 {
        return Err(format!(
            "Silero {MODEL_VERSION} model SHA-256 verification failed"
        ));
    }
    Ok(())
}

fn model_file_is_valid_sync(path: &Path) -> Result<bool, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("Failed to inspect Silero model: {error}"))?;
    if !metadata.is_file() || metadata.len() != MODEL_BYTES {
        return Ok(false);
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("Failed to read Silero model: {error}"))?;
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok(validate_model_metadata(bytes.len() as u64, &digest).is_ok())
}

impl SileroVad {
    fn load(model_path: &Path) -> Result<Self, String> {
        let session = Session::builder()
            .and_then(|mut builder| builder.commit_from_file(model_path))
            .map_err(|error| error.to_string())?;
        Ok(Self {
            session,
            state: ArrayD::zeros(vec![2, 1, 128]),
            context: Array1::zeros(CONTEXT_SAMPLES),
        })
    }

    fn predict(&mut self, samples: &[f32]) -> Result<f32, String> {
        let mut frame = vec![0.0; FRAME_SAMPLES];
        let copied = samples.len().min(FRAME_SAMPLES);
        frame[..copied].copy_from_slice(&samples[..copied]);

        let mut input = Vec::with_capacity(CONTEXT_SAMPLES + FRAME_SAMPLES);
        input.extend(self.context.iter().copied());
        input.extend(frame.iter().copied());

        let input =
            Array2::from_shape_vec((1, input.len()), input).map_err(|error| error.to_string())?;
        let input = Value::from_array(input).map_err(|error| error.to_string())?;
        let state = Value::from_array(take(&mut self.state)).map_err(|error| error.to_string())?;
        let sample_rate = Value::from_array(Array1::from_vec(vec![SAMPLE_RATE]))
            .map_err(|error| error.to_string())?;

        let outputs = self
            .session
            .run([(&input).into(), (&state).into(), (&sample_rate).into()])
            .map_err(|error| error.to_string())?;
        let (state_shape, state_data) = outputs
            .get("stateN")
            .ok_or_else(|| "Silero VAD did not return stateN".to_string())?
            .try_extract_tensor::<f32>()
            .map_err(|error| error.to_string())?;
        let shape = state_shape
            .iter()
            .map(|dimension| *dimension as usize)
            .collect::<Vec<_>>();
        self.state = ArrayD::from_shape_vec(shape, state_data.to_vec())
            .map_err(|error| error.to_string())?;
        self.context = Array1::from_vec(frame[FRAME_SAMPLES - CONTEXT_SAMPLES..].to_vec());

        outputs
            .get("output")
            .ok_or_else(|| "Silero VAD did not return output".to_string())?
            .try_extract_tensor::<f32>()
            .map_err(|error| error.to_string())?
            .1
            .first()
            .copied()
            .ok_or_else(|| "Silero VAD returned an empty output".into())
    }

    fn reset(&mut self) {
        self.state = ArrayD::zeros(vec![2, 1, 128]);
        self.context.fill(0.0);
    }
}

fn energy_probability(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let mean_square =
        samples.iter().map(|sample| sample * sample).sum::<f32>() / samples.len() as f32;
    (mean_square.sqrt() / ENERGY_REFERENCE_RMS).min(1.0)
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct SpeechSegmenter {
    silence_samples: usize,
    hard_silence_samples: usize,
    min_silence_at_target_samples: usize,
    min_speech_samples: usize,
    preferred_boundary_start_samples: usize,
    absolute_max_samples: usize,
    smart_endpointing: bool,
    chunks: Vec<Vec<f32>>,
    buffered_samples: usize,
    speech_samples: usize,
    silence_samples_seen: usize,
    candidate_ready: bool,
    candidate_pending: bool,
    generation: u64,
}

#[allow(dead_code)]
impl SpeechSegmenter {
    pub fn new(sample_rate: u32, silence_seconds: f64, max_speech_seconds: f64) -> Self {
        Self::with_min_speech_seconds(
            sample_rate,
            silence_seconds,
            DEFAULT_MIN_SPEECH_SECONDS,
            max_speech_seconds,
            false,
        )
    }

    pub fn smart(sample_rate: u32, silence_seconds: f64, max_speech_seconds: f64) -> Self {
        Self::with_min_speech_seconds(
            sample_rate,
            silence_seconds,
            DEFAULT_MIN_SPEECH_SECONDS,
            max_speech_seconds,
            true,
        )
    }

    fn with_min_speech_seconds(
        sample_rate: u32,
        silence_seconds: f64,
        min_speech_seconds: f64,
        max_speech_seconds: f64,
        smart_endpointing: bool,
    ) -> Self {
        let samples = |seconds: f64| (seconds * f64::from(sample_rate)) as usize;
        let max_extension_seconds = if smart_endpointing {
            SMART_TURN_MAX_EXTENSION_SECONDS
        } else {
            max_speech_seconds.min(SILENCE_MAX_EXTENSION_SECONDS)
        };
        let boundary_lookback_seconds = MAX_SPEECH_BOUNDARY_LOOKBACK_SECONDS
            .min(max_speech_seconds * MAX_SPEECH_BOUNDARY_LOOKBACK_FRACTION);
        Self {
            silence_samples: samples(silence_seconds),
            hard_silence_samples: samples(SMART_TURN_HARD_SILENCE_SECONDS),
            min_silence_at_target_samples: samples(MIN_SILENCE_AT_TARGET_SECONDS),
            min_speech_samples: samples(min_speech_seconds),
            preferred_boundary_start_samples: samples(
                max_speech_seconds - boundary_lookback_seconds,
            ),
            absolute_max_samples: samples(
                (max_speech_seconds + max_extension_seconds).min(SMART_TURN_ABSOLUTE_MAX_SECONDS),
            ),
            smart_endpointing,
            chunks: Vec::new(),
            buffered_samples: 0,
            speech_samples: 0,
            silence_samples_seen: 0,
            candidate_ready: false,
            candidate_pending: false,
            generation: 0,
        }
    }

    /// 接收一个 PCM 块，并在达到尾部静音、首选边界或硬上限时返回完整语音段。
    pub fn push(&mut self, chunk: &[f32], speech: bool) -> Option<Vec<f32>> {
        if speech {
            if self.candidate_pending {
                self.generation = self.generation.wrapping_add(1);
                self.candidate_pending = false;
                self.candidate_ready = false;
            }
            self.chunks.push(chunk.to_vec());
            self.buffered_samples += chunk.len();
            self.speech_samples += chunk.len();
            self.silence_samples_seen = 0;
        } else if !self.chunks.is_empty() {
            self.chunks.push(chunk.to_vec());
            self.buffered_samples += chunk.len();
            self.silence_samples_seen += chunk.len();
        }

        let total = self.speech_samples + self.silence_samples_seen;
        let short_silence_near_target = self.speech_samples >= self.min_speech_samples
            && self.silence_samples_seen >= self.min_silence_at_target_samples
            && total >= self.preferred_boundary_start_samples;
        if self.smart_endpointing
            && !self.candidate_pending
            && (self.silence_samples_seen >= self.silence_samples || short_silence_near_target)
        {
            self.candidate_pending = true;
            self.candidate_ready = true;
            let reason = if self.silence_samples_seen >= self.silence_samples {
                "pause"
            } else {
                "target_window"
            };
            tracing::debug!(reason, "Smart Turn endpoint candidate");
        }
        let finished = if self.smart_endpointing {
            let hard_silence = self.silence_samples_seen >= self.hard_silence_samples;
            let absolute_max = self.buffered_samples >= self.absolute_max_samples;
            if hard_silence || absolute_max {
                let reason = if hard_silence {
                    "hard_silence"
                } else {
                    "absolute_max"
                };
                tracing::debug!(reason, "Smart Turn segment ended by safety limit");
            }
            hard_silence || absolute_max
        } else {
            self.silence_samples_seen >= self.silence_samples
                || short_silence_near_target
                || self.buffered_samples >= self.absolute_max_samples
        };
        if !finished {
            return None;
        }
        self.finish()
    }

    pub fn take_candidate(&mut self) -> Option<(u64, Vec<f32>)> {
        if !self.candidate_ready {
            return None;
        }
        self.candidate_ready = false;
        Some((self.generation, self.chunks.concat()))
    }

    pub fn complete_candidate(&mut self, generation: u64) -> Option<Option<Vec<f32>>> {
        (self.candidate_pending && self.generation == generation).then(|| self.finish())
    }

    pub fn is_active(&self) -> bool {
        !self.chunks.is_empty()
    }

    pub fn reset(&mut self) {
        if !self.chunks.is_empty() || self.candidate_pending {
            self.generation = self.generation.wrapping_add(1);
        }
        self.chunks.clear();
        self.buffered_samples = 0;
        self.speech_samples = 0;
        self.silence_samples_seen = 0;
        self.candidate_ready = false;
        self.candidate_pending = false;
    }

    fn finish(&mut self) -> Option<Vec<f32>> {
        let valid = self.speech_samples >= self.min_speech_samples;
        let segment = valid.then(|| self.chunks.concat());
        self.reset();
        segment
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn energy_detector_distinguishes_speech_from_silence() {
        let mut detector = VoiceDetector::default();
        assert_eq!(detector.backend(), "energy");
        assert!(!detector.is_speech(&[0.0; 512]));
        assert!(detector.is_speech(&[0.04; 512]));
    }

    #[test]
    fn detector_uses_hysteresis_after_speech_starts() {
        let mut detector = VoiceDetector::default();

        assert!(detector.is_speech(&[0.04; 512]));
        assert!(detector.is_speech(&[0.016; 512]));
        assert!(!detector.is_speech(&[0.012; 512]));
    }

    #[test]
    fn missing_model_uses_energy_fallback() {
        let detector = VoiceDetector::load(Path::new("missing-silero-vad.onnx"));
        assert_eq!(detector.backend(), "energy");
    }

    #[test]
    #[ignore = "requires a real model specified by VRCS_SILERO_MODEL"]
    fn silero_model_runs_when_provided() {
        let path = std::env::var("VRCS_SILERO_MODEL").expect("VRCS_SILERO_MODEL");
        let mut detector = VoiceDetector::load(Path::new(&path));
        assert_eq!(detector.backend(), "silero-onnx");
        assert!(!detector.is_speech(&[0.0; FRAME_SAMPLES]));
    }

    #[test]
    fn segmenter_emits_after_silence() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.2, 0.1, 6.0, false);
        let speech = [1.0; 10];
        let silence = [0.0; 10];

        assert!(segmenter.push(&speech, true).is_none());
        assert!(segmenter.push(&silence, false).is_none());
        assert_eq!(segmenter.push(&silence, false).unwrap().len(), 30);
    }

    #[test]
    fn segmenter_drops_short_noise() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.1, 0.25, 6.0, false);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.push(&[0.0; 10], false).is_none());
    }

    #[test]
    fn segmenter_treats_the_configured_maximum_as_a_soft_target() {
        let mut segmenter = SpeechSegmenter::new(100, 0.4, 6.0);
        for _ in 0..11 {
            assert!(segmenter.push(&[1.0; 100], true).is_none());
        }
        assert_eq!(segmenter.push(&[1.0; 100], true).unwrap().len(), 1200);
    }

    #[test]
    fn segmenter_ends_at_a_short_pause_after_the_soft_target() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.4, 0.1, 1.0, false);

        assert!(segmenter.push(&[1.0; 100], true).is_none());
        assert_eq!(segmenter.push(&[0.0; 10], false).unwrap().len(), 110);
    }

    #[test]
    fn segmenter_prefers_a_short_silence_near_the_maximum() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.4, 0.1, 1.0, false);

        assert!(segmenter.push(&[1.0; 60], true).is_none());
        assert_eq!(segmenter.push(&[0.0; 10], false).unwrap().len(), 70);
    }

    #[test]
    fn segmenter_ignores_a_short_silence_well_before_the_maximum() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.4, 0.1, 1.0, false);

        assert!(segmenter.push(&[1.0; 40], true).is_none());
        assert!(segmenter.push(&[0.0; 10], false).is_none());
    }

    #[test]
    fn smart_segmenter_waits_for_a_completion_decision() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.2, 0.1, 6.0, true);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.push(&[0.0; 20], false).is_none());

        let (generation, audio) = segmenter.take_candidate().unwrap();
        assert_eq!(audio.len(), 30);
        assert_eq!(
            segmenter
                .complete_candidate(generation)
                .unwrap()
                .unwrap()
                .len(),
            30
        );
        assert!(!segmenter.is_active());
    }

    #[test]
    fn resumed_speech_invalidates_a_smart_turn_result() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.2, 0.1, 6.0, true);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.push(&[0.0; 20], false).is_none());
        let (generation, _) = segmenter.take_candidate().unwrap();
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.complete_candidate(generation).is_none());
        assert!(segmenter.is_active());
    }

    #[test]
    fn smart_segmenter_finishes_after_hard_silence_without_a_result() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.2, 0.1, 6.0, true);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.push(&[0.0; 20], false).is_none());
        assert_eq!(segmenter.push(&[0.0; 280], false).unwrap().len(), 310);
        assert!(!segmenter.is_active());
    }

    #[test]
    fn smart_segmenter_treats_the_configured_maximum_as_soft() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.4, 0.1, 1.0, true);
        assert!(segmenter.push(&[1.0; 100], true).is_none());
        assert!(segmenter.push(&[0.0; 10], false).is_none());

        let (generation, audio) = segmenter.take_candidate().unwrap();
        assert_eq!(audio.len(), 110);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.complete_candidate(generation).is_none());
        assert!(segmenter.is_active());
    }

    #[test]
    fn smart_segmenter_enforces_an_absolute_maximum() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.4, 0.1, 1.0, true);
        assert_eq!(segmenter.push(&[1.0; 900], true).unwrap().len(), 900);
        assert!(!segmenter.is_active());
    }

    #[test]
    fn validates_pinned_model_metadata() {
        assert!(validate_model_metadata(MODEL_BYTES, MODEL_SHA256).is_ok());
        assert!(validate_model_metadata(MODEL_BYTES - 1, MODEL_SHA256).is_err());
        assert!(validate_model_metadata(MODEL_BYTES, "invalid").is_err());
    }

    #[test]
    fn runtime_state_tracks_the_active_backend() {
        let runtime = VadRuntimeState::default();
        assert_eq!(runtime.backend(), "energy");
        assert_eq!(runtime.model_version(), None);
        runtime.set_silero();
        assert_eq!(runtime.backend(), "silero-onnx");
        assert_eq!(runtime.model_version(), Some(MODEL_VERSION));
        runtime.set_energy();
        assert_eq!(runtime.backend(), "energy");
    }
}
