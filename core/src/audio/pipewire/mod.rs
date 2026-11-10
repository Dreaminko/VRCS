//! Native Linux audio capture through PipeWire 0.3.65+ / PipeWire 1.0.
//!
//! Streams deliberately omit `RT_PROCESS`: callbacks run on the capture thread's
//! main loop, copy into a bounded queue, and never invoke the microphone tap or
//! send blocking messages. This trades some scheduling latency for the existing
//! `Vec<f32>` channel contract without allocating on a real-time thread.

mod capture;
mod devices;

pub(crate) use capture::capture_main;
pub(crate) use devices::list_devices;

use super::{AudioError, CaptureSource};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DeviceDirection {
    Render,
    Capture,
}

#[derive(Clone, Debug)]
pub(crate) enum CaptureTarget {
    Process(u32),
    Device {
        backend_id: Option<String>,
        direction: DeviceDirection,
    },
}

pub(crate) fn resolve_device_id(_id: i64, _source: CaptureSource) -> Result<String, AudioError> {
    Err(AudioError::with_code(
        "audio.device_reselection_required",
        "Linux audio devices require a PipeWire device key; select the device again",
    ))
}

pub(crate) fn find_process_id(_name: &str) -> Result<Option<u32>, AudioError> {
    Err(AudioError::with_code(
        "audio.process_capture_unsupported",
        "Application-only audio capture is not supported on Linux; select a speaker device",
    ))
}
