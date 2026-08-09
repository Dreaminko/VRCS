use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ::wasapi::{AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};
use tokio::sync::mpsc;

use crate::models::AudioDevice;

use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{endpoint_info, err, find_device_by_wasapi_id, init_com};
use super::pcm::{append_mono_f32, resample_linear, NativeFormat, SampleEncoding};
use super::{CaptureTarget, DeviceDirection};

const BUFFER_DURATION_HNS: i64 = 200_000;
const EVENT_WAIT_MS: u32 = 200;

pub(crate) fn capture_main(
    target: CaptureTarget,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, String>>,
) {
    let initialize = || -> Result<(AudioClient, AudioDevice, NativeFormat), AudioError> {
        init_com()?;
        let (mut client, device, wave_format, native) = match &target {
            CaptureTarget::Process(process_id) => {
                let client = AudioClient::new_application_loopback_client(*process_id, true)
                    .map_err(|error| {
                        AudioError::new(format!("Failed to connect to VRChat audio: {error}"))
                    })?;
                let device = AudioDevice {
                    id: -1,
                    name: "VRChat（仅应用音频）".into(),
                    is_default: false,
                    is_loopback: true,
                    sample_rate: output_rate,
                    channels: 1,
                };
                let format =
                    WaveFormat::new(16, 16, &SampleType::Int, output_rate as usize, 1, None);
                (
                    client,
                    device,
                    format,
                    NativeFormat {
                        sample_rate: output_rate,
                        channels: 1,
                        encoding: SampleEncoding::SignedInt {
                            container_bytes: 2,
                            valid_bits: 16,
                        },
                    },
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
                let device = match wasapi_id {
                    Some(id) => find_device_by_wasapi_id(id, &wasapi_direction)?,
                    None => {
                        let enumerator = DeviceEnumerator::new().map_err(err)?;
                        enumerator
                            .get_default_device(&wasapi_direction)
                            .map_err(err)?
                    }
                };
                let wave_format = device.get_device_format().map_err(err)?;
                let native = NativeFormat::from_wave_format(&wave_format)?;
                let info = endpoint_info(
                    &device,
                    wasapi_id.is_none(),
                    *direction == DeviceDirection::Render,
                )?;
                (
                    device.get_iaudioclient().map_err(err)?,
                    info,
                    wave_format,
                    native,
                )
            }
        };
        let mode = StreamMode::EventsShared {
            autoconvert: matches!(target, CaptureTarget::Process(_)),
            buffer_duration_hns: BUFFER_DURATION_HNS,
        };
        client
            .initialize_client(&wave_format, &Direction::Capture, &mode)
            .map_err(err)?;
        Ok((client, device, native))
    };

    let (client, device, native) = match initialize() {
        Ok(initialized) => initialized,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };

    let stream = (|| -> Result<(), AudioError> {
        let event = client.set_get_eventhandle().map_err(err)?;
        let capture = client.get_audiocaptureclient().map_err(err)?;
        client.start_stream().map_err(err)?;
        let _ = ready.send(Ok(device));

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
