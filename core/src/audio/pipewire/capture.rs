use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ::pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::buffer::meta::{MetaHeader, MetaHeaderFlags};
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::Pod;
use tokio::sync::mpsc;

use crate::models::AudioDevice;
use crate::vision::MicrophoneTap;
use super::super::tap::{TapResampler, TAP_RATE};
use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{error, Node, Session};
use super::{CaptureTarget, DeviceDirection};

const QUEUED_CHUNKS: usize = 16;
const WAKE_INTERVAL: Duration = Duration::from_millis(50);
const RECOVERY_DELAY: Duration = Duration::from_millis(150);
const START_TIMEOUT: Duration = Duration::from_secs(4);
const MAX_TIMING_GAP: Duration = Duration::from_millis(250);

struct Packet {
    samples: [f32; CHUNK_FRAMES],
    start: Instant,
    end: Instant,
}

struct CaptureData {
    rate: u32,
    format_ready: bool,
    streaming: bool,
    failure: Option<AudioError>,
    queue: VecDeque<Packet>,
    pending: [f32; CHUNK_FRAMES],
    pending_len: usize,
    origin: Option<Instant>,
    frames: u64,
    dropped: u64,
    discontinuity: bool,
    tap: TapResampler,
}

impl CaptureData {
    fn new(rate: u32) -> Self {
        Self {
            rate,
            format_ready: false,
            streaming: false,
            failure: None,
            queue: VecDeque::with_capacity(QUEUED_CHUNKS),
            pending: [0.0; CHUNK_FRAMES],
            pending_len: 0,
            origin: None,
            frames: 0,
            dropped: 0,
            discontinuity: false,
            tap: TapResampler::new(rate),
        }
    }

    fn duration(&self, frames: u64) -> Duration {
        Duration::from_nanos(frames.saturating_mul(1_000_000_000) / u64::from(self.rate))
    }

    fn reset_timeline(&mut self) {
        if self.origin.is_some() || self.pending_len != 0 || !self.queue.is_empty() {
            self.discontinuity = true;
        }
        self.pending_len = 0;
        self.origin = None;
        self.frames = 0;
    }

    fn append(&mut self, bytes: &[u8], now: Instant) {
        let buffer_frames = bytes.len() / 4;
        let duration = self.duration(buffer_frames as u64);
        if let Some(origin) = self.origin {
            let end = origin + self.duration(self.frames + self.pending_len as u64 + buffer_frames as u64);
            if now.saturating_duration_since(end) > MAX_TIMING_GAP
                || end.saturating_duration_since(now) > MAX_TIMING_GAP
            {
                self.reset_timeline();
            }
        }
        let origin = *self.origin.get_or_insert_with(|| now.checked_sub(duration).unwrap_or(now));
        for bytes in bytes.as_chunks::<4>().0 {
            self.pending[self.pending_len] = f32::from_le_bytes(*bytes);
            self.pending_len += 1;
            if self.pending_len == CHUNK_FRAMES {
                let start = origin + self.duration(self.frames);
                self.frames += CHUNK_FRAMES as u64;
                let end = origin + self.duration(self.frames);
                if self.queue.len() == QUEUED_CHUNKS {
                    self.dropped += 1;
                    self.discontinuity = true;
                } else {
                    let samples = std::mem::replace(&mut self.pending, [0.0; CHUNK_FRAMES]);
                    self.queue.push_back(Packet { samples, start, end });
                }
                self.pending_len = 0;
            }
        }
    }
}

fn format_bytes(rate: u32) -> Result<Vec<u8>, AudioError> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_channels(1);
    info.set_rate(rate);
    let mut position = info.position();
    position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(object),
    ).map(|(cursor, _)| cursor.into_inner()).map_err(|detail| error(format!("Cannot serialize PipeWire audio format: {detail:?}")))
}

type StreamConnection = (
    pw::stream::StreamRc,
    pw::stream::StreamListener<Rc<RefCell<CaptureData>>>,
);

