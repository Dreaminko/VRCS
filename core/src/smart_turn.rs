//! Smart Turn v3.2 model preparation and native ONNX inference.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use ndarray::Array3;
use ort::session::Session;
use ort::value::Value;
use rustfft::num_complex::Complex64;
use rustfft::{Fft, FftPlanner};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

const SAMPLE_RATE: usize = 16_000;
const AUDIO_SECONDS: usize = 8;
const AUDIO_SAMPLES: usize = SAMPLE_RATE * AUDIO_SECONDS;
const N_FFT: usize = 400;
const HOP_LENGTH: usize = 160;
const N_MELS: usize = 80;
const N_FRAMES: usize = 800;
const MEL_FLOOR: f32 = 1e-10;
const NORM_VARIANCE_EPS: f32 = 1e-7;

pub const MODEL_VERSION: &str = "v3.2-cpu";
pub const MODEL_FILENAME: &str = "smart-turn-v3.2-cpu.onnx";
pub const COMPLETION_THRESHOLD: f32 = 0.5;
const MODEL_REVISION: &str = "f766f81d3cfdf7737ac64aad813d91bbfd56bf93";
const MODEL_URL: &str = "https://huggingface.co/pipecat-ai/smart-turn-v3/resolve/f766f81d3cfdf7737ac64aad813d91bbfd56bf93/smart-turn-v3.2-cpu.onnx";
const MODEL_BYTES: u64 = 8_679_182;
const MODEL_SHA256: &str = "2bb026316b14a660486a75b1733cd3fbab8c2fd0314dc9af7be49f8cca967e4f";

#[derive(Clone)]
pub struct SmartTurnRuntime {
    model_path: Arc<PathBuf>,
    analyzer: Arc<Mutex<Option<SmartTurnAnalyzer>>>,
    prepare_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SmartTurnRuntime {
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path: Arc::new(model_path),
            analyzer: Arc::new(Mutex::new(None)),
            prepare_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub async fn prepare(&self) -> Result<(), String> {
        let _guard = self.prepare_lock.lock().await;
        ensure_model(&self.model_path).await?;
        let analyzer = Arc::clone(&self.analyzer);
        let model_path = Arc::clone(&self.model_path);
        tokio::task::spawn_blocking(move || {
            let mut analyzer = analyzer
                .lock()
                .map_err(|_| "Smart Turn runtime lock is poisoned".to_string())?;
            if analyzer.is_none() {
                *analyzer = Some(SmartTurnAnalyzer::load(&model_path)?);
            }
            Ok(())
        })
        .await
        .map_err(|error| format!("Smart Turn initialization task failed: {error}"))?
    }

    pub async fn predict(&self, audio: Vec<f32>) -> Result<f32, String> {
        let analyzer = Arc::clone(&self.analyzer);
        let model_path = Arc::clone(&self.model_path);
        tokio::task::spawn_blocking(move || {
            let mut analyzer = analyzer
                .lock()
                .map_err(|_| "Smart Turn runtime lock is poisoned".to_string())?;
            if analyzer.is_none() {
                *analyzer = Some(SmartTurnAnalyzer::load(&model_path)?);
            }
            analyzer
                .as_mut()
                .expect("Smart Turn analyzer initialized")
                .predict(&audio)
        })
        .await
        .map_err(|error| format!("Smart Turn inference task failed: {error}"))?
    }
}

struct SmartTurnAnalyzer {
    session: Session,
    features: WhisperFeatures,
}

impl SmartTurnAnalyzer {
    fn load(model_path: &Path) -> Result<Self, String> {
        if !model_file_is_valid_sync(model_path)? {
            return Err(format!(
                "Smart Turn {MODEL_VERSION} model is missing or invalid"
            ));
        }
        let session = Session::builder()
            .and_then(|mut builder| builder.commit_from_file(model_path))
            .map_err(|error| format!("Failed to initialize Smart Turn: {error}"))?;
        Ok(Self {
            session,
            features: WhisperFeatures::new(),
        })
    }

