//! Silero ONNX 流式 VAD、能量检测回退与语音分段。

use std::mem::take;
use std::path::Path;

use ndarray::{Array1, Array2, ArrayD};
use ort::session::Session;
use ort::value::Value;

const SAMPLE_RATE: i64 = 16_000;
const FRAME_SAMPLES: usize = 512;
const CONTEXT_SAMPLES: usize = 64;
const ENERGY_REFERENCE_RMS: f32 = 0.04;
const DEFAULT_THRESHOLD: f32 = 0.5;
const DEFAULT_MIN_SPEECH_SECONDS: f64 = 0.25;

#[derive(Debug)]
pub struct VoiceDetector {
    threshold: f32,
    backend: DetectorBackend,
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
        Self::energy(DEFAULT_THRESHOLD)
    }
}

impl VoiceDetector {
    pub fn load(model_path: &Path) -> Self {
        if !model_path.is_file() {
            tracing::warn!(
                path = %model_path.display(),
                "Silero VAD model not found; using energy fallback"
            );
            return Self::default();
        }

        match SileroVad::load(model_path) {
            Ok(model) => Self {
                threshold: DEFAULT_THRESHOLD,
                backend: DetectorBackend::Silero(Box::new(model)),
            },
            Err(error) => {
                tracing::warn!(
                    path = %model_path.display(),
                    %error,
                    "Silero VAD initialization failed; using energy fallback"
                );
                Self::default()
            }
        }
    }

    fn energy(threshold: f32) -> Self {
        Self {
            threshold,
            backend: DetectorBackend::Energy,
        }
    }

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
                    energy_probability(samples)
                }
            },
            DetectorBackend::Energy => energy_probability(samples),
        };
        probability >= self.threshold
    }

    #[allow(dead_code)]
    pub fn reset(&mut self) {
        if let DetectorBackend::Silero(model) = &mut self.backend {
            model.reset();
        }
    }
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
    min_speech_samples: usize,
    max_speech_samples: usize,
    chunks: Vec<Vec<f32>>,
    speech_samples: usize,
    silence_samples_seen: usize,
}

#[allow(dead_code)]
impl SpeechSegmenter {
    pub fn new(sample_rate: u32, silence_seconds: f64, max_speech_seconds: f64) -> Self {
        Self::with_min_speech_seconds(
            sample_rate,
            silence_seconds,
            DEFAULT_MIN_SPEECH_SECONDS,
            max_speech_seconds,
        )
    }

    fn with_min_speech_seconds(
        sample_rate: u32,
        silence_seconds: f64,
        min_speech_seconds: f64,
        max_speech_seconds: f64,
    ) -> Self {
        let samples = |seconds: f64| (seconds * f64::from(sample_rate)) as usize;
        Self {
            silence_samples: samples(silence_seconds),
            min_speech_samples: samples(min_speech_seconds),
            max_speech_samples: samples(max_speech_seconds),
            chunks: Vec::new(),
            speech_samples: 0,
            silence_samples_seen: 0,
        }
    }

    /// 接收一个 PCM 块，并在达到尾部静音或最大时长时返回完整语音段。
    pub fn push(&mut self, chunk: &[f32], speech: bool) -> Option<Vec<f32>> {
        if speech {
            self.chunks.push(chunk.to_vec());
            self.speech_samples += chunk.len();
            self.silence_samples_seen = 0;
        } else if !self.chunks.is_empty() {
            self.chunks.push(chunk.to_vec());
            self.silence_samples_seen += chunk.len();
        }

        let total = self.speech_samples + self.silence_samples_seen;
        let finished =
            self.silence_samples_seen >= self.silence_samples || total >= self.max_speech_samples;
        if !finished {
            return None;
        }

        let valid = self.speech_samples >= self.min_speech_samples;
        let segment = valid.then(|| self.chunks.concat());
        self.reset();
        segment
    }

    pub fn reset(&mut self) {
        self.chunks.clear();
        self.speech_samples = 0;
        self.silence_samples_seen = 0;
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
    fn missing_model_uses_energy_fallback() {
        let detector = VoiceDetector::load(Path::new("missing-silero-vad.onnx"));
        assert_eq!(detector.backend(), "energy");
    }

    #[test]
    #[ignore = "需要通过 VRCS_SILERO_MODEL 指定真实模型"]
    fn silero_model_runs_when_provided() {
        let path = std::env::var("VRCS_SILERO_MODEL").expect("VRCS_SILERO_MODEL");
        let mut detector = VoiceDetector::load(Path::new(&path));
        assert_eq!(detector.backend(), "silero-onnx");
        assert!(!detector.is_speech(&[0.0; FRAME_SAMPLES]));
    }

    #[test]
    fn segmenter_emits_after_silence() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.2, 0.1, 6.0);
        let speech = [1.0; 10];
        let silence = [0.0; 10];

        assert!(segmenter.push(&speech, true).is_none());
        assert!(segmenter.push(&silence, false).is_none());
        assert_eq!(segmenter.push(&silence, false).unwrap().len(), 30);
    }

    #[test]
    fn segmenter_drops_short_noise() {
        let mut segmenter = SpeechSegmenter::with_min_speech_seconds(100, 0.1, 0.25, 6.0);
        assert!(segmenter.push(&[1.0; 10], true).is_none());
        assert!(segmenter.push(&[0.0; 10], false).is_none());
    }

    #[test]
    fn segmenter_caps_long_utterances() {
        let mut segmenter = SpeechSegmenter::new(100, 0.4, 6.0);
        for _ in 0..5 {
            assert!(segmenter.push(&[1.0; 100], true).is_none());
        }
        assert_eq!(segmenter.push(&[1.0; 100], true).unwrap().len(), 600);
    }
}
