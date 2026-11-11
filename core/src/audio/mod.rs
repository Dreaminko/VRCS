//! 音频采集公共接口与生命周期管理。
//! Windows 的设备枚举、WASAPI 采集和 PCM 转换位于 `audio/wasapi/`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::models::AudioDevice;

#[cfg(not(any(windows, target_os = "linux")))]
mod platform;
#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
use wasapi as platform;
#[cfg(target_os = "linux")]
mod pipewire;
#[cfg(target_os = "linux")]
use pipewire as platform;

pub const CHUNK_FRAMES: usize = 512;
const CHANNEL_CAPACITY: usize = 128;
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
const START_ATTEMPTS: usize = 3;
const START_RETRY_DELAYS: [std::time::Duration; START_ATTEMPTS - 1] = [
    std::time::Duration::from_millis(150),
    std::time::Duration::from_millis(400),
];
const RESOURCE_EXHAUSTED_START_ATTEMPTS: usize = 6;
const RESOURCE_EXHAUSTED_RETRY_DELAYS: [std::time::Duration;
    RESOURCE_EXHAUSTED_START_ATTEMPTS - 1] = [
    std::time::Duration::from_millis(150),
    std::time::Duration::from_millis(400),
    std::time::Duration::from_secs(2),
    std::time::Duration::from_secs(5),
    std::time::Duration::from_secs(10),
];

#[derive(Debug, Clone)]
pub struct AudioError {
    code: &'static str,
    message: String,
    retryable: bool,
}

impl AudioError {
    fn new(message: impl Into<String>) -> Self {
        Self::with_code("audio.unavailable", message)
    }

    pub(crate) fn with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: false,
        }
    }

    #[cfg(any(windows, target_os = "linux", test))]
    pub(crate) fn retryable_with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            retryable: true,
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
    }

    #[cfg(any(windows, test))]
    pub(crate) fn with_default_code(mut self, code: &'static str) -> Self {
        if self.code == "audio.unavailable" {
            self.code = code;
        }
        self
    }

    fn is_retryable(&self) -> bool {
        self.retryable
    }

    fn at_stage(mut self, stage: &'static str) -> Self {
        self.message = format!("{stage}: {}", self.message);
        self
    }
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AudioError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureSource {
    Speaker,
    Microphone,
}

struct CaptureSession {
    stop: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
    rx: mpsc::Receiver<Vec<f32>>,
    status: Option<std::sync::mpsc::Receiver<Result<AudioDevice, AudioError>>>,
    priming: std::collections::VecDeque<Vec<f32>>,
}

pub struct AudioCapture {
    output_rate: u32,
    source: CaptureSource,
    session: Option<CaptureSession>,
    device: Option<AudioDevice>,
    microphone_tap: Option<crate::vision::MicrophoneTap>,
}

impl AudioCapture {
    #[cfg(any(windows, target_os = "linux", test))]
    pub(crate) fn from_stream(
        rx: mpsc::Receiver<Vec<f32>>,
        priming: std::collections::VecDeque<Vec<f32>>,
    ) -> Self {
        Self {
            output_rate: 16_000,
            source: CaptureSource::Speaker,
            session: Some(CaptureSession {
                stop: Arc::new(AtomicBool::new(false)),
                join: None,
                rx,
                priming,
                status: None,
            }),
            device: None,
            microphone_tap: None,
        }
    }
    pub fn new(output_rate: u32, source: CaptureSource) -> Self {
        Self {
            output_rate,
            source,
            session: None,
            device: None,
            microphone_tap: None,
        }
    }

    pub(crate) fn with_microphone_tap(mut self, tap: crate::vision::MicrophoneTap) -> Self {
        if self.source == CaptureSource::Microphone {
            self.microphone_tap = Some(tap);
        }
        self
    }

    #[allow(dead_code)]
    pub fn device(&self) -> Option<&AudioDevice> {
        self.device.as_ref()
    }

