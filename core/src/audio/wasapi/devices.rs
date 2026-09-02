use ::wasapi::{deinitialize, initialize_mta, Device, DeviceEnumerator, Direction};

use crate::models::AudioDevice;

use super::super::{AudioError, CaptureSource};

pub(super) fn err(error: ::wasapi::WasapiError) -> AudioError {
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND};
    use windows::Win32::Media::Audio::{
        AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_ENDPOINT_CREATE_FAILED,
        AUDCLNT_E_RESOURCES_INVALIDATED, AUDCLNT_E_SERVICE_NOT_RUNNING,
        AUDCLNT_E_UNSUPPORTED_FORMAT,
    };

    let detail = error.to_string();
    match &error {
        ::wasapi::WasapiError::DeviceNotFound(_) => {
            AudioError::retryable_with_code("audio.device_unavailable", detail)
        }
        ::wasapi::WasapiError::UnsupportedFormat
        | ::wasapi::WasapiError::UnsupportedSubformat(_) => {
            AudioError::with_code("audio.unsupported_format", detail)
        }
        ::wasapi::WasapiError::Windows(error) => {
            let code = error.code();
            if code == windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
                AudioError::with_code("audio.permission_denied", detail)
            } else if code == windows::core::HRESULT::from_win32(ERROR_FILE_NOT_FOUND.0)
                || matches!(
                    code,
                    AUDCLNT_E_DEVICE_INVALIDATED
                        | AUDCLNT_E_ENDPOINT_CREATE_FAILED
                        | AUDCLNT_E_RESOURCES_INVALIDATED
                )
            {
                AudioError::retryable_with_code("audio.device_unavailable", detail)
            } else if code == AUDCLNT_E_DEVICE_IN_USE {
                AudioError::retryable_with_code("audio.device_in_use", detail)
            } else if code == AUDCLNT_E_SERVICE_NOT_RUNNING {
                AudioError::retryable_with_code("audio.service_not_running", detail)
            } else if code == AUDCLNT_E_UNSUPPORTED_FORMAT {
                AudioError::with_code("audio.unsupported_format", detail)
            } else {
                AudioError::new(detail)
            }
        }
        _ => AudioError::new(detail),
    }
}

pub(super) struct ComApartment(std::marker::PhantomData<std::rc::Rc<()>>);

impl Drop for ComApartment {
    fn drop(&mut self) {
        deinitialize();
    }
}

