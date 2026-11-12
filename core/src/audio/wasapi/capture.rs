use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ::wasapi::{
    AudioCaptureClient, AudioClient, AudioClientProperties, Device, Direction, Handle, SampleType,
    ShareMode, StreamMode, StreamOption, WaveFormat,
};
use tokio::sync::mpsc;

use crate::models::AudioDevice;

use super::super::tap::{TapResampler, TAP_RATE};
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
const MAX_RECOVERY_DELAY: std::time::Duration = std::time::Duration::from_secs(10);
const DEFAULT_DEVICE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);

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
                tracing::warn!(stage = "set_raw_properties", detail = %error, "WASAPI RAW fallback is unavailable");
                return Err(previous_oom.expect("RAW fallback follows an out-of-memory error"));
            }
        }
        let mode = plan.stream_mode(autoconvert);
        match client
            .initialize_client(wave_format, &Direction::Capture, &mode)
            .map_err(|error| err(error).at_stage(plan.stage()))
        {
            Ok(()) => return Ok((client, plan.timing)),
            Err(error) if error.code() == "audio.resource_exhausted" && index + 1 < plans.len() => {
                let next = plans[index + 1];
                tracing::warn!(
                    detail = %error,
                    stage = plan.stage(),
                    timing = ?plan.timing,
                    raw = plan.raw,
                    autoconvert,
                    next_timing = ?next.timing,
                    next_raw = next.raw,
                    "WASAPI initialization ran out of resources; trying a fallback mode"
                );
                previous_oom = Some(error);
            }
            Err(error) => return Err(error),
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
    let client = device
        .get_iaudioclient()
        .map_err(|error| err(error).at_stage("activate_client"))?;
    let mix_format = client
        .get_mixformat()
        .map_err(|error| err(error).at_stage("read_mix_format"))?;
    let wave_format = client
        .is_supported(&mix_format, &ShareMode::Shared)
        .map_err(|error| err(error).at_stage("check_format"))?
        .unwrap_or(mix_format);
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
            let client = device
                .get_iaudioclient()
                .map_err(|error| err(error).at_stage("activate_client"))?;
            let wave_format =
                WaveFormat::new(32, 32, &SampleType::Float, output_rate as usize, 1, None);
            let (client, timing) = initialize_shared_client(
                client,
                || {
                    device
                        .get_iaudioclient()
                        .map_err(|error| err(error).at_stage("activate_client"))
                },
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
            let client = AudioClient::new_application_loopback_client(*process_id, true).map_err(
                |error| map_capture_error(error, true).at_stage("activate_process_client"),
            )?;
            let device = AudioDevice {
                id: -1,
                device_key: None,
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
                    AudioClient::new_application_loopback_client(*process_id, true).map_err(
                        |error| map_capture_error(error, true).at_stage("activate_process_client"),
                    )
                },
                &wave_format,
                false,
                false,
            )
            .map_err(|error| error.with_default_code("audio.process_loopback_unavailable"))?;
            (client, device, native, timing, "process", endpoint_id, None)
        }
        CaptureTarget::Device {
            backend_id,
            direction,
        } => {
            let wasapi_direction = match direction {
                DeviceDirection::Render => Direction::Render,
                DeviceDirection::Capture => Direction::Capture,
            };
            let follows_default = backend_id.is_none();
            let device = resolve_device(com, backend_id.as_deref(), &wasapi_direction)?;
            let endpoint_id = device.get_id().map_err(err)?;
            let info = endpoint_info(
                &device,
                backend_id.is_none(),
                *direction == DeviceDirection::Render,
            )?;
            let selection = match (follows_default, direction) {
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
            let default_direction = follows_default.then_some(wasapi_direction);
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
    microphone_tap: Option<&crate::vision::MicrophoneTap>,
) -> Result<(), StreamFailure> {
    let frames_per_chunk = ((stream.native.sample_rate as u64 * CHUNK_FRAMES as u64
        + output_rate as u64 / 2)
        / output_rate as u64)
        .max(1) as usize;
    let mut pending = VecDeque::<f32>::new();
    let mut packet = VecDeque::<u8>::new();
    let mut timeline_origin = None;
    let mut timeline_frames = 0u64;
    let mut tap_resampler = TapResampler::new(stream.native.sample_rate);
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
            // This WASAPI wrapper does not expose device/QPC timestamps or packet
            // discontinuity flags. Anchor the first packet at producer read time,
            // then advance by native frame count. Scheduling latency affects the
            // absolute boundary accuracy; pipeline queue latency does not. A read
            // stall larger than 250 ms invalidates the window instead of guessing.
            let read_at = std::time::Instant::now();
            let native_duration = |frames: u64| {
                std::time::Duration::from_secs_f64(frames as f64 / stream.native.sample_rate as f64)
            };
            let mut origin = *timeline_origin
                .get_or_insert_with(|| read_at - native_duration(pending.len() as u64));
            let estimated_end = origin + native_duration(timeline_frames + pending.len() as u64);
            if let Some(tap) = microphone_tap {
                if read_at.saturating_duration_since(estimated_end)
                    > std::time::Duration::from_millis(250)
                    || estimated_end.saturating_duration_since(read_at)
                        > std::time::Duration::from_millis(250)
                {
                    tap.discontinuity("Microphone capture timing discontinuity");
                    tap_resampler.reset();
                    origin = read_at - native_duration(pending.len() as u64);
                    timeline_origin = Some(origin);
                    timeline_frames = 0;
                }
            }
            while pending.len() >= frames_per_chunk {
                let chunk_start = origin + native_duration(timeline_frames);
                timeline_frames += frames_per_chunk as u64;
                let chunk_end = origin + native_duration(timeline_frames);
                let frame: Vec<f32> = pending.drain(..frames_per_chunk).collect();
                publish_chunk(
                    &frame,
                    stream.native.sample_rate,
                    output_rate,
                    chunk_start,
                    chunk_end,
                    tx,
                    microphone_tap,
                    &mut tap_resampler,
                );
            }
        }
    }
    Ok(())
}