    fn predict(&mut self, audio: &[f32]) -> Result<f32, String> {
        let features = self.features.compute(audio)?;
        let input = Array3::from_shape_vec((1, N_MELS, N_FRAMES), features)
            .map_err(|error| error.to_string())?;
        let input = Value::from_array(input).map_err(|error| error.to_string())?;
        let outputs = self
            .session
            .run(ort::inputs!["input_features" => input])
            .map_err(|error| format!("Smart Turn inference failed: {error}"))?;
        outputs[0]
            .try_extract_tensor::<f32>()
            .map_err(|error| error.to_string())?
            .1
            .first()
            .copied()
            .ok_or_else(|| "Smart Turn returned an empty output".into())
    }
}

struct WhisperFeatures {
    fft: Arc<dyn Fft<f64>>,
    hann: Vec<f64>,
    mel_filters: Vec<f64>,
}

impl WhisperFeatures {
    fn new() -> Self {
        let mut planner = FftPlanner::<f64>::new();
        Self {
            fft: planner.plan_fft_forward(N_FFT),
            hann: (0..N_FFT)
                .map(|index| {
                    0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / N_FFT as f64).cos()
                })
                .collect(),
            mel_filters: build_mel_filters(),
        }
    }

    fn compute(&self, audio: &[f32]) -> Result<Vec<f32>, String> {
        let mut waveform = fixed_audio_window(audio);
        normalize(&mut waveform);
        let padded = reflect_pad(&waveform, N_FFT / 2);
        let mut mel = vec![0.0_f64; N_MELS * (N_FRAMES + 1)];
        let mut fft_buffer = vec![Complex64::default(); N_FFT];

        for frame in 0..=N_FRAMES {
            let start = frame * HOP_LENGTH;
            for index in 0..N_FFT {
                fft_buffer[index] =
                    Complex64::new(f64::from(padded[start + index]) * self.hann[index], 0.0);
            }
            self.fft.process(&mut fft_buffer);
            for mel_index in 0..N_MELS {
                let filter_offset = mel_index * (N_FFT / 2 + 1);
                let value = fft_buffer[..=N_FFT / 2]
                    .iter()
                    .enumerate()
                    .map(|(bin, value)| value.norm_sqr() * self.mel_filters[filter_offset + bin])
                    .sum::<f64>()
                    .max(f64::from(MEL_FLOOR))
                    .log10();
                mel[mel_index * (N_FRAMES + 1) + frame] = value;
            }
        }

        let maximum = mel.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        if !maximum.is_finite() {
            return Err("Smart Turn feature extraction produced invalid values".into());
        }
        let floor = maximum - 8.0;
        let mut output = Vec::with_capacity(N_MELS * N_FRAMES);
        for mel_index in 0..N_MELS {
            let offset = mel_index * (N_FRAMES + 1);
            output.extend(
                mel[offset..offset + N_FRAMES]
                    .iter()
                    .map(|value| ((value.max(floor) + 4.0) / 4.0) as f32),
            );
        }
        Ok(output)
    }
}

fn fixed_audio_window(audio: &[f32]) -> Vec<f32> {
    if audio.len() >= AUDIO_SAMPLES {
        return audio[audio.len() - AUDIO_SAMPLES..].to_vec();
    }
    let mut output = vec![0.0; AUDIO_SAMPLES - audio.len()];
    output.extend_from_slice(audio);
    output
}

fn normalize(audio: &mut [f32]) {
    let mean = audio.iter().sum::<f32>() / audio.len() as f32;
    let variance = audio
        .iter()
        .map(|sample| {
            let centered = sample - mean;
            centered * centered
        })
        .sum::<f32>()
        / audio.len() as f32;
    let scale = (variance + NORM_VARIANCE_EPS).sqrt();
    for sample in audio {
        *sample = (*sample - mean) / scale;
    }
}

