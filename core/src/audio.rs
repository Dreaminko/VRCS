//! 音频采集公共接口与生命周期管理。
//! Windows 的设备枚举、WASAPI 采集和 PCM 转换位于 `audio/wasapi/`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use tokio::sync::mpsc;

use crate::models::AudioDevice;

#[cfg(not(windows))]
mod platform;
#[cfg(windows)]
mod wasapi;
#[cfg(windows)]
use wasapi as platform;

pub const CHUNK_FRAMES: usize = 512;
const CHANNEL_CAPACITY: usize = 128;
const START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);

#[derive(Debug, Clone)]
pub struct AudioError {
    code: &'static str,
    message: String,
}

impl AudioError {
    fn new(message: impl Into<String>) -> Self {
        Self::with_code("audio.unavailable", message)
    }

    pub(crate) fn with_code(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }

    pub fn code(&self) -> &'static str {
        self.code
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
}

pub struct AudioCapture {
    output_rate: u32,
    source: CaptureSource,
    session: Option<CaptureSession>,
    device: Option<AudioDevice>,
}

impl AudioCapture {
    pub fn new(output_rate: u32, source: CaptureSource) -> Self {
        Self {
            output_rate,
            source,
            session: None,
            device: None,
        }
    }

    #[allow(dead_code)]
    pub fn device(&self) -> Option<&AudioDevice> {
        self.device.as_ref()
    }

    pub fn start(
        &mut self,
        device_id: Option<i64>,
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
        let target = match device_id {
            Some(id) => platform::CaptureTarget::Device {
                wasapi_id: Some(platform::resolve_device_id(id, self.source)?),
                direction,
            },
            None => platform::CaptureTarget::Device {
                wasapi_id: None,
                direction,
            },
        };
        self.start_session(target)
    }

    fn start_session(
        &mut self,
        target: platform::CaptureTarget,
    ) -> Result<AudioDevice, AudioError> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<AudioDevice, String>>();
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let output_rate = self.output_rate;
        let join = std::thread::Builder::new()
            .name("vrcs-audio-capture".into())
            .spawn(move || {
                platform::capture_main(target, output_rate, thread_stop, tx, ready_tx);
            })
            .map_err(|error| {
                AudioError::new(format!("Failed to start audio capture thread: {error}"))
            })?;

        let device = match ready_rx.recv_timeout(START_TIMEOUT) {
            Ok(Ok(device)) => device,
            Ok(Err(message)) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError::new(message));
            }
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                let _ = join.join();
                return Err(AudioError::new("Timed out while starting audio capture"));
            }
        };
        self.session = Some(CaptureSession {
            stop,
            join: Some(join),
            rx,
        });
        self.device = Some(device.clone());
        Ok(device)
    }

    pub async fn read(&mut self) -> Result<Vec<f32>, AudioError> {
        let session = self
            .session
            .as_mut()
            .ok_or_else(|| AudioError::new("Audio capture is not running"))?;
        session
            .rx
            .recv()
            .await
            .ok_or_else(|| AudioError::new("Audio capture has stopped"))
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

pub fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    platform::list_devices()
}

pub fn validate_device_id(
    device_id: i64,
    source: CaptureSource,
) -> Result<AudioDevice, AudioError> {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let device = capture.start(None, None).unwrap();
        assert!(device.is_loopback);
        for _ in 0..3 {
            let chunk = capture.read().await.unwrap();
            assert_eq!(chunk.len(), CHUNK_FRAMES);
        }
        capture.stop();
        assert!(capture.device().is_none());
    }
}
