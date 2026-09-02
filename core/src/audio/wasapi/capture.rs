use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ::wasapi::{
    AudioCaptureClient, AudioClient, AudioClientProperties, Device, Direction, Handle, SampleType,
    ShareMode, StreamMode, StreamOption, WaveFormat,
};
use tokio::sync::mpsc;

use crate::models::AudioDevice;

use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{
    default_device_id, endpoint_info, err, init_com, is_endpoint_invalidation, resolve_device,
    ComApartment,
};
use super::pcm::{append_mono_f32, resample_linear, NativeFormat, SampleEncoding};
use super::{CaptureTarget, DeviceDirection};

const SHARED_BUFFER_DURATION_HNS: i64 = 0;
const POLLING_BUFFER_DURATION_HNS: i64 = 200_000;
const EVENT_WAIT_MS: u32 = 200;
const POLLING_WAIT: std::time::Duration = std::time::Duration::from_millis(10);
const DEFAULT_RECOVERY_DELAY: std::time::Duration = std::time::Duration::from_millis(150);
const DEFAULT_DEVICE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

fn is_out_of_memory(error: &::wasapi::WasapiError) -> bool {
    use windows::Win32::Foundation::ERROR_OUTOFMEMORY;

    matches!(
        error,
        ::wasapi::WasapiError::Windows(error)
            if error.code() == windows::core::HRESULT::from_win32(ERROR_OUTOFMEMORY.0)
    )
}