    pub fn start(
        &mut self,
        device_id: Option<i64>,
        device_key: Option<&str>,
        process_name: Option<&str>,
    ) -> Result<AudioDevice, AudioError> {
        if self.session.is_some() {
            return Err(AudioError::with_code(
                "capture.already_running",
                "Audio capture is already running",
            ));
        }
        if let Some(name) = process_name {
            if self.source != CaptureSource::Speaker {
                return Err(AudioError::new(
                    "Process loopback is only valid for speaker capture",
                ));
            }
            let process_id = platform::find_process_id(name)?.ok_or_else(|| {
                AudioError::with_code("audio.vrchat_not_running", "VRChat is not running")
            })?;
            return self.start_session(platform::CaptureTarget::Process(process_id));
        }
        let direction = match self.source {
            CaptureSource::Speaker => platform::DeviceDirection::Render,
            CaptureSource::Microphone => platform::DeviceDirection::Capture,
        };
        let backend_id = match device_key {
            Some(key) => {
                validate_device_key(Some(key), self.source)?;
                Some(key.to_owned())
            }
            None => device_id.map(|id| platform::resolve_device_id(id, self.source)).transpose()?,
        };
        let target = platform::CaptureTarget::Device { backend_id, direction };
        self.start_session(target)
    }

    fn start_session(
        &mut self,
        target: platform::CaptureTarget,
    ) -> Result<AudioDevice, AudioError> {
        let source = self.source;
        start_with_retry(source, || self.start_session_once(target.clone())).map_err(|error| {
            let stage = match source {
                CaptureSource::Speaker => "speaker_capture",
                CaptureSource::Microphone => "microphone_capture",
            };
            error.at_stage(stage)
        })
    }

    fn start_session_once(
        &mut self,
        target: platform::CaptureTarget,
    ) -> Result<AudioDevice, AudioError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<AudioDevice, AudioError>>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let output_rate = self.output_rate;
        let microphone_tap = self.microphone_tap.clone();
        let span = tracing::Span::current();
        let join = std::thread::Builder::new()
            .name("vrcs-audio-capture".into())
            .spawn(move || {
                let _entered = span.enter();
                platform::capture_main(
                    target,
                    output_rate,
                    thread_stop,
                    tx,
                    ready_tx,
                    microphone_tap,
                );
            })
            .map_err(|error| {
                AudioError::with_code(
                    "audio.start_thread_failed",
                    format!("Failed to start audio capture thread: {error}"),
                )
            })?;

