use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ::wasapi::{
    AudioClient, Device, DeviceEnumerator, Direction, SampleType, ShareMode, StreamMode, WaveFormat,
};
use tokio::sync::mpsc;

use crate::models::AudioDevice;

use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{endpoint_info, err, find_device_by_wasapi_id, init_com};
use super::pcm::{append_mono_f32, resample_linear, NativeFormat, SampleEncoding};
use super::{CaptureTarget, DeviceDirection};

const BUFFER_DURATION_HNS: i64 = 200_000;
const EVENT_WAIT_MS: u32 = 200;

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

fn initialize_mix_format(device: &Device) -> Result<(AudioClient, NativeFormat), AudioError> {
    let mut client = device.get_iaudioclient().map_err(err)?;
    let mix_format = client.get_mixformat().map_err(err)?;
    let wave_format = client
        .is_supported(&mix_format, &ShareMode::Shared)
        .map_err(err)?
        .unwrap_or(mix_format);
    let native = NativeFormat::from_wave_format(&wave_format)?;
    let mode = StreamMode::EventsShared {
        autoconvert: false,
        buffer_duration_hns: BUFFER_DURATION_HNS,
    };
    client
        .initialize_client(&wave_format, &Direction::Capture, &mode)
        .map_err(map_initialize_error)?;
    Ok((client, native))
}

fn initialize_device_client(
    device: &Device,
    output_rate: u32,
) -> Result<(AudioClient, NativeFormat), AudioError> {
    match initialize_mix_format(device) {
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
                buffer_duration_hns: BUFFER_DURATION_HNS,
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

pub(crate) fn capture_main(
    target: CaptureTarget,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
) {
    let process_loopback = matches!(&target, CaptureTarget::Process(_));
    let map_capture_error = |error| {
        let error = err(error);
        if process_loopback {
            error.with_default_code("audio.process_loopback_unavailable")
        } else {
            error
        }
    };
    let initialize = || -> Result<(AudioClient, AudioDevice, NativeFormat), AudioError> {
        init_com()?;
        match &target {
            CaptureTarget::Process(process_id) => {
                let mut client = AudioClient::new_application_loopback_client(*process_id, true)
                    .map_err(|error| map_capture_error(error))?;
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
                    buffer_duration_hns: BUFFER_DURATION_HNS,
                };
                client
                    .initialize_client(&wave_format, &Direction::Capture, &mode)
                    .map_err(|error| {
                        map_initialize_error(error)
                            .with_default_code("audio.process_loopback_unavailable")
                    })?;
                Ok((
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
                ))
            }
            CaptureTarget::Device {
                wasapi_id,
                direction,
            } => {
                let wasapi_direction = match direction {
                    DeviceDirection::Render => Direction::Render,
                    DeviceDirection::Capture => Direction::Capture,
                };
                let device = match wasapi_id {
                    Some(id) => find_device_by_wasapi_id(id, &wasapi_direction)?,
                    None => {
                        let enumerator = DeviceEnumerator::new().map_err(err)?;
                        enumerator
                            .get_default_device(&wasapi_direction)
                            .map_err(err)?
                    }
                };
                let info = endpoint_info(
                    &device,
                    wasapi_id.is_none(),
                    *direction == DeviceDirection::Render,
                )?;
                let (client, native) = initialize_device_client(&device, output_rate)?;
                Ok((client, info, native))
            }
        }
    };

    let (client, device, native) = match initialize() {
        Ok(initialized) => initialized,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("initialize")));
            return;
        }
    };

    let event = match client
        .set_get_eventhandle()
        .map_err(|error| map_capture_error(error))
    {
        Ok(event) => event,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("create_event")));
            return;
        }
    };
    let capture = match client
        .get_audiocaptureclient()
        .map_err(|error| map_capture_error(error))
    {
        Ok(capture) => capture,
        Err(error) => {
            let _ = ready.send(Err(error.at_stage("get_capture_client")));
            return;
        }
    };
    if let Err(error) = client
        .start_stream()
        .map_err(|error| map_capture_error(error))
    {
        let _ = ready.send(Err(error.at_stage("start_stream")));
        return;
    }
    let _ = ready.send(Ok(device));

    let stream = (|| -> Result<(), AudioError> {
        let frames_per_chunk = ((native.sample_rate as u64 * CHUNK_FRAMES as u64
            + output_rate as u64 / 2)
            / output_rate as u64)
            .max(1) as usize;
        let mut pending = VecDeque::<f32>::new();
        let mut packet = VecDeque::<u8>::new();
        while !stop.load(Ordering::Relaxed) {
            let _ = event.wait_for_event(EVENT_WAIT_MS);
            while let Some(size) = capture.get_next_packet_size().map_err(err)? {
                if size == 0 {
                    break;
                }
                packet.clear();
                capture
                    .read_from_device_to_deque(&mut packet)
                    .map_err(err)?;
                append_mono_f32(&mut packet, native, &mut pending)?;
                while pending.len() >= frames_per_chunk {
                    let frame: Vec<f32> = pending.drain(..frames_per_chunk).collect();
                    let chunk = resample_linear(&frame, output_rate, native.sample_rate);
                    if !chunk.is_empty() {
                        let _ = tx.try_send(chunk);
                    }
                }
            }
        }
        Ok(())
    })();
    let _ = client.stop_stream();
    if let Err(error) = stream {
        tracing::warn!("audio capture stopped with error: {error}");
    }
}
