use super::{AudioDevice, AudioError};

pub(crate) enum CaptureTarget {
    Process(u32),
    Device {
        wasapi_id: Option<String>,
        direction: DeviceDirection,
    },
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum DeviceDirection {
    Render,
    Capture,
}

pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    Err(AudioError::new(
        "Audio capture is supported only on Windows",
    ))
}

pub(crate) fn resolve_device_id(
    _id: i64,
    _source: super::CaptureSource,
) -> Result<String, AudioError> {
    Err(AudioError::new(
        "Audio capture is supported only on Windows",
    ))
}

pub(crate) fn find_process_id(_name: &str) -> Result<Option<u32>, AudioError> {
    Err(AudioError::new(
        "Audio capture is supported only on Windows",
    ))
}

pub(crate) fn capture_main(
    _target: CaptureTarget,
    _rate: u32,
    _stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    _tx: tokio::sync::mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, String>>,
) {
    let _ = ready.send(Err("Audio capture is supported only on Windows".into()));
}