        let device = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(device)) => device,
            Ok(Err(error)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(error);
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError::with_code(
                    "audio.start_timeout",
                    "Timed out while starting audio capture",
                ));
            }
        };
        self.session = Some(CaptureSession {
            stop,
            join: Some(join),
            rx,
            status: Some(ready_rx),
            priming: Default::default(),
        });
        self.device = Some(device.clone());
        Ok(device)
    }

    pub async fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AudioError::new("Audio capture is not running"))?;
        if let Some(frame) = session.priming.pop_front() {
            return Ok(frame);
        }
        match session.rx.recv().await {
            Some(frame) => Ok(frame),
            None => Err(session.status.as_ref()
                .and_then(|status| status.try_iter().find_map(Result::err))
                .unwrap_or_else(|| AudioError::new("Audio capture has stopped"))),
        }
    }

    #[allow(dead_code)]
    pub fn interrupt(&mut self) {
        if let Some(session) = &self.session {
            session.stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn stop(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.stop.store(true, Ordering::Relaxed);
            if let Some(join) = session.join.take() {
                let _ = join.join();
            }
        }
        self.device = None;
    }

    pub async fn shutdown(&mut self) {
        let Some(mut session) = self.session.take() else {
            self.device = None;
            return;
        };
        session.stop.store(true, Ordering::Relaxed);
        if let Some(join) = session.join.take() {
            let _ = tokio::task::spawn_blocking(move || join.join()).await;
        }
        self.device = None;
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

fn start_with_retry<T>(
    source: CaptureSource,
    mut operation: impl FnMut() -> Result<T, AudioError>,
) -> Result<T, AudioError> {
    let started = std::time::Instant::now();
    let mut retry_delays = &START_RETRY_DELAYS[..];
    for attempt in 1..=RESOURCE_EXHAUSTED_START_ATTEMPTS {
        let span = tracing::info_span!("audio_capture_start", ?source, attempt);
        let _entered = span.enter();
        match operation() {
            Ok(value) => {
                if attempt > 1 {
                    tracing::info!(
                        elapsed_ms = started.elapsed().as_millis() as u64,
                        "audio capture recovered after retry"
                    );
                }
                return Ok(value);
            }
            Err(error) => {
                if error.code() == "audio.resource_exhausted" {
                    retry_delays = &RESOURCE_EXHAUSTED_RETRY_DELAYS;
                }
                let delay = retry_delays
                    .get(attempt - 1)
                    .filter(|_| error.is_retryable());
                tracing::warn!(
                    code = error.code(),
                    detail = %error,
                    retrying = delay.is_some(),
                    retry_delay_ms = delay.map(|delay| delay.as_millis() as u64),
                    "failed to start audio capture"
                );
                match delay {
                    Some(delay) => std::thread::sleep(*delay),
                    None => return Err(error),
                }
            }
        }
    }
    unreachable!("audio startup retry limits are enforced inside the loop")
}

pub fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    platform::list_devices()
}

/// Explicit device identities must resolve within the selected backend and direction.
pub(crate) fn validate_device_key(key: Option<&str>, source: CaptureSource) -> Result<(), AudioError> {
    #[cfg(target_os = "linux")]
    if let Some(key) = key {
        let loopback = source == CaptureSource::Speaker;
        return list_devices()?.into_iter()
            .find(|device| device.is_loopback == loopback && device.device_key.as_deref() == Some(key))
            .map(|_| ())
            .ok_or_else(|| AudioError::with_code("audio.device_unavailable", "The selected PipeWire device is no longer available"));
    }
    let _ = source;
    if key.is_some() {
        Err(AudioError::with_code(
            crate::platform_capabilities::DEVICE_KEY_UNSUPPORTED,
            "Reselect an audio device on this platform before starting capture",
        ))
    } else {
        Ok(())
    }
}

pub fn validate_device_id(
    device_id: i64,
    source: CaptureSource,
) -> Result<AudioDevice, AudioError> {
    #[cfg(target_os = "linux")]
    {
        let _ = (device_id, source);
        Err(AudioError::with_code(crate::platform_capabilities::DEVICE_KEY_UNSUPPORTED, "Linux audio devices require a PipeWire device key; select the device again"))
    }
    #[cfg(not(target_os = "linux"))]
    {
    let expected_loopback = source == CaptureSource::Speaker;
    platform::list_devices()?
        .into_iter()
        .find(|item| item.id == device_id && item.is_loopback == expected_loopback)
        .ok_or_else(|| {
            let label = if expected_loopback {
                "system output"
            } else {
                "microphone"
            };
            AudioError::with_code(
                "audio.device_unavailable",
                format!("The selected {label} device is no longer available"),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_a_transient_default_device_start_failure() {
        let mut attempts = 0;
        let result = start_with_retry(CaptureSource::Speaker, || {
            attempts += 1;
            if attempts == 1 {
                Err(AudioError::retryable_with_code(
                    "audio.device_unavailable",
                    "device is switching",
                ))
            } else {
                Ok(())
            }
        });

        assert!(result.is_ok());
        assert_eq!(attempts, 2);
    }

    #[test]
    fn fallback_codes_preserve_more_specific_audio_errors() {
        let generic = AudioError::new("activation failed")
            .with_default_code("audio.process_loopback_unavailable");
        let specific = AudioError::with_code("audio.permission_denied", "access denied")
            .with_default_code("audio.process_loopback_unavailable");

        assert_eq!(generic.code(), "audio.process_loopback_unavailable");
        assert_eq!(specific.code(), "audio.permission_denied");
    }

    #[test]
    fn does_not_retry_permanent_start_failures() {
        let mut attempts = 0;
        let result = start_with_retry(CaptureSource::Speaker, || {
            attempts += 1;
            Err::<(), _>(AudioError::with_code(
                "audio.device_unavailable",
                "selected device is unavailable",
            ))
        });

        assert!(result.is_err());
        assert_eq!(attempts, 1);
    }

    #[test]
    fn retries_default_endpoint_resource_exhaustion_with_backoff() {
        let mut attempts = 0;
        let result = start_with_retry(CaptureSource::Speaker, || {
            attempts += 1;
            if attempts < START_ATTEMPTS {
                Err(AudioError::retryable_with_code(
                    "audio.resource_exhausted",
                    "audio engine is out of resources",
                ))
            } else {
                Ok(())
            }
        });

        assert!(result.is_ok());
        assert_eq!(attempts, START_ATTEMPTS);
    }

    #[test]
    fn resource_exhaustion_recovery_handles_changing_transient_errors() {
        let mut attempts = 0;
        let result = start_with_retry(CaptureSource::Microphone, || {
            attempts += 1;
            match attempts {
                1 => Err(AudioError::retryable_with_code(
                    "audio.resource_exhausted",
                    "audio engine is out of resources",
                )),
                2 | 3 => Err(AudioError::retryable_with_code(
                    "audio.device_unavailable",
                    "device is switching",
                )),
                _ => Ok(()),
            }
        });

        assert!(result.is_ok());
        assert_eq!(attempts, 4);
    }

    #[cfg(windows)]
    #[test]
    fn lists_loopback_and_microphone_devices() {
        let devices = list_devices().unwrap();
        for device in &devices {
            assert!(!device.name.is_empty());
            assert!(device.sample_rate > 0);
        }
        let ids: std::collections::HashSet<i64> = devices.iter().map(|device| device.id).collect();
        assert_eq!(ids.len(), devices.len(), "device IDs must be unique");
    }

    #[cfg(windows)]
    #[tokio::test]
    #[ignore]
    async fn captures_default_loopback_chunks() {
        let mut capture = AudioCapture::new(16_000, CaptureSource::Speaker);
        let device = capture.start(None, None, None).unwrap();
        assert!(device.is_loopback);
        for _ in 0..3 {
            let chunk = capture.read().await.unwrap();
            assert_eq!(chunk.len(), CHUNK_FRAMES);
        }
        capture.stop();
        assert!(capture.device().is_none());
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    #[ignore = "requires tests/pipewire-smoke.conf in a disposable runtime with WirePlumber; never the user's audio session"]
    async fn isolated_pipewire_captures_both_sources_and_stops_without_audio() {
        assert_eq!(std::env::var("PIPEWIRE_REMOTE").as_deref(), Ok("vrcs-test-pipewire"));
        let source = "pipewire:Audio/Source:vrcs-test-source";
        let sink = "pipewire:Audio/Sink:vrcs-test-sink";
        for graph_rate in [48_000, 44_100] {
            let status = std::process::Command::new("pw-metadata")
                .args(["-n", "settings", "0", "clock.force-rate", &graph_rate.to_string()]).status().unwrap();
            assert!(status.success());
            let mut microphone = AudioCapture::new(16_000, CaptureSource::Microphone);
            let mut output = AudioCapture::new(16_000, CaptureSource::Speaker);
            microphone.start(None, Some(source), None).unwrap();
            output.start(None, Some(sink), None).unwrap();
            let started = std::time::Instant::now();
            let mut microphone_energy = 0.0f64;
            let mut output_energy = 0.0f64;
            for _ in 0..24 {
                let (microphone, output) = tokio::join!(
                    tokio::time::timeout(std::time::Duration::from_secs(3), microphone.read()),
                    tokio::time::timeout(std::time::Duration::from_secs(3), output.read()),
                );
                let microphone = microphone.expect("microphone PCM deadline");
                let output = output.expect("sink monitor PCM deadline");
                let microphone = microphone.unwrap();
                let output = output.unwrap();
                assert_eq!((microphone.len(), output.len()), (CHUNK_FRAMES, CHUNK_FRAMES));
                assert!(microphone.iter().chain(&output).all(|sample| sample.is_finite()));
                microphone_energy += microphone.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>();
                output_energy += output.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>();
            }
            assert!(microphone_energy > 1.0 && output_energy > 1.0, "virtual source/monitor tone missing");
            let stop = std::time::Instant::now();
            microphone.shutdown().await;
            output.shutdown().await;
            assert!(stop.elapsed() < std::time::Duration::from_secs(1));
            eprintln!("PipeWire graph={graph_rate} mono16k samples={} capture_ms={} stop_ms={}", 24 * CHUNK_FRAMES, started.elapsed().as_millis(), stop.elapsed().as_millis());
        }
        let mut missing = AudioCapture::new(16_000, CaptureSource::Microphone);
        assert_eq!(missing.start(None, Some("pipewire:Audio/Source:missing-source"), None).unwrap_err().code(), "audio.device_unavailable");
        assert_eq!(missing.start(Some(1), None, None).unwrap_err().code(), "audio.device_reselection_required");
        let mut disconnected = AudioCapture::new(16_000, CaptureSource::Microphone);
        disconnected.start(None, Some(source), None).unwrap();
        let mut default_microphone = AudioCapture::new(16_000, CaptureSource::Microphone);
        let mut default_output = AudioCapture::new(16_000, CaptureSource::Speaker);
        default_microphone.start(None, None, None).unwrap();
        default_output.start(None, None, None).unwrap();
        let dump = std::process::Command::new("pw-dump").output().unwrap();
        assert!(dump.status.success());
        let graph: serde_json::Value = serde_json::from_slice(&dump.stdout).unwrap();
        let id = graph.as_array().unwrap().iter().find(|node| node["info"]["props"]["node.name"] == "vrcs-test-source").unwrap()["id"].as_u64().unwrap();
        assert!(std::process::Command::new("pw-cli").args(["destroy", &id.to_string()]).status().unwrap().success());
        let failure = tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop { if let Err(error) = disconnected.read().await { break error; } }
        }).await.unwrap();
        assert_eq!(failure.code(), "audio.device_unavailable");
        disconnected.shutdown().await;
        // A missing default must leave PCM open, while the explicit selection
        // above must fail instead of moving to another device.
        assert!(tokio::time::timeout(std::time::Duration::from_millis(400), async {
            loop { default_microphone.read().await.unwrap(); }
        }).await.is_err());
        assert!(std::process::Command::new("pw-cli").args(["create-node", "adapter",
            r#"{ factory.name = audiotestsrc node.name = vrcs-test-source node.group = test-clock media.class = Audio/Source node.driver = false object.linger = true audio.format = F32LE audio.rate = 48000 audio.channels = 2 audio.position = [ FL FR ] node.param.Props = { live = false wave = sine freq = 440 volume = 0.25 } }"#,
        ]).status().unwrap().success());
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        for channel in ["FL", "FR"] {
            loop {
                let status = std::process::Command::new("pw-link")
                    .args([format!("vrcs-test-source:capture_{channel}"), format!("vrcs-test-sink:playback_{channel}")])
                    .stderr(std::process::Stdio::null()).status().unwrap();
                if status.success() { break; }
                assert!(std::time::Instant::now() < deadline, "restored source ports did not appear");
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        }
        let samples = tokio::time::timeout(std::time::Duration::from_secs(5), default_microphone.read()).await.unwrap().unwrap();
        assert!(samples.iter().all(|sample| sample.is_finite()));
        assert!(samples.iter().any(|sample| sample.abs() > 0.01), "restored microphone has no tone");
        eprintln!("PipeWire default source restored; checking sustained microphone and monitor capture");
        let mut energy = [0.0f64; 2];
        for _ in 0..48 {
            let (microphone, output) = tokio::join!(
                tokio::time::timeout(std::time::Duration::from_secs(5), default_microphone.read()),
                tokio::time::timeout(std::time::Duration::from_secs(5), default_output.read()),
            );
            for (channel, samples) in [microphone.unwrap().unwrap(), output.unwrap().unwrap()].into_iter().enumerate() {
                assert_eq!(samples.len(), CHUNK_FRAMES);
                assert!(samples.iter().all(|sample| sample.is_finite()));
                energy[channel] += samples.iter().map(|&sample| f64::from(sample).powi(2)).sum::<f64>();
            }
        }
        assert!(energy.iter().all(|energy| *energy > 1.0), "restored microphone/monitor tone missing");
        // Both directions must also stop promptly while no default exists.
        let graph: serde_json::Value = serde_json::from_slice(&std::process::Command::new("pw-dump").output().unwrap().stdout).unwrap();
        for name in ["vrcs-test-source", "vrcs-test-sink"] {
            let id = graph.as_array().unwrap().iter().find(|node| node["info"]["props"]["node.name"] == name).unwrap()["id"].as_u64().unwrap();
            assert!(std::process::Command::new("pw-cli").args(["destroy", &id.to_string()]).status().unwrap().success());
        }
        for capture in [&mut default_microphone, &mut default_output] {
            assert!(tokio::time::timeout(std::time::Duration::from_millis(400), async {
                loop { capture.read().await.unwrap(); }
            }).await.is_err());
            let stop = std::time::Instant::now();
            capture.shutdown().await;
            assert!(stop.elapsed() < std::time::Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn player_startup_audio_precedes_new_frames_and_shutdown_closes_stream() {
        let (sender, receiver) = mpsc::channel(2);
        sender.send(vec![0.3; 512]).await.unwrap();
        let priming = [vec![0.1; 512], vec![0.2; 512]].into();
        let mut capture = AudioCapture::from_stream(receiver, priming);
        for value in [0.1, 0.2, 0.3] {
            assert_eq!(capture.read().await.unwrap(), vec![value; 512]);
        }
        capture.shutdown().await;
        assert!(sender.send(vec![0.4; 512]).await.is_err());
    }
}