fn reflect_pad(audio: &[f32], padding: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(audio.len() + padding * 2);
    output.extend((1..=padding).rev().map(|index| audio[index]));
    output.extend_from_slice(audio);
    output.extend((1..=padding).map(|index| audio[audio.len() - 1 - index]));
    output
}

fn hertz_to_mel(frequency: f64) -> f64 {
    if frequency < 1_000.0 {
        3.0 * frequency / 200.0
    } else {
        15.0 + (frequency / 1_000.0).ln() * (27.0 / 6.4_f64.ln())
    }
}

fn mel_to_hertz(mel: f64) -> f64 {
    if mel < 15.0 {
        200.0 * mel / 3.0
    } else {
        1_000.0 * ((6.4_f64.ln() / 27.0) * (mel - 15.0)).exp()
    }
}

fn build_mel_filters() -> Vec<f64> {
    let mel_min = hertz_to_mel(0.0);
    let mel_max = hertz_to_mel(SAMPLE_RATE as f64 / 2.0);
    let mel_points = (0..N_MELS + 2)
        .map(|index| {
            mel_to_hertz(mel_min + (mel_max - mel_min) * index as f64 / (N_MELS + 1) as f64)
        })
        .collect::<Vec<_>>();
    let bins = N_FFT / 2 + 1;
    let mut filters = vec![0.0_f64; N_MELS * bins];
    for mel_index in 0..N_MELS {
        let left = mel_points[mel_index];
        let center = mel_points[mel_index + 1];
        let right = mel_points[mel_index + 2];
        let normalization = 2.0 / (right - left);
        for bin in 0..bins {
            let frequency = bin as f64 * (SAMPLE_RATE as f64 / 2.0) / (bins - 1) as f64;
            let down = (frequency - left) / (center - left);
            let up = (right - frequency) / (right - center);
            filters[mel_index * bins + bin] = down.min(up).max(0.0) * normalization;
        }
    }
    filters
}

pub async fn ensure_model(model_path: &Path) -> Result<(), String> {
    if model_file_is_valid(model_path).await? {
        return Ok(());
    }
    let parent = model_path.parent().ok_or_else(|| {
        format!(
            "Smart Turn model path has no parent directory: {}",
            model_path.display()
        )
    })?;
    tokio::fs::create_dir_all(parent)
        .await
        .map_err(|error| format!("Failed to create Smart Turn model directory: {error}"))?;
    let partial = model_path.with_extension("onnx.part");
    let _ = tokio::fs::remove_file(&partial).await;

    tracing::info!(
        version = MODEL_VERSION,
        revision = MODEL_REVISION,
        "downloading Smart Turn model"
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
        .map_err(|error| format!("Smart Turn model download failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Smart Turn model download failed: {error}"))?;
    if response
        .content_length()
        .is_some_and(|length| length != MODEL_BYTES)
    {
        return Err("Smart Turn model download size does not match the pinned version".into());
    }

    let mut file = tokio::fs::File::create(&partial)
        .await
        .map_err(|error| format!("Failed to create Smart Turn temporary file: {error}"))?;
    let mut stream = response.bytes_stream();
    let mut downloaded = 0u64;
    let mut hasher = Sha256::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|error| format!("Smart Turn model download failed: {error}"))?;
        downloaded = downloaded
            .checked_add(chunk.len() as u64)
            .ok_or_else(|| "Smart Turn model size overflow".to_string())?;
        if downloaded > MODEL_BYTES {
            let _ = tokio::fs::remove_file(&partial).await;
            return Err("Smart Turn model download exceeds the pinned version size".into());
        }
        hasher.update(&chunk);
        file.write_all(&chunk)
            .await
            .map_err(|error| format!("Failed to write Smart Turn model: {error}"))?;
    }
    file.flush()
        .await
        .map_err(|error| format!("Failed to flush Smart Turn model: {error}"))?;
    file.sync_all()
        .await
        .map_err(|error| format!("Failed to persist Smart Turn model: {error}"))?;
    drop(file);

    let digest = format!("{:x}", hasher.finalize());
    if let Err(error) = validate_model_metadata(downloaded, &digest) {
        let _ = tokio::fs::remove_file(&partial).await;
        return Err(error);
    }
    if model_path.exists() {
        tokio::fs::remove_file(model_path)
            .await
            .map_err(|error| format!("Failed to replace invalid Smart Turn model: {error}"))?;
    }
    tokio::fs::rename(&partial, model_path)
        .await
        .map_err(|error| format!("Failed to install Smart Turn model: {error}"))?;
    Ok(())
}

