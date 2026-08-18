use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ::wasapi::{
    AudioCaptureClient, AudioClient, Device, Direction, Handle, SampleType, ShareMode, StreamMode,
    WaveFormat,
};
use tokio::sync::mpsc;

use crate::models::AudioDevice;

use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{endpoint_info, err, init_com, is_endpoint_invalidation, resolve_device};
use super::pcm::{append_mono_f32, resample_linear, NativeFormat, SampleEncoding};
use super::{CaptureTarget, DeviceDirection};

const SHARED_BUFFER_DURATION_HNS: i64 = 0;
const EVENT_WAIT_MS: u32 = 200;
const DEFAULT_RECOVERY_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

fn map_initialize_error(error: ::wasapi::WasapiError) -> AudioError {
    use windows::Win32::Foundation::ERROR_OUTOFMEMORY;

    if matches!(
        &error,
        ::wasapi::WasapiError::Windows(error)
            if error.code() == windows::core::HRESULT::from_win32(ERROR_OUTOFMEMORY.0)
    ) {
        AudioError::retryable_with_code("audio.unavailable", error.to_string())
    } else {
        err(error)
    }
}

fn initialize_loopback_client(device: &Device) -> Result<(AudioClient, NativeFormat), AudioError> {
    let mut client = device
        .get_iaudioclient()
        .map_err(|error| err(error).at_stage("activate_client"))?;
    let wave_format = device
        .get_device_format()
        .map_err(|error| err(error).at_stage("read_device_format"))?;
    let native = NativeFormat::from_wave_format(&wave_format)?;
    let mode = StreamMode::EventsShared {
        autoconvert: false,
        buffer_duration_hns: SHARED_BUFFER_DURATION_HNS,
    };
    client
        .initialize_client(&wave_format, &Direction::Capture, &mode)
        .map_err(|error| map_initialize_error(error).at_stage("initialize_client"))?;
    Ok((client, native))
}

fn initialize_capture_mix_format(
    device: &Device,
) -> Result<(AudioClient, NativeFormat), AudioError> {
    let mut client = device.get_iaudioclient().map_err(err)?;
    let mix_format = client.get_mixformat().map_err(err)?;
    let wave_format = client
        .is_supported(&mix_format, &ShareMode::Shared)
        .map_err(err)?
        .unwrap_or(mix_format);
    let native = NativeFormat::from_wave_format(&wave_format)?;
    let mode = StreamMode::EventsShared {
        autoconvert: false,
        buffer_duration_hns: SHARED_BUFFER_DURATION_HNS,
    };
    client
        .initialize_client(&wave_format, &Direction::Capture, &mode)
        .map_err(map_initialize_error)?;
    Ok((client, native))
}

fn initialize_capture_client(
    device: &Device,
    output_rate: u32,
) -> Result<(AudioClient, NativeFormat), AudioError> {
    match initialize_capture_mix_format(device) {
        Ok(initialized) => Ok(initialized),
        Err(error) if error.code() == "audio.unsupported_format" => {
            tracing::warn!(
                output_rate,
                "WASAPI rejected the shared mix format; using automatic format conversion"
            );
            let mut client = device.get_iaudioclient().map_err(err)?;
            let wave_format =
                WaveFormat::new(32, 32, &SampleType::Float, output_rate as usize, 1, None);
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: SHARED_BUFFER_DURATION_HNS,
            };
            client
                .initialize_client(&wave_format, &Direction::Capture, &mode)
                .map_err(map_initialize_error)?;
            Ok((
                client,
                NativeFormat {
                    sample_rate: output_rate,
                    channels: 1,
                    encoding: SampleEncoding::Float32,
                },
            ))
        }
        Err(error) => Err(error),
    }
}