pub(super) fn init_com() -> Result<ComApartment, AudioError> {
    let result = initialize_mta();
    if result.is_ok() {
        Ok(ComApartment(std::marker::PhantomData))
    } else {
        Err(AudioError::with_code(
            "audio.com_initialization_failed",
            format!("COM initialization failed: {result:?}"),
        ))
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

pub(super) fn default_device_id(direction: &Direction) -> Result<String, AudioError> {
    DeviceEnumerator::new()
        .map_err(err)?
        .get_default_device(direction)
        .and_then(|device| device.get_id())
        .map_err(err)
}

fn push_enumerated_device(
    devices: &mut Vec<AudioDevice>,
    result: Result<AudioDevice, AudioError>,
    direction: &Direction,
    index: u32,
) {
    match result {
        Ok(device) => devices.push(device),
        Err(error) => tracing::warn!(
            ?direction,
            index,
            detail = %error,
            "skipping unavailable audio endpoint"
        ),
    }
}

fn enumerate_direction(
    enumerator: &DeviceEnumerator,
    direction: &Direction,
    is_loopback: bool,
    default_id: Option<&str>,
    devices: &mut Vec<AudioDevice>,
) -> Result<(), AudioError> {
    let collection = enumerator.get_device_collection(direction).map_err(err)?;
    let count = collection.get_nbr_devices().map_err(err)?;
    for index in 0..count {
        let endpoint = collection.get_device_at_index(index).and_then(|device| {
            if device.get_state()? != ::wasapi::DeviceState::Active {
                return Ok(None);
            }
            device.get_id().map(|id| Some((device, id)))
        });
        let (device, wasapi_id) = match endpoint {
            Ok(Some(endpoint)) => endpoint,
            Ok(None) => continue,
            Err(error) => {
                push_enumerated_device(devices, Err(err(error)), direction, index);
                continue;
            }
        };
        let is_default = default_id == Some(wasapi_id.as_str());
        push_enumerated_device(
            devices,
            endpoint_info(&device, is_default, is_loopback),
            direction,
            index,
        );
    }
    Ok(())
}

pub(crate) fn list_devices() -> Result<Vec<AudioDevice>, AudioError> {
    let _com = init_com()?;
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
    let mut successful_directions = 0;
    let mut last_error = None;
    for (direction, is_loopback, default_id) in [
        (Direction::Render, true, default_render_id),
        (Direction::Capture, false, default_capture_id),
    ] {
        match enumerate_direction(
            &enumerator,
            &direction,
            is_loopback,
            default_id.as_deref(),
            &mut devices,
        ) {
            Ok(()) => successful_directions += 1,
            Err(error) => {
                tracing::warn!(?direction, detail = %error, "failed to enumerate audio direction");
                last_error = Some(error);
            }
        }
    }
    if successful_directions == 0 {
        Err(last_error.unwrap_or_else(|| AudioError::new("No audio directions were enumerated")))
    } else {
        Ok(devices)
    }
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
    let _com = init_com()?;
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let collection = enumerator.get_device_collection(&direction).map_err(err)?;
    for index in 0..collection.get_nbr_devices().map_err(err)? {
        let wasapi_id = match collection
            .get_device_at_index(index)
            .and_then(|device| device.get_id())
        {
            Ok(id) => id,
            Err(error) => {
                tracing::warn!(?direction, index, detail = %error, "skipping unavailable audio endpoint");
                continue;
            }
        };
        if device_key(&wasapi_id) == device.id {
            return Ok(wasapi_id);
        }
    }
    Err(AudioError::with_code(
        "audio.device_unavailable",
        "The selected audio device is no longer available",
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
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|error| {
            AudioError::new(format!("Failed to enumerate Windows processes: {error}"))
        })?;
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

pub(super) fn resolve_device(
    _com: &ComApartment,
    wasapi_id: Option<&str>,
    direction: &Direction,
) -> Result<Device, AudioError> {
    let enumerator = DeviceEnumerator::new().map_err(err)?;
    let follows_default = wasapi_id.is_none();
    let endpoint_id = match wasapi_id {
        Some(id) => id.to_owned(),
        None => enumerator
            .get_default_device(direction)
            .and_then(|device| device.get_id())
            .map_err(err)?,
    };

    let collection = enumerator.get_device_collection(direction).map_err(err)?;
    for index in 0..collection.get_nbr_devices().map_err(err)? {
        let endpoint = collection
            .get_device_at_index(index)
            .and_then(|device| device.get_id().map(|id| (device, id)));
        let (device, id) = match endpoint {
            Ok(endpoint) => endpoint,
            Err(error) => {
                tracing::warn!(?direction, index, detail = %error, "skipping unavailable audio endpoint");
                continue;
            }
        };
        if id == endpoint_id {
            return Ok(device);
        }
    }
    Err(device_unavailable(follows_default))
}

fn device_unavailable(follows_default: bool) -> AudioError {
    let message = if follows_default {
        "The default audio device is changing or no longer available"
    } else {
        "The selected audio device is no longer available"
    };
    if follows_default {
        AudioError::retryable_with_code("audio.device_unavailable", message)
    } else {
        AudioError::with_code("audio.device_unavailable", message)
    }
}

pub(super) fn is_endpoint_invalidation(error: &::wasapi::WasapiError) -> bool {
    use windows::Win32::Media::Audio::{
        AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_RESOURCES_INVALIDATED,
    };

    matches!(
        error,
        ::wasapi::WasapiError::Windows(error)
            if matches!(
                error.code(),
                AUDCLNT_E_DEVICE_INVALIDATED | AUDCLNT_E_RESOURCES_INVALIDATED
            )
    )
}

#[cfg(test)]
mod tests {
    use super::{
        device_key, device_unavailable, err, init_com, is_endpoint_invalidation,
        push_enumerated_device,
    };
    use crate::audio::AudioError;
    use crate::models::AudioDevice;
    use ::wasapi::Direction;
    use windows::Win32::Foundation::{ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND};
    use windows::Win32::Media::Audio::{
        AUDCLNT_E_DEVICE_INVALIDATED, AUDCLNT_E_DEVICE_IN_USE, AUDCLNT_E_RESOURCES_INVALIDATED,
        AUDCLNT_E_SERVICE_NOT_RUNNING, AUDCLNT_E_UNSUPPORTED_FORMAT,
    };

    #[test]
    fn com_apartment_is_uninitialized_when_guard_drops() {
        std::thread::spawn(|| {
            let apartment = init_com().unwrap();
            drop(apartment);

            let second = ::wasapi::initialize_mta();
            if second.is_ok() {
                ::wasapi::deinitialize();
            }
            let balanced = second == windows::core::HRESULT(0);
            if second == windows::core::HRESULT(1) {
                ::wasapi::deinitialize();
            }
            assert!(balanced, "COM remained initialized after the guard dropped");
        })
        .join()
        .unwrap();
    }

    #[test]
    fn file_not_found_is_retryable_during_audio_startup() {
        let windows_error = windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(
            ERROR_FILE_NOT_FOUND.0,
        ));
        let error = err(::wasapi::WasapiError::Windows(windows_error));

        assert!(error.is_retryable());
        assert_eq!(error.code(), "audio.device_unavailable");
    }

    #[test]
    fn classifies_common_windows_audio_failures() {
        for (hresult, expected_code, retryable) in [
            (
                windows::core::HRESULT::from_win32(ERROR_ACCESS_DENIED.0),
                "audio.permission_denied",
                false,
            ),
            (AUDCLNT_E_DEVICE_IN_USE, "audio.device_in_use", true),
            (
                AUDCLNT_E_SERVICE_NOT_RUNNING,
                "audio.service_not_running",
                true,
            ),
            (
                AUDCLNT_E_UNSUPPORTED_FORMAT,
                "audio.unsupported_format",
                false,
            ),
        ] {
            let windows_error = windows::core::Error::from_hresult(hresult);
            let error = err(::wasapi::WasapiError::Windows(windows_error));
            assert_eq!(error.code(), expected_code);
            assert_eq!(error.is_retryable(), retryable);
        }
    }

    #[test]
    fn default_device_transition_is_retryable_but_explicit_loss_is_not() {
        assert!(device_unavailable(true).is_retryable());
        assert!(!device_unavailable(false).is_retryable());
    }

    #[test]
    fn runtime_recovery_is_limited_to_endpoint_invalidation() {
        for hresult in [
            AUDCLNT_E_DEVICE_INVALIDATED,
            AUDCLNT_E_RESOURCES_INVALIDATED,
        ] {
            let error = ::wasapi::WasapiError::Windows(windows::core::Error::from_hresult(hresult));
            assert!(is_endpoint_invalidation(&error));
        }

        let other = ::wasapi::WasapiError::Windows(windows::core::Error::from_hresult(
            AUDCLNT_E_DEVICE_IN_USE,
        ));
        assert!(!is_endpoint_invalidation(&other));
    }

    #[test]
    fn device_keys_are_javascript_safe() {
        let key = device_key("render-device-that-produces-a-large-fnv-hash");
        assert!((0..=9_007_199_254_740_991).contains(&key));
    }

    #[test]
    fn one_bad_endpoint_does_not_discard_healthy_devices() {
        let healthy = AudioDevice {
            id: 7,
            name: "healthy".into(),
            is_default: false,
            is_loopback: false,
            sample_rate: 48_000,
            channels: 1,
        };
        let mut devices = Vec::new();

        push_enumerated_device(
            &mut devices,
            Err(AudioError::with_code(
                "audio.unavailable",
                "broken endpoint",
            )),
            &Direction::Capture,
            0,
        );
        push_enumerated_device(&mut devices, Ok(healthy.clone()), &Direction::Capture, 1);

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id, healthy.id);
    }
}
