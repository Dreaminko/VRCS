use ::wasapi::{initialize_mta, Device, DeviceEnumerator, Direction};

use crate::models::AudioDevice;

use super::super::{AudioError, CaptureSource};

pub(super) fn err<E: std::fmt::Display>(error: E) -> AudioError {
    AudioError::new(error.to_string())
}

pub(super) fn init_com() -> Result<(), AudioError> {
    let result = initialize_mta();
    if result.is_ok() {
        Ok(())
    } else {
        Err(AudioError::new(format!("COM 初始化失败：{result:?}")))
    }
}

fn device_key(wasapi_id: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in wasapi_id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash & ((1u64 << 53) - 1)) as i64
}

pub(super) fn endpoint_info(
    device: &Device,
    is_default: bool,
    is_loopback: bool,
) -> Result<AudioDevice, AudioError> {
    let wasapi_id = device.get_id().map_err(err)?;
    let format = device.get_device_format().map_err(err)?;
    Ok(AudioDevice {
        id: device_key(&wasapi_id),
        name: device.get_friendlyname().map_err(err)?,
        is_default,
        is_loopback,
        sample_rate: format.get_samplespersec(),
        channels: u32::from(format.get_nchannels()),
    })
}

pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    init_com()?;
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let default_render_id = enumerator
        .get_default_device(&Direction::Render)
        .and_then(|device| device.get_id())
        .ok();
    let default_capture_id = enumerator
        .get_default_device(&Direction::Capture)
        .and_then(|device| device.get_id())
        .ok();
    let mut devices = Vec::new();
    for (direction, is_loopback, default_id) in [
        (Direction::Render, true, default_render_id),
        (Direction::Capture, false, default_capture_id),
    ] {
        let collection = enumerator.get_device_collection(&direction).map_err(err)?;
        for index in 0..collection.get_nbr_devices().map_err(err)? {
            let device = collection.get_device_at_index(index).map_err(err)?;
            if device.get_state().map_err(err)? != ::wasapi::DeviceState::Active {
                continue;
            }
            let wasapi_id = device.get_id().map_err(err)?;
            let is_default = default_id.as_deref() == Some(wasapi_id.as_str());
            devices.push(endpoint_info(&device, is_default, is_loopback)?);
        }
    }
    Ok(devices)
}

pub(crate) fn resolve_device_id(
    device_id: i64,
    source: CaptureSource,
) -> Result<String, AudioError> {
    let device = super::super::validate_device_id(device_id, source)?;
    let direction = match source {
        CaptureSource::Speaker => Direction::Render,
        CaptureSource::Microphone => Direction::Capture,
    };
    init_com()?;
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let collection = enumerator.get_device_collection(&direction).map_err(err)?;
    for index in 0..collection.get_nbr_devices().map_err(err)? {
        let device_handle = collection.get_device_at_index(index).map_err(err)?;
        let wasapi_id = device_handle.get_id().map_err(err)?;
        if device_key(&wasapi_id) == device.id {
            return Ok(wasapi_id);
        }
    }
    Err(AudioError::with_code(
        "audio.device_unavailable",
        "所选音频设备已失效，请重新选择",
    ))
}

pub(crate) fn find_process_id(process_name: &str) -> Result<Option<u32>, AudioError> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    let target = process_name.to_lowercase();
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
            .map_err(|error| AudioError::new(format!("无法枚举 Windows 进程：{error}")))?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut result = None;
        if Process32FirstW(snapshot, &mut entry).is_ok() {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|character| *character == 0)
                    .unwrap_or(entry.szExeFile.len());
                let executable = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if executable.to_lowercase() == target {
                    result = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snapshot);
        Ok(result)
    }
}

pub(super) fn find_device_by_wasapi_id(
    wasapi_id: &str,
    direction: &Direction,
) -> Result<Device, AudioError> {
    init_com()?;
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    if let Ok(device) = enumerator.get_device(wasapi_id) {
        return Ok(device);
    }
    let collection = enumerator.get_device_collection(direction).map_err(err)?;
    for index in 0..collection.get_nbr_devices().map_err(err)? {
        let device = collection.get_device_at_index(index).map_err(err)?;
        if device.get_id().map_err(err)? == wasapi_id {
            return Ok(device);
        }
    }
    Err(AudioError::with_code(
        "audio.device_unavailable",
        "所选音频设备已失效，请重新选择",
    ))
}

#[cfg(test)]
mod tests {
    use super::device_key;

    #[test]
    fn device_keys_are_javascript_safe() {
        let key = device_key("render-device-that-produces-a-large-fnv-hash");
        assert!((0..=9_007_199_254_740_991).contains(&key));
    }
}