struct CaptureStream {
    client: AudioClient,
    event: Handle,
    capture: AudioCaptureClient,
    device: AudioDevice,
    native: NativeFormat,
    endpoint_id: String,
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

fn open_stream(target: &CaptureTarget, output_rate: u32) -> Result<CaptureStream, AudioError> {
    let process_loopback = matches!(target, CaptureTarget::Process(_));
    let (client, device, native, selection, endpoint_id) = match target {
        CaptureTarget::Process(process_id) => {
            init_com()?;
            let mut client = AudioClient::new_application_loopback_client(*process_id, true)
                .map_err(|error| map_capture_error(error, true))?;
            let device = AudioDevice {
                id: -1,
                name: "VRChat（仅应用音频）".into(),
                is_default: false,
                is_loopback: true,
                sample_rate: output_rate,
                channels: 1,
            };
            let wave_format =
                WaveFormat::new(16, 16, &SampleType::Int, output_rate as usize, 1, None);
            let mode = StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: SHARED_BUFFER_DURATION_HNS,
            };
            client
                .initialize_client(&wave_format, &Direction::Capture, &mode)
                .map_err(|error| {
                    map_initialize_error(error)
                        .with_default_code("audio.process_loopback_unavailable")
                })?;
            (
                client,
                device,
                NativeFormat {
                    sample_rate: output_rate,
                    channels: 1,
                    encoding: SampleEncoding::SignedInt {
                        container_bytes: 2,
                        valid_bits: 16,
                    },
                },
                "process",
                format!("process:{process_id}"),
            )
        }
        CaptureTarget::Device {
            wasapi_id,
            direction,
        } => {
            let wasapi_direction = match direction {
                DeviceDirection::Render => Direction::Render,
                DeviceDirection::Capture => Direction::Capture,
            };
            let device = resolve_device(wasapi_id.as_deref(), &wasapi_direction)?;
            let endpoint_id = device.get_id().map_err(err)?;
            let info = endpoint_info(
                &device,
                wasapi_id.is_none(),
                *direction == DeviceDirection::Render,
            )?;
            let (client, native) = match direction {
                DeviceDirection::Render => initialize_loopback_client(&device)?,
                DeviceDirection::Capture => initialize_capture_client(&device, output_rate)?,
            };
            let selection = match (wasapi_id.is_none(), direction) {
                (true, DeviceDirection::Render) => "default-render",
                (true, DeviceDirection::Capture) => "default-capture",
                (false, DeviceDirection::Render) => "explicit-render",
                (false, DeviceDirection::Capture) => "explicit-capture",
            };
            (client, info, native, selection, endpoint_id)
        }
    };

    let event = client
        .set_get_eventhandle()
        .map_err(|error| map_capture_error(error, process_loopback).at_stage("create_event"))?;
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
        "initialized WASAPI capture"
    );
    Ok(CaptureStream {
        client,
        event,
        capture,
        device,
        native,
        endpoint_id,
    })
}

fn stream_failure(error: ::wasapi::WasapiError, process_loopback: bool) -> StreamFailure {
    let endpoint_invalidated = is_endpoint_invalidation(&error);
    StreamFailure {
        error: map_capture_error(error, process_loopback),
        endpoint_invalidated,
    }
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
    while !stop.load(Ordering::Relaxed) {
        let _ = stream.event.wait_for_event(EVENT_WAIT_MS);
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
    let mut stream = match open_stream(&target, output_rate) {
        Ok(stream) => stream,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("initialize")));
            return;
        }
    };
    let _ = ready.send(Ok(stream.device.clone()));

    let mut recovered = false;
    loop {
        let result = run_stream(&stream, output_rate, &stop, &tx, process_loopback);
        let _ = stream.client.stop_stream();
        match result {
            Ok(()) => return,
            Err(failure)
                if follows_default
                    && failure.endpoint_invalidated
                    && !recovered
                    && !stop.load(Ordering::Relaxed) =>
            {
                recovered = true;
                tracing::warn!(
                    endpoint_id = %stream.endpoint_id,
                    detail = %failure.error,
                    "default audio endpoint was invalidated; rebuilding capture once"
                );
                std::thread::sleep(DEFAULT_RECOVERY_DELAY);
                match open_stream(&target, output_rate) {
                    Ok(replacement) => {
                        stream = replacement;
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(detail = %error, "failed to rebuild default audio capture");
                        return;
                    }
                }
            }
            Err(failure) => {
                tracing::warn!(detail = %failure.error, "audio capture stopped with error");
                return;
            }
        }
    }
}
