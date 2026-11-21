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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

pub(crate) fn find_process_id(name: &str) -> Result<Option<u32>, AudioError> {
    use std::os::unix::fs::MetadataExt;
    if name.eq_ignore_ascii_case("VRChat.exe") || name.eq_ignore_ascii_case("VRChat") {
        return crate::vrchat_process::game()
            .map(|found| found.map(|(owner, _)| owner.pid))
            .map_err(|detail| AudioError::with_code("audio.pipewire_failed", detail));
    }
    let target = name.to_ascii_lowercase();
    let uid = unsafe { libc::geteuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Ok(None);
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        if std::fs::metadata(format!("/proc/{pid}"))
            .ok()
            .is_none_or(|meta| meta.uid() != uid)
        {
            continue;
        }
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if comm.trim().eq_ignore_ascii_case(&target) {
            return Ok(Some(pid));
        }
        let cmdline = std::fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        let argv0 = cmdline.split(|&byte| byte == 0).next().unwrap_or_default();
        let base = String::from_utf8_lossy(argv0);
        let base = base.rsplit(['/', '\\']).next().unwrap_or("");
        if base.eq_ignore_ascii_case(&target) {
            return Ok(Some(pid));
        }
    }
    Ok(None)
}