fn map_initialize_error(error: ::wasapi::WasapiError) -> AudioError {
    if is_out_of_memory(&error) {
        AudioError::with_code("audio.unavailable", error.to_string())
    } else {
        err(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CaptureTiming {
    Events,
    Polling,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct InitializePlan {
    timing: CaptureTiming,
    raw: bool,
}

const INITIALIZE_PLANS: [InitializePlan; 3] = [
    InitializePlan {
        timing: CaptureTiming::Events,
        raw: false,
    },
    InitializePlan {
        timing: CaptureTiming::Polling,
        raw: false,
    },
    InitializePlan {
        timing: CaptureTiming::Polling,
        raw: true,
    },
];

impl InitializePlan {
    fn stream_mode(self, autoconvert: bool) -> StreamMode {
        match self.timing {
            CaptureTiming::Events => StreamMode::EventsShared {
                autoconvert,
                buffer_duration_hns: SHARED_BUFFER_DURATION_HNS,
            },
            CaptureTiming::Polling => StreamMode::PollingShared {
                autoconvert,
                buffer_duration_hns: POLLING_BUFFER_DURATION_HNS,
            },
        }
    }

    fn stage(self) -> &'static str {
        match (self.timing, self.raw) {
            (CaptureTiming::Events, false) => "initialize_client_events",
            (CaptureTiming::Polling, false) => "initialize_client_polling",
            (_, true) => "initialize_client_raw",
        }
    }
}

fn initialize_shared_client(
    client: AudioClient,
    mut recreate_client: impl FnMut() -> Result<AudioClient, AudioError>,
    wave_format: &WaveFormat,
    autoconvert: bool,
    raw_fallback: bool,
) -> Result<(AudioClient, CaptureTiming), AudioError> {
    let plans = if raw_fallback {
        &INITIALIZE_PLANS[..]
    } else {
        &INITIALIZE_PLANS[..2]
    };
    let mut first_client = Some(client);
    let mut previous_oom = None;
    for (index, plan) in plans.iter().copied().enumerate() {
        let mut client = match first_client.take() {
            Some(client) => client,
            None => recreate_client()?,
        };
        if plan.raw {
            if let Err(error) =
                client.set_properties(AudioClientProperties::new().set_option(StreamOption::Raw))
            {
                tracing::warn!(detail = %error, "WASAPI RAW fallback is unavailable");
                let error = previous_oom.expect("RAW fallback follows an out-of-memory error");
                return Err(map_initialize_error(error).at_stage(plan.stage()));
            }
        }
        let mode = plan.stream_mode(autoconvert);
        match client.initialize_client(wave_format, &Direction::Capture, &mode) {
            Ok(()) => return Ok((client, plan.timing)),
            Err(error) if is_out_of_memory(&error) && index + 1 < plans.len() => {
                let next = plans[index + 1];
                tracing::warn!(
                    detail = %error,
                    timing = ?plan.timing,
                    next_timing = ?next.timing,
                    next_raw = next.raw,
                    "WASAPI initialization ran out of resources; trying a fallback mode"
                );
                previous_oom = Some(error);
            }
            Err(error) => {
                return Err(map_initialize_error(error).at_stage(plan.stage()));
            }
        }
    }
    unreachable!("WASAPI initialization plans are non-empty")
}

fn initialize_loopback_client(
    device: &Device,
) -> Result<(AudioClient, NativeFormat, CaptureTiming), AudioError> {
    let client = device
        .get_iaudioclient()
        .map_err(|error| err(error).at_stage("activate_client"))?;
    let wave_format = device
        .get_device_format()
        .map_err(|error| err(error).at_stage("read_device_format"))?;
    let native = NativeFormat::from_wave_format(&wave_format)?;
    let (client, timing) = initialize_shared_client(
        client,
        || {
            device
                .get_iaudioclient()
                .map_err(|error| err(error).at_stage("activate_client"))
        },
        &wave_format,
        false,
        true,
    )?;
    Ok((client, native, timing))
}

fn initialize_capture_mix_format(
    device: &Device,
) -> Result<(AudioClient, NativeFormat, CaptureTiming), AudioError> {
    let client = device.get_iaudioclient().map_err(err)?;
    let mix_format = client.get_mixformat().map_err(err)?;
    let wave_format = client
        .is_supported(&mix_format, &ShareMode::Shared)
        .map_err(err)?
        .unwrap_or(mix_format);
    let native = NativeFormat::from_wave_format(&wave_format)?;
    let (client, timing) = initialize_shared_client(
        client,
        || device.get_iaudioclient().map_err(err),
        &wave_format,
        false,
        true,
    )?;
    Ok((client, native, timing))
}

fn initialize_capture_client(
    device: &Device,
    output_rate: u32,
) -> Result<(AudioClient, NativeFormat, CaptureTiming), AudioError> {
    match initialize_capture_mix_format(device) {
        Ok(initialized) => Ok(initialized),
        Err(error) if error.code() == "audio.unsupported_format" => {
            tracing::warn!(
                output_rate,
                "WASAPI rejected the shared mix format; using automatic format conversion"
            );
            let client = device.get_iaudioclient().map_err(err)?;
            let wave_format =
                WaveFormat::new(32, 32, &SampleType::Float, output_rate as usize, 1, None);
            let (client, timing) = initialize_shared_client(
                client,
                || device.get_iaudioclient().map_err(err),
                &wave_format,
                true,
                true,
            )?;
            Ok((
                client,
                NativeFormat {
                    sample_rate: output_rate,
                    channels: 1,
                    encoding: SampleEncoding::Float32,
                },
                timing,
            ))
        }
        Err(error) => Err(error),
    }
}

struct CaptureStream {
    capture: AudioCaptureClient,
    wait: CaptureWait,
    client: AudioClient,
    device: AudioDevice,
    native: NativeFormat,
    endpoint_id: String,
    default_direction: Option<Direction>,
}

enum CaptureWait {
    Event(Handle),
    Polling,
}

struct StreamFailure {
    error: AudioError,
    endpoint_invalidated: bool,
}

fn map_capture_error(error: ::wasapi::WasapiError, process_loopback: bool) -> AudioError {
    let error = err(error);
    if process_loopback {
        error.with_default_code("audio.process_loopback_unavailable")
    } else {
        error
    }
}

fn open_stream(
    com: &ComApartment,
    target: &CaptureTarget,
    output_rate: u32,
) -> Result<CaptureStream, AudioError> {
    let process_loopback = matches!(target, CaptureTarget::Process(_));
    let (client, device, native, timing, selection, endpoint_id, default_direction) = match target {
        CaptureTarget::Process(process_id) => {
            let render_device = resolve_device(com, None, &Direction::Render)?;
            let wave_format = render_device
                .get_device_format()
                .map_err(|error| err(error).at_stage("read_device_format"))?;
            let native = NativeFormat::from_wave_format(&wave_format)?;
            let client = AudioClient::new_application_loopback_client(*process_id, true)
                .map_err(|error| map_capture_error(error, true))?;
            let device = AudioDevice {
                id: -1,
                name: "VRChat（仅应用音频）".into(),
                is_default: false,
                is_loopback: true,
                sample_rate: output_rate,
                channels: 1,
            };
            let endpoint_id = format!("process:{process_id}");
            tracing::info!(
                selection = "process",
                endpoint_id = %endpoint_id,
                device_name = %device.name,
                "initializing WASAPI capture"
            );
            let (client, timing) = initialize_shared_client(
                client,
                || {
                    AudioClient::new_application_loopback_client(*process_id, true)
                        .map_err(|error| map_capture_error(error, true))
                },
                &wave_format,
                false,
                false,
            )
            .map_err(|error| error.with_default_code("audio.process_loopback_unavailable"))?;
            (client, device, native, timing, "process", endpoint_id, None)
        }
        CaptureTarget::Device {
            wasapi_id,
            direction,
        } => {
            let wasapi_direction = match direction {
                DeviceDirection::Render => Direction::Render,
                DeviceDirection::Capture => Direction::Capture,
            };
            let device = resolve_device(com, wasapi_id.as_deref(), &wasapi_direction)?;
            let endpoint_id = device.get_id().map_err(err)?;
            let info = endpoint_info(
                &device,
                wasapi_id.is_none(),
                *direction == DeviceDirection::Render,
            )?;
            let selection = match (wasapi_id.is_none(), direction) {
                (true, DeviceDirection::Render) => "default-render",
                (true, DeviceDirection::Capture) => "default-capture",
                (false, DeviceDirection::Render) => "explicit-render",
                (false, DeviceDirection::Capture) => "explicit-capture",
            };
            tracing::info!(
                selection,
                endpoint_id = %endpoint_id,
                device_name = %info.name,
                "initializing WASAPI capture"
            );
            let (client, native, timing) = match direction {
                DeviceDirection::Render => initialize_loopback_client(&device)?,
                DeviceDirection::Capture => initialize_capture_client(&device, output_rate)?,
            };
            let default_direction = wasapi_id.is_none().then_some(wasapi_direction);
            (
                client,
                info,
                native,
                timing,
                selection,
                endpoint_id,
                default_direction,
            )
        }
    };

    let wait = match timing {
        CaptureTiming::Events => {
            CaptureWait::Event(client.set_get_eventhandle().map_err(|error| {
                map_capture_error(error, process_loopback).at_stage("create_event")
            })?)
        }
        CaptureTiming::Polling => CaptureWait::Polling,
    };
    let capture = client.get_audiocaptureclient().map_err(|error| {
        map_capture_error(error, process_loopback).at_stage("get_capture_client")
    })?;
    client
        .start_stream()
        .map_err(|error| map_capture_error(error, process_loopback).at_stage("start_stream"))?;

    tracing::info!(
        selection,
        endpoint_id = %endpoint_id,
        device_name = %device.name,
        sample_rate = native.sample_rate,
        channels = native.channels,
        timing = ?timing,
        "initialized WASAPI capture"
    );
    Ok(CaptureStream {
        capture,
        wait,
        client,
        device,
        native,
        endpoint_id,
        default_direction,
    })
}

fn stream_failure(error: ::wasapi::WasapiError, process_loopback: bool) -> StreamFailure {
    let endpoint_invalidated = is_endpoint_invalidation(&error);
    StreamFailure {
        error: map_capture_error(error, process_loopback),
        endpoint_invalidated,
    }
}

fn default_endpoint_changed(opened: &str, current: &str) -> bool {
    opened != current
}

fn should_recover_default(
    follows_default: bool,
    endpoint_invalidated: bool,
    stopped: bool,
) -> bool {
    follows_default && endpoint_invalidated && !stopped
}

fn run_stream(
    stream: &CaptureStream,
    output_rate: u32,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Vec<f32>>,
    process_loopback: bool,
) -> Result<(), StreamFailure> {
    let frames_per_chunk = ((stream.native.sample_rate as u64 * CHUNK_FRAMES as u64
        + output_rate as u64 / 2)
        / output_rate as u64)
        .max(1) as usize;
    let mut pending = VecDeque::<f32>::new();
    let mut packet = VecDeque::<u8>::new();
    let mut next_default_check = std::time::Instant::now() + DEFAULT_DEVICE_CHECK_INTERVAL;
    while !stop.load(Ordering::Relaxed) {
        match &stream.wait {
            CaptureWait::Event(event) => {
                let _ = event.wait_for_event(EVENT_WAIT_MS);
            }
            CaptureWait::Polling => std::thread::sleep(POLLING_WAIT),
        }
        if let Some(direction) = &stream.default_direction {
            let now = std::time::Instant::now();
            if now >= next_default_check {
                next_default_check = now + DEFAULT_DEVICE_CHECK_INTERVAL;
                match default_device_id(direction) {
                    Ok(current) if default_endpoint_changed(&stream.endpoint_id, &current) => {
                        return Err(StreamFailure {
                            error: AudioError::retryable_with_code(
                                "audio.device_unavailable",
                                "The Windows default audio device changed",
                            ),
                            endpoint_invalidated: true,
                        });
                    }
                    Ok(_) => {}
                    Err(error) => tracing::debug!(
                        ?direction,
                        detail = %error,
                        "could not inspect the current default audio endpoint"
                    ),
                }
            }
        }
        while let Some(size) = stream
            .capture
            .get_next_packet_size()
            .map_err(|error| stream_failure(error, process_loopback))?
        {
            if size == 0 {
                break;
            }
            packet.clear();
            stream
                .capture
                .read_from_device_to_deque(&mut packet)
                .map_err(|error| stream_failure(error, process_loopback))?;
            append_mono_f32(&mut packet, stream.native, &mut pending).map_err(|error| {
                StreamFailure {
                    error,
                    endpoint_invalidated: false,
                }
            })?;
            while pending.len() >= frames_per_chunk {
                let frame: Vec<f32> = pending.drain(..frames_per_chunk).collect();
                let chunk = resample_linear(&frame, output_rate, stream.native.sample_rate);
                if !chunk.is_empty() {
                    let _ = tx.try_send(chunk);
                }
            }
        }
    }
    Ok(())
}

pub(crate) fn capture_main(
    target: CaptureTarget,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
) {
    let follows_default = matches!(
        &target,
        CaptureTarget::Device {
            wasapi_id: None,
            ..
        }
    );
    let process_loopback = matches!(&target, CaptureTarget::Process(_));
    let _com = match init_com() {
        Ok(com) => com,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("initialize")));
            return;
        }
    };
    let mut stream = match open_stream(&_com, &target, output_rate) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("initialize")));
            return;
        }
    };
    let _ = ready.send(Ok(stream.device.clone()));

    loop {
        let result = run_stream(&stream, output_rate, &stop, &tx, process_loopback);
        let _ = stream.client.stop_stream();
        match result {
            Ok(()) => return,
            Err(failure)
                if should_recover_default(
                    follows_default,
                    failure.endpoint_invalidated,
                    stop.load(Ordering::Relaxed),
                ) =>
            {
                let endpoint_id = stream.endpoint_id.clone();
                tracing::warn!(
                    endpoint_id = %endpoint_id,
                    detail = %failure.error,
                    "default audio endpoint changed or was invalidated; rebuilding capture"
                );
                drop(stream);
                stream = loop {
                    if stop.load(Ordering::Relaxed) {
                        return;
                    }
                    std::thread::sleep(DEFAULT_RECOVERY_DELAY);
                    match open_stream(&_com, &target, output_rate) {
                        Ok(replacement) => break replacement,
                        Err(error) if error.is_retryable() => {
                            tracing::debug!(
                                detail = %error,
                                "default audio endpoint is not ready; retrying"
                            );
                        }
                        Err(error) => {
                            tracing::warn!(
                                detail = %error,
                                "failed to rebuild default audio capture"
                            );
                            return;
                        }
                    }
                };
                continue;
            }
            Err(failure) => {
                tracing::warn!(detail = %failure.error, "audio capture stopped with error");
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        default_endpoint_changed, map_initialize_error, should_recover_default, CaptureTiming,
        INITIALIZE_PLANS,
    };

    #[test]
    fn out_of_memory_does_not_repeat_identical_startup() {
        let windows_error = windows::core::Error::from_hresult(windows::core::HRESULT::from_win32(
            windows::Win32::Foundation::ERROR_OUTOFMEMORY.0,
        ));
        let error = map_initialize_error(::wasapi::WasapiError::Windows(windows_error));

        assert!(!error.is_retryable());
    }

    #[test]
    fn out_of_memory_recovery_ends_with_raw_polling() {
        let final_plan = *INITIALIZE_PLANS.last().unwrap();
        assert_eq!(INITIALIZE_PLANS.len(), 3);
        assert_eq!(final_plan.timing, CaptureTiming::Polling);
        assert!(final_plan.raw);
    }

    #[test]
    fn default_endpoint_change_requests_a_reopen() {
        assert!(!default_endpoint_changed("endpoint-a", "endpoint-a"));
        assert!(default_endpoint_changed("endpoint-a", "endpoint-b"));
    }

    #[test]
    fn every_default_endpoint_invalidation_is_recoverable() {
        assert!(should_recover_default(true, true, false));
        assert!(should_recover_default(true, true, false));
        assert!(!should_recover_default(false, true, false));
        assert!(!should_recover_default(true, false, false));
        assert!(!should_recover_default(true, true, true));
    }

    #[test]
    #[ignore]
    fn initializes_native_process_loopback_client() {
        let _com = super::init_com().unwrap();
        let stream = super::open_stream(
            &_com,
            &super::CaptureTarget::Process(std::process::id()),
            16_000,
        )
        .unwrap();
        assert!(stream.native.sample_rate > 0);
        stream.client.stop_stream().unwrap();
    }
}