async fn model_file_is_valid(path: &Path) -> Result<bool, String> {
    let metadata = match tokio::fs::metadata(path).await {
        Ok(metadata) if metadata.is_file() && metadata.len() == MODEL_BYTES => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Failed to inspect Smart Turn model: {error}")),
    };
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| format!("Failed to read Smart Turn model: {error}"))?;
    debug_assert_eq!(metadata.len(), bytes.len() as u64);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok(validate_model_metadata(bytes.len() as u64, &digest).is_ok())
}

fn model_file_is_valid_sync(path: &Path) -> Result<bool, String> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() && metadata.len() == MODEL_BYTES => metadata,
        Ok(_) => return Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(format!("Failed to inspect Smart Turn model: {error}")),
    };
    let bytes =
        std::fs::read(path).map_err(|error| format!("Failed to read Smart Turn model: {error}"))?;
    debug_assert_eq!(metadata.len(), bytes.len() as u64);
    let digest = format!("{:x}", Sha256::digest(&bytes));
    Ok(validate_model_metadata(bytes.len() as u64, &digest).is_ok())
}

fn validate_model_metadata(bytes: u64, sha256: &str) -> Result<(), String> {
    if bytes != MODEL_BYTES {
        return Err(format!(
            "Smart Turn {MODEL_VERSION} model size mismatch: expected {MODEL_BYTES} bytes, found {bytes} bytes"
        ));
    }
    if sha256 != MODEL_SHA256 {
        return Err(format!(
            "Smart Turn {MODEL_VERSION} model SHA-256 verification failed"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_is_left_padded_and_keeps_the_latest_eight_seconds() {
        let padded = fixed_audio_window(&[1.0, 2.0]);
        assert_eq!(padded.len(), AUDIO_SAMPLES);
        assert_eq!(&padded[AUDIO_SAMPLES - 2..], &[1.0, 2.0]);

        let long = (0..AUDIO_SAMPLES + 2)
            .map(|value| value as f32)
            .collect::<Vec<_>>();
        let truncated = fixed_audio_window(&long);
        assert_eq!(truncated[0], 2.0);
        assert_eq!(truncated.len(), AUDIO_SAMPLES);
    }

    #[test]
    fn feature_extractor_returns_the_model_shape() {
        let features = WhisperFeatures::new()
            .compute(&vec![0.0; SAMPLE_RATE])
            .unwrap();
        assert_eq!(features.len(), N_MELS * N_FRAMES);
        assert!(features.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn validates_pinned_model_metadata() {
        assert!(validate_model_metadata(MODEL_BYTES, MODEL_SHA256).is_ok());
        assert!(validate_model_metadata(MODEL_BYTES - 1, MODEL_SHA256).is_err());
        assert!(validate_model_metadata(MODEL_BYTES, "bad").is_err());
    }

    #[tokio::test]
    #[ignore = "requires a real model specified by VRCS_SMART_TURN_MODEL"]
    async fn model_runs_when_provided() {
        let path = std::env::var("VRCS_SMART_TURN_MODEL").expect("VRCS_SMART_TURN_MODEL");
        let runtime = SmartTurnRuntime::new(PathBuf::from(path));
        runtime.prepare().await.unwrap();
        let probability = runtime.predict(vec![0.0; SAMPLE_RATE]).await.unwrap();
        assert!((0.0..=1.0).contains(&probability));
    }
}