fn connect_stream(session: &Session, node: &Node, rate: u32, state: Rc<RefCell<CaptureData>>)
    -> Result<StreamConnection, AudioError>
{
    let props = properties! {
        "media.type" => "Audio",
        "media.category" => "Capture",
        "media.role" => "Communication",
        "application.name" => "VRCS",
        "target.object" => node.name.as_str(),
        "stream.capture.sink" => if node.direction == DeviceDirection::Render { "true" } else { "false" },
        "node.dont-reconnect" => "true",
        "node.dont-move" => "true",
        "node.latency" => format!("{CHUNK_FRAMES}/{rate}"),
    };
    let stream = pw::stream::StreamRc::new(session.core.clone(), "VRCS audio capture", props).map_err(error)?;
    let listener = stream.add_local_listener_with_user_data(state)
        .state_changed(|_, shared, old, new| {
            let mut state = shared.borrow_mut();
            state.streaming = new == pw::stream::StreamState::Streaming;
            match new {
                pw::stream::StreamState::Error(detail) => {
                    state.failure = Some(AudioError::retryable_with_code("audio.pipewire_failed", format!("PipeWire capture stream: {detail}")));
                }
                pw::stream::StreamState::Unconnected if old != pw::stream::StreamState::Unconnected => {
                    state.failure = Some(AudioError::with_code("audio.device_unavailable", "PipeWire capture stream disconnected"));
                }
                _ => {}
            }
        })
        .param_changed(|_, shared, id, param| {
            if id != spa::param::ParamType::Format.as_raw() { return; }
            let mut state = shared.borrow_mut();
            state.format_ready = false;
            state.reset_timeline();
            let Some(param) = param else { return; };
            let mut format = AudioInfoRaw::new();
            let media = spa::param::format_utils::parse_format(param);
            if !matches!(media, Ok((spa::param::format::MediaType::Audio, spa::param::format::MediaSubtype::Raw)))
                || format.parse(param).is_err()
                || format.format() != AudioFormat::F32LE || format.channels() != 1 || format.rate() != state.rate
            {
                state.failure = Some(AudioError::with_code(
                    "audio.unsupported_format", "PipeWire did not negotiate the requested mono F32 capture format and rate",
                ));
                return;
            }
            // PipeWire's adapter resamples 44.1/48 kHz graph audio to this rate
            // and downmixes before delivery; NO_CONVERT is intentionally absent.
            state.format_ready = true;
        })
        .process(|stream, shared| {
            let mut state = shared.borrow_mut();
            if !state.format_ready { return; }
            // This callback is NOT RT_PROCESS. Only bounded PCM copying happens
            // here: no channel sends, logging, tap mutexes, or inference/IO.
            for _ in 0..QUEUED_CHUNKS {
                let Some(mut buffer) = stream.dequeue_buffer() else { break; };
                if let Some(header) = buffer.find_meta::<MetaHeader>() {
                    if header.flags().intersects(MetaHeaderFlags::DISCONT | MetaHeaderFlags::CORRUPTED | MetaHeaderFlags::GAP) {
                        state.reset_timeline();
                        if header.flags().intersects(MetaHeaderFlags::CORRUPTED | MetaHeaderFlags::GAP) { continue; }
                    }
                }
                let Some(data) = buffer.datas_mut().first_mut() else { continue; };
                if data.as_raw().chunk.is_null() {
                    state.failure = Some(error("PipeWire supplied an audio buffer without a chunk"));
                    break;
                }
                let chunk = data.chunk();
                let offset = chunk.offset() as usize;
                let size = chunk.size() as usize;
                let stride = chunk.stride();
                if chunk.flags().contains(spa::buffer::ChunkFlags::CORRUPTED) {
                    state.reset_timeline();
                    continue;
                }
                if size == 0 { continue; }
                if (stride != 0 && stride != 4) || !size.is_multiple_of(4) {
                    state.failure = Some(AudioError::with_code("audio.unsupported_format", "Invalid mono F32 PipeWire audio stride or size"));
                    break;
                }
                let Some(bytes) = data.data() else {
                    state.failure = Some(error("PipeWire audio buffer is not mapped"));
                    break;
                };
                // Chunk offset is modulo maxsize. A wrapped chunk is split on a
                // sample boundary; never read beyond the mapped buffer.
                if bytes.is_empty() || size > bytes.len() || !offset.is_multiple_of(4) || !bytes.len().is_multiple_of(4) {
                    state.failure = Some(error("PipeWire audio chunk is outside its mapped buffer"));
                    break;
                }
                let offset = offset % bytes.len();
                let first = size.min(bytes.len() - offset);
                let now = Instant::now();
                if first == size {
                    state.append(&bytes[offset..offset + size], now);
                } else {
                    let remaining_duration = state.duration(((size - first) / 4) as u64);
                    state.append(&bytes[offset..], now.checked_sub(remaining_duration).unwrap_or(now));
                    state.append(&bytes[..size - first], now);
                }
            }
        }).register().map_err(error)?;
    let bytes = format_bytes(rate)?;
    let pod = Pod::from_bytes(&bytes).ok_or_else(|| error("Invalid PipeWire audio format pod"))?;
    // Pin this exact resolved node for each stream. Default-device following is
    // explicit below; policy must never silently move a persisted selection.
    stream.connect(spa::utils::Direction::Input, Some(node.id),
        pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::DONT_RECONNECT,
        &mut [pod],
    ).map_err(error)?;
    Ok((stream, listener))
}