fn publish_chunk(
    frame: &[f32],
    native_rate: u32,
    output_rate: u32,
    start: std::time::Instant,
    end: std::time::Instant,
    tx: &mpsc::Sender<Vec<f32>>,
    microphone_tap: Option<&crate::vision::MicrophoneTap>,
    tap_resampler: &mut TapResampler,
) {
    let chunk = resample_linear(frame, output_rate, native_rate);
    if let Some(tap) = microphone_tap {
        let resampled;
        let samples: &[f32] = if output_rate == TAP_RATE {
            frame
        } else {
            resampled = tap_resampler.resample(frame);
            &resampled
        };
        tap.push(samples, start, end);
    }
    if !chunk.is_empty() {
        if let Ok(permit) = tx.try_reserve() {
            permit.send(chunk);
        }
    }
}

pub(crate) fn capture_main(
    target: CaptureTarget,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
    microphone_tap: Option<crate::vision::MicrophoneTap>,
) {
    // This guard also clears running state if the producer unwinds.
    struct StopTap<'a>(Option<&'a crate::vision::MicrophoneTap>);
    impl Drop for StopTap<'_> {
        fn drop(&mut self) {
            if let Some(tap) = self.0 {
                tap.stopped("Microphone capture stopped");
            }
        }
    }
    let _tap_lifetime = StopTap(microphone_tap.as_ref());
    let follows_default = matches!(
        &target,
        CaptureTarget::Device {
            backend_id: None,
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
    if let Some(tap) = &microphone_tap {
        tap.started();
    }
    let _ = ready.send(Ok(stream.device.clone()));

    loop {
        let result = run_stream(
            &stream,
            output_rate,
            &stop,
            &tx,
            process_loopback,
            microphone_tap.as_ref(),
        );
        if let Some(tap) = &microphone_tap {
            tap.stopped("Microphone capture stopped or device changed");
        }
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
                let mut retry_delay = DEFAULT_RECOVERY_DELAY;
                stream = loop {
                    let retry_at = std::time::Instant::now() + retry_delay;
                    loop {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        let remaining =
                            retry_at.saturating_duration_since(std::time::Instant::now());
                        if remaining.is_zero() {
                            break;
                        }
                        std::thread::sleep(remaining.min(DEFAULT_RECOVERY_DELAY));
                    }
                    match open_stream(&_com, &target, output_rate) {
                        Ok(replacement) => {
                            tracing::info!("default audio capture recovered");
                            break replacement;
                        }
                        Err(error) if error.is_retryable() => {
                            if error.code() == "audio.resource_exhausted" {
                                retry_delay = (retry_delay * 2).min(MAX_RECOVERY_DELAY);
                                tracing::warn!(
                                    detail = %error,
                                    retry_delay_ms = retry_delay.as_millis() as u64,
                                    "default audio capture ran out of resources; backing off"
                                );
                            } else {
                                retry_delay = DEFAULT_RECOVERY_DELAY;
                                tracing::debug!(
                                    detail = %error,
                                    "default audio endpoint is not ready; retrying"
                                );
                            }
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
                if let Some(tap) = &microphone_tap {
                    tap.started();
                }
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
        default_endpoint_changed, should_recover_default, CaptureTiming, INITIALIZE_PLANS,
    };

    #[tokio::test]
    async fn microphone_tap_is_independent_of_full_or_closed_ordinary_queue() {
        for output_rate in [16_000, 48_000] {
            for closed in [false, true] {
                let (tx, mut rx) = tokio::sync::mpsc::channel(1);
                tx.try_send(vec![9.0]).unwrap();
                if closed {
                    rx.close();
                }
                let tap = crate::vision::MicrophoneTap::default();
                tap.started();
                let mut tap_resampler = super::TapResampler::new(48_000);
                let start = std::time::Instant::now();
                let mut window = tap.begin(1).unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                let middle = std::time::Instant::now();
                super::publish_chunk(
                    &[0.1; 96],
                    48_000,
                    output_rate,
                    start,
                    middle,
                    &tx,
                    Some(&tap),
                    &mut tap_resampler,
                );
                window.reader().check().unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(2)).await;
                window.release();
                super::publish_chunk(
                    &[0.2; 96],
                    48_000,
                    output_rate,
                    middle,
                    std::time::Instant::now(),
                    &tx,
                    Some(&tap),
                    &mut tap_resampler,
                );
                let samples = window.finish().await.unwrap();
                assert!(samples.contains(&0.1));
                assert!(samples.contains(&0.2));
                assert!(samples
                    .iter()
                    .all(|sample| *sample == 0.1 || *sample == 0.2));
                assert_eq!(rx.try_recv().unwrap(), vec![9.0]);
                assert!(rx.try_recv().is_err());
            }
        }
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