fn publish(state: &mut CaptureData, tx: &mpsc::Sender<Vec<f32>>, tap: Option<&MicrophoneTap>) {
    if std::mem::take(&mut state.discontinuity) {
        state.tap.reset();
        if let Some(tap) = tap { tap.discontinuity("PipeWire audio was interrupted or dropped"); }
    }
    let dropped = std::mem::take(&mut state.dropped);
    if dropped != 0 { tracing::warn!(dropped_chunks = dropped, "PipeWire bounded capture queue overflowed"); }
    let mut channel_dropped = 0u64;
    while let Some(packet) = state.queue.pop_front() {
        if let Some(tap) = tap {
            if state.rate == TAP_RATE {
                tap.push(&packet.samples, packet.start, packet.end);
            } else {
                tap.push(&state.tap.resample(&packet.samples), packet.start, packet.end);
            }
        }
        match tx.try_reserve() {
            Ok(permit) => permit.send(packet.samples.to_vec()),
            Err(mpsc::error::TrySendError::Full(_)) => channel_dropped += 1,
            Err(mpsc::error::TrySendError::Closed(_)) => {}
        }
    }
    if channel_dropped != 0 {
        tracing::warn!(dropped_chunks = channel_dropped, "PipeWire PCM consumer queue overflowed");
        if let Some(tap) = tap { tap.discontinuity("PipeWire PCM consumer queue overflowed"); }
    }
}

fn run(
    target: &CaptureTarget, output_rate: u32, stop: &AtomicBool,
    tx: &mpsc::Sender<Vec<f32>>, ready: &std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
    tap: Option<&MicrophoneTap>, first_ready: &mut bool,
) -> Result<(), AudioError> {
    let (key, direction) = match target {
        CaptureTarget::Process(pid) => return Err(AudioError::with_code(
            "audio.process_capture_unsupported", format!("Application-only capture ({pid}) is not supported on Linux"),
        )),
        CaptureTarget::Device { backend_id, direction } => (backend_id, *direction),
    };
    if output_rate == 0 || output_rate > 384_000 {
        return Err(AudioError::with_code("audio.unsupported_format", "Invalid PipeWire capture sample rate"));
    }
    if key.as_deref().is_some_and(|key| !key.starts_with("pipewire:")) {
        return Err(AudioError::with_code("audio.device_reselection_required", "Selected device key is not a Linux PipeWire identity; select it again"));
    }
    let session = Session::connect(Some(stop))?;
    // Timer wakes even an idle/no-signal graph so stop never waits for audio.
    let timer = session.main_loop.loop_().add_timer(|_| {});
    timer.update_timer(Some(WAKE_INTERVAL), Some(WAKE_INTERVAL)).into_sync_result().map_err(error)?;
    'select: loop {
        if stop.load(Ordering::Relaxed) { return Ok(()); }
        let selected = session.inventory.borrow().resolve(key.as_deref(), direction)?;
        let state = Rc::new(RefCell::new(CaptureData::new(output_rate)));
        let (stream, _listener) = connect_stream(&session, &selected, output_rate, state.clone())?;
        let deadline = Instant::now() + START_TIMEOUT;
        let mut started = false;
        loop {
            if stop.load(Ordering::Relaxed) { return Ok(()); }
            session.pump(pw::loop_::Timeout::Finite(WAKE_INTERVAL))?;
            // Resolve before publishing: if a selected node vanished, no queued
            // audio from a silently substituted endpoint can enter the pipeline.
            let current = session.inventory.borrow().resolve(key.as_deref(), direction)?;
            if current.id != selected.id {
                if let Some(tap) = tap { tap.discontinuity("PipeWire default device changed"); }
                let _ = stream.disconnect();
                continue 'select;
            }
            let mut state = state.borrow_mut();
            if let Some(failure) = state.failure.take() { return Err(failure); }
            if !started && state.format_ready && state.streaming {
                started = true;
                if let Some(tap) = tap { tap.started(); }
                if !*first_ready {
                    let mut device = selected.device(key.is_none());
                    device.sample_rate = output_rate;
                    device.channels = 1;
                    ready.send(Ok(device)).map_err(|_| error("Capture startup receiver was closed"))?;
                    *first_ready = true;
                }
            }
            if !started && Instant::now() >= deadline {
                return Err(AudioError::with_code("audio.start_timeout", "PipeWire capture did not negotiate a format and enter Streaming state"));
            }
            if started { publish(&mut state, tx, tap); }
            if tx.is_closed() && tap.is_none() { return Ok(()); }
        }
    }
}

pub(crate) fn capture_main(
    target: CaptureTarget, output_rate: u32, stop: Arc<AtomicBool>, tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>, microphone_tap: Option<MicrophoneTap>,
) {
    struct StopTap<'a>(Option<&'a MicrophoneTap>);
    impl Drop for StopTap<'_> {
        fn drop(&mut self) {
            if let Some(tap) = self.0 { tap.stopped("PipeWire microphone capture stopped"); }
        }
    }
    let _lifetime = StopTap(microphone_tap.as_ref());
    let follows_default = matches!(&target, CaptureTarget::Device { backend_id: None, .. });
    let mut first_ready = false;
    loop {
        let failure = match run(&target, output_rate, &stop, &tx, &ready, microphone_tap.as_ref(), &mut first_ready) {
            Ok(()) => return,
            Err(failure) => failure,
        };
        if stop.load(Ordering::Relaxed) { return; }
        // Keep the PCM channel open while rebuilding a previously working
        // default capture. Explicit selections and permanent errors still fail.
        if follows_default && first_ready && (failure.is_retryable() || matches!(failure.code(),
            "audio.device_unavailable" | "audio.service_not_running" | "audio.service_disconnected"
                | "audio.service_timeout" | "audio.start_timeout"))
        {
            tracing::debug!(code = failure.code(), detail = %failure, "rebuilding PipeWire default audio capture");
            if let Some(tap) = &microphone_tap { tap.stopped("PipeWire default audio capture interrupted"); }
            let retry_at = Instant::now() + RECOVERY_DELAY;
            while Instant::now() < retry_at {
                if stop.load(Ordering::Relaxed) { return; }
                std::thread::sleep(WAKE_INTERVAL.min(retry_at.saturating_duration_since(Instant::now())));
            }
            continue;
        }
        tracing::error!(code = failure.code(), detail = %failure, "PipeWire capture stopped");
        if let Some(tap) = &microphone_tap { tap.failed(&failure.to_string()); }
        let _ = ready.send(Err(failure));
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_remain_512_frames_at_the_negotiated_rate_and_overflow_is_bounded() {
        let mut state = CaptureData::new(16_000);
        let bytes: Vec<_> = (0..CHUNK_FRAMES).flat_map(|_| 0.25f32.to_le_bytes()).collect();
        let start = Instant::now();
        for index in 0..QUEUED_CHUNKS + 3 {
            state.append(&bytes, start + Duration::from_millis((index as u64 + 1) * 32));
        }
        assert_eq!(state.queue.len(), QUEUED_CHUNKS);
        assert_eq!(state.dropped, 3);
        assert!(state.discontinuity);
        let first = state.queue.pop_front().unwrap();
        assert_eq!(first.samples.len(), 512);
        assert!(first.samples.iter().all(|sample| *sample == 0.25));
        assert_eq!(first.end.duration_since(first.start), Duration::from_millis(32));
        assert_eq!(first.end, state.queue.front().unwrap().start);
    }

    #[test]
    fn timing_gap_discards_partial_pcm_and_marks_microphone_discontinuity() {
        let mut state = CaptureData::new(16_000);
        let bytes: Vec<_> = (0..256).flat_map(|_| 0.25f32.to_le_bytes()).collect();
        let now = Instant::now();
        state.append(&bytes, now);
        state.append(&bytes, now + Duration::from_secs(1));
        assert!(state.discontinuity);
        assert_eq!(state.pending_len, 256);
        assert!(state.queue.is_empty());
    }

    #[test]
    fn tap_resampling_keeps_fractional_phase_across_44100_and_48000_packets() {
        for rate in [44_100, 48_000] {
            let mut state = CaptureData::new(rate);
            let mut count = 0;
            let samples = [0.25; CHUNK_FRAMES];
            for _ in 0..100 {
                let chunk = state.tap.resample(&samples);
                assert!(chunk.iter().all(|sample| *sample == 0.25));
                count += chunk.len();
            }
            let expected =
                ((100 * CHUNK_FRAMES - 1) as u64 * u64::from(TAP_RATE) / u64::from(rate) + 1)
                    as usize;
            assert_eq!(count, expected);
        }
    }
}
