use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ::pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::buffer::meta::{MetaHeader, MetaHeaderFlags};
use spa::param::audio::{AudioFormat, AudioInfoRaw};
use spa::pod::Pod;
use tokio::sync::mpsc;

use super::super::tap::{TAP_RATE, TapResampler};
use super::super::{AudioError, CHUNK_FRAMES};
use super::devices::{Node, Session, error};
use super::{CaptureTarget, DeviceDirection};
use crate::models::AudioDevice;
use crate::vision::MicrophoneTap;

const QUEUED_CHUNKS: usize = 16;
const WAKE_INTERVAL: Duration = Duration::from_millis(50);
const RECOVERY_DELAY: Duration = Duration::from_millis(150);
const MAX_RECOVERY_DELAY: Duration = Duration::from_secs(5);
const EXPLICIT_RECONNECT_WINDOW: Duration = Duration::from_millis(1800);
const EXPLICIT_RETRY_DELAY: Duration = Duration::from_millis(250);
const HEALTHY_RESET_AGE: Duration = Duration::from_secs(5);
const START_TIMEOUT: Duration = Duration::from_secs(4);
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(500);
const MAX_TIMING_GAP: Duration = Duration::from_millis(250);
const PW_ID_ANY: u32 = u32::MAX;

struct Packet {
    samples: [f32; CHUNK_FRAMES],
    start: Instant,
    end: Instant,
    discontinuity: bool,
}

struct CaptureData {
    rate: u32,
    channels: u32,
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
    mono_scratch: Vec<u8>,
}

impl CaptureData {
    fn new(rate: u32, channels: u32) -> Self {
        Self {
            rate,
            channels,
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
            mono_scratch: Vec::new(),
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
        if self.channels == 2 {
            let mut scratch = std::mem::take(&mut self.mono_scratch);
            downmix_stereo_to_mono(bytes, &mut scratch);
            self.append_mono(&scratch, now);
            self.mono_scratch = scratch;
        } else {
            self.append_mono(bytes, now);
        }
    }

    fn append_mono(&mut self, sample_bytes: &[u8], now: Instant) {
        let buffer_frames = sample_bytes.len() / 4;
        let duration = self.duration(buffer_frames as u64);
        if let Some(origin) = self.origin {
            let end = origin
                + self.duration(self.frames + self.pending_len as u64 + buffer_frames as u64);
            if now.saturating_duration_since(end) > MAX_TIMING_GAP
                || end.saturating_duration_since(now) > MAX_TIMING_GAP
            {
                self.reset_timeline();
            }
        }
        let origin = *self
            .origin
            .get_or_insert_with(|| now.checked_sub(duration).unwrap_or(now));
        for bytes in sample_bytes.as_chunks::<4>().0 {
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
                    self.queue.push_back(Packet {
                        samples,
                        start,
                        end,
                        discontinuity: std::mem::take(&mut self.discontinuity),
                    });
                }
                self.pending_len = 0;
            }
        }
    }
}

fn downmix_stereo_to_mono(stereo_bytes: &[u8], out: &mut Vec<u8>) {
    out.clear();
    out.reserve(stereo_bytes.len() / 2);
    for frame in stereo_bytes.as_chunks::<8>().0 {
        let left = f32::from_le_bytes([frame[0], frame[1], frame[2], frame[3]]);
        let right = f32::from_le_bytes([frame[4], frame[5], frame[6], frame[7]]);
        out.extend_from_slice(&((left + right) * 0.5).to_le_bytes());
    }
}

fn format_bytes(rate: u32, channels: u32) -> Result<Vec<u8>, AudioError> {
    let mut info = AudioInfoRaw::new();
    info.set_format(AudioFormat::F32LE);
    info.set_channels(channels);
    info.set_rate(rate);
    let mut position = info.position();
    if channels == 2 {
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_FL;
        position[1] = spa::sys::SPA_AUDIO_CHANNEL_FR;
    } else {
        position[0] = spa::sys::SPA_AUDIO_CHANNEL_MONO;
    }
    info.set_position(position);
    let object = spa::pod::Object {
        type_: spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
        id: spa::param::ParamType::EnumFormat.as_raw(),
        properties: info.into(),
    };
    spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(object),
    )
    .map(|(cursor, _)| cursor.into_inner())
    .map_err(|detail| {
        error(format!(
            "Cannot serialize PipeWire audio format: {detail:?}"
        ))
    })
}

type StreamConnection = (
    pw::stream::StreamRc,
    pw::stream::StreamListener<Rc<RefCell<CaptureData>>>,
);

fn build_capture_stream(
    session: &Session,
    props: pw::properties::PropertiesBox,
    rate: u32,
    channels: u32,
    flags: pw::stream::StreamFlags,
    state: Rc<RefCell<CaptureData>>,
) -> Result<StreamConnection, AudioError> {
    let stream = pw::stream::StreamRc::new(session.core.clone(), "VRCS audio capture", props)
        .map_err(error)?;
    let listener = stream
        .add_local_listener_with_user_data(state)
        .state_changed(|_, shared, old, new| {
            let mut state = shared.borrow_mut();
            state.streaming = new == pw::stream::StreamState::Streaming;
            match new {
                pw::stream::StreamState::Error(detail) => {
                    state.failure = Some(AudioError::retryable_with_code(
                        "audio.pipewire_failed",
                        format!("PipeWire capture stream: {detail}"),
                    ));
                }
                pw::stream::StreamState::Unconnected
                    if old != pw::stream::StreamState::Unconnected =>
                {
                    state.failure = Some(AudioError::with_code(
                        "audio.device_unavailable",
                        "PipeWire capture stream disconnected",
                    ));
                }
                _ => {}
            }
        })
        .param_changed(|_, shared, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let mut state = shared.borrow_mut();
            state.format_ready = false;
            state.reset_timeline();
            let Some(param) = param else {
                return;
            };
            let mut format = AudioInfoRaw::new();
            let media = spa::param::format_utils::parse_format(param);
            if !matches!(
                media,
                Ok((
                    spa::param::format::MediaType::Audio,
                    spa::param::format::MediaSubtype::Raw
                ))
            ) || format.parse(param).is_err()
                || format.format() != AudioFormat::F32LE
                || format.channels() != state.channels
                || format.rate() != state.rate
            {
                state.failure = Some(AudioError::with_code(
                    "audio.unsupported_format",
                    "PipeWire did not negotiate the requested F32 capture format and rate",
                ));
                return;
            }
            // PipeWire's adapter resamples 44.1/48 kHz graph audio to this rate
            // and downmixes before delivery; NO_CONVERT is intentionally absent.
            state.format_ready = true;
        })
        .process(|stream, shared| {
            let mut state = shared.borrow_mut();
            if !state.format_ready {
                while stream.dequeue_buffer().is_some() {}
                return;
            }
            let frame_bytes = (state.channels as usize) * 4;
            // This callback is NOT RT_PROCESS. Only bounded PCM copying happens
            // here: no channel sends, logging, tap mutexes, or inference/IO.
            for _ in 0..QUEUED_CHUNKS {
                let Some(mut buffer) = stream.dequeue_buffer() else {
                    break;
                };
                if let Some(header) = buffer.find_meta::<MetaHeader>() {
                    if header.flags().intersects(
                        MetaHeaderFlags::DISCONT
                            | MetaHeaderFlags::CORRUPTED
                            | MetaHeaderFlags::GAP,
                    ) {
                        state.reset_timeline();
                        if header
                            .flags()
                            .intersects(MetaHeaderFlags::CORRUPTED | MetaHeaderFlags::GAP)
                        {
                            continue;
                        }
                    }
                }
                let Some(data) = buffer.datas_mut().first_mut() else {
                    continue;
                };
                if data.as_raw().chunk.is_null() {
                    state.failure =
                        Some(error("PipeWire supplied an audio buffer without a chunk"));
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
                if size == 0 {
                    continue;
                }
                if (stride != 0 && stride != frame_bytes as i32)
                    || !size.is_multiple_of(frame_bytes)
                {
                    state.failure = Some(AudioError::with_code(
                        "audio.unsupported_format",
                        "Invalid F32 PipeWire audio stride or size",
                    ));
                    break;
                }
                let Some(bytes) = data.data() else {
                    state.failure = Some(error("PipeWire audio buffer is not mapped"));
                    break;
                };
                // Chunk offset is modulo maxsize. A wrapped chunk is split on a
                // frame boundary; never read beyond the mapped buffer.
                if bytes.is_empty()
                    || size > bytes.len()
                    || !offset.is_multiple_of(frame_bytes)
                    || !bytes.len().is_multiple_of(frame_bytes)
                {
                    state.failure =
                        Some(error("PipeWire audio chunk is outside its mapped buffer"));
                    break;
                }
                let offset = offset % bytes.len();
                let first = size.min(bytes.len() - offset);
                let now = Instant::now();
                if first == size {
                    state.append(&bytes[offset..offset + size], now);
                } else {
                    let remaining_frames = (size - first) / frame_bytes;
                    let remaining_duration = state.duration(remaining_frames as u64);
                    state.append(
                        &bytes[offset..],
                        now.checked_sub(remaining_duration).unwrap_or(now),
                    );
                    state.append(&bytes[..size - first], now);
                }
            }
        })
        .register()
        .map_err(error)?;
    let bytes = format_bytes(rate, channels)?;
    let pod = Pod::from_bytes(&bytes).ok_or_else(|| error("Invalid PipeWire audio format pod"))?;
    stream
        .connect(spa::utils::Direction::Input, None, flags, &mut [pod])
        .map_err(error)?;
    Ok((stream, listener))
}

fn connect_device_stream(
    session: &Session,
    node: &Node,
    rate: u32,
    state: Rc<RefCell<CaptureData>>,
) -> Result<StreamConnection, AudioError> {
    let target_object = node.target_object();
    let latency = format!("{CHUNK_FRAMES}/{rate}");
    let sink = if node.direction == DeviceDirection::Render {
        "true"
    } else {
        "false"
    };
    let props = if node.direction == DeviceDirection::Capture {
        properties! {
            "media.type" => "Audio",
            "media.category" => "Capture",
            "media.role" => "Communication",
            "application.name" => "VRCS",
            "target.object" => target_object.as_str(),
            "stream.capture.sink" => sink,
            "node.dont-reconnect" => "true",
            "node.dont-move" => "true",
            "node.latency" => latency.as_str(),
        }
    } else {
        properties! {
            "media.type" => "Audio",
            "media.category" => "Capture",
            "application.name" => "VRCS",
            "target.object" => target_object.as_str(),
            "stream.capture.sink" => sink,
            "node.dont-reconnect" => "true",
            "node.dont-move" => "true",
            "node.latency" => latency.as_str(),
        }
    };
    build_capture_stream(
        session,
        props,
        rate,
        1,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::DONT_RECONNECT,
        state,
    )
}

fn connect_process_stream(
    session: &Session,
    rate: u32,
    state: Rc<RefCell<CaptureData>>,
) -> Result<StreamConnection, AudioError> {
    let latency = format!("{CHUNK_FRAMES}/{rate}");
    let props = properties! {
        "media.type" => "Audio",
        "media.category" => "Capture",
        "application.name" => "VRCS",
        "node.autoconnect" => "false",
        "node.dont-reconnect" => "true",
        "node.dont-move" => "true",
        "node.passive" => "true",
        "node.latency" => latency.as_str(),
    };
    build_capture_stream(
        session,
        props,
        rate,
        2,
        pw::stream::StreamFlags::MAP_BUFFERS | pw::stream::StreamFlags::DONT_RECONNECT,
        state,
    )
}

fn proc_ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rparen = stat.rfind(')')?;
    let mut fields = stat.get(rparen + 2..)?.split_whitespace();
    let _state = fields.next()?;
    fields.next()?.parse().ok()
}

fn pid_matches_target(
    candidate: u32,
    target: u32,
    parent_cache: &mut HashMap<u32, Option<u32>>,
) -> bool {
    let mut current = candidate;
    for _ in 0..8 {
        if current == target {
            return true;
        }
        if current <= 1 {
            return false;
        }
        let parent = *parent_cache
            .entry(current)
            .or_insert_with(|| proc_ppid(current));
        let Some(next) = parent else {
            return false;
        };
        if next == current {
            return false;
        }
        current = next;
    }
    false
}

fn reconcile_process_links(
    session: &Session,
    target_pid: u32,
    our_node_id: u32,
    active_links: &mut HashMap<(u32, u32), pw::link::Link>,
    parent_cache: &mut HashMap<u32, Option<u32>>,
) {
    if our_node_id == PW_ID_ANY {
        return;
    }
    let desired: HashMap<(u32, u32), u32> = {
        let inventory = session.inventory.borrow();
        let mut in_ports: Vec<_> = inventory
            .ports
            .values()
            .filter(|port| port.node_id == our_node_id && port.direction == "in")
            .collect();
        in_ports.sort_by_key(|port| port.id);
        let Some(in_fl) = in_ports
            .iter()
            .find(|port| port.channel.as_deref() == Some("FL"))
            .or_else(|| in_ports.first())
            .map(|port| port.id)
        else {
            return;
        };
        let in_fr = in_ports
            .iter()
            .find(|port| port.channel.as_deref() == Some("FR") && port.id != in_fl)
            .or_else(|| in_ports.iter().find(|port| port.id != in_fl))
            .map(|port| port.id);

        let mut map = HashMap::new();
        for stream_node in inventory.app_streams.values() {
            let client_pid = stream_node
                .client_id
                .and_then(|cid| inventory.clients.get(&cid).copied());
            if ![stream_node.pid, client_pid]
                .into_iter()
                .flatten()
                .any(|pid| pid_matches_target(pid, target_pid, parent_cache))
            {
                continue;
            }
            let mut out_ports: Vec<_> = inventory
                .ports
                .values()
                .filter(|port| port.node_id == stream_node.id && port.direction == "out")
                .collect();
            out_ports.sort_by_key(|port| port.id);
            if out_ports.is_empty() {
                continue;
            }
            if out_ports.len() == 1 {
                let out_id = out_ports[0].id;
                map.insert((out_id, in_fl), stream_node.id);
                if let Some(in_fr) = in_fr {
                    map.insert((out_id, in_fr), stream_node.id);
                }
            } else {
                let left = out_ports
                    .iter()
                    .find(|port| port.channel.as_deref() == Some("FL"))
                    .unwrap_or(&out_ports[0]);
                map.insert((left.id, in_fl), stream_node.id);
                if let Some(in_fr) = in_fr {
                    let right = out_ports
                        .iter()
                        .find(|port| port.channel.as_deref() == Some("FR") && port.id != left.id)
                        .or_else(|| out_ports.iter().find(|port| port.id != left.id));
                    if let Some(right) = right {
                        map.insert((right.id, in_fr), stream_node.id);
                    }
                }
            }
        }
        map
    };

    let stale_keys: Vec<_> = active_links
        .keys()
        .filter(|pair| !desired.contains_key(pair))
        .copied()
        .collect();
    for pair in stale_keys {
        if let Some(link) = active_links.remove(&pair) {
            let _ = session.core.destroy_object(link);
        }
    }

    for ((out_port, in_port), out_node) in desired {
        if active_links.contains_key(&(out_port, in_port)) {
            continue;
        }
        let out_port_s = out_port.to_string();
        let in_port_s = in_port.to_string();
        let out_node_s = out_node.to_string();
        let in_node_s = our_node_id.to_string();
        match session.core.create_object::<pw::link::Link>(
            "link-factory",
            &properties! {
                "link.output.port" => out_port_s.as_str(),
                "link.input.port" => in_port_s.as_str(),
                "link.output.node" => out_node_s.as_str(),
                "link.input.node" => in_node_s.as_str(),
                "object.linger" => "false",
            },
        ) {
            Ok(link) => {
                active_links.insert((out_port, in_port), link);
            }
            Err(detail) => {
                tracing::debug!(out_port, in_port, %detail, "PipeWire process link creation failed");
            }
        }
    }
}

fn process_audio_device(pid: u32, rate: u32) -> AudioDevice {
    let key = format!("pipewire:process:{pid}");
    AudioDevice {
        id: crate::audio::device_id(&key),
        device_key: Some(key),
        name: "VRChat".into(),
        is_default: false,
        is_loopback: true,
        sample_rate: rate,
        channels: 1,
    }
}

fn publish(state: &mut CaptureData, tx: &mpsc::Sender<Vec<f32>>, tap: Option<&MicrophoneTap>) {
    let dropped = std::mem::take(&mut state.dropped);
    if dropped != 0 {
        tracing::warn!(
            dropped_chunks = dropped,
            "PipeWire bounded capture queue overflowed"
        );
    }
    let mut channel_dropped = 0u64;
    while let Some(packet) = state.queue.pop_front() {
        if packet.discontinuity {
            state.tap.reset();
            if let Some(tap) = tap {
                tap.discontinuity("PipeWire audio was interrupted or dropped");
            }
        }
        if let Some(tap) = tap {
            if state.rate == TAP_RATE {
                tap.push(&packet.samples, packet.start, packet.end);
            } else {
                let resampled = state.tap.resample(&packet.samples);
                if !resampled.is_empty() {
                    tap.push(&resampled, packet.start, packet.end);
                }
            }
        }
        match tx.try_send(packet.samples.to_vec()) {
            Ok(()) => {}
            Err(mpsc::error::TrySendError::Full(_)) => {
                channel_dropped += 1;
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                state.queue.clear();
                break;
            }
        }
    }
    if channel_dropped != 0 {
        tracing::warn!(
            dropped_chunks = channel_dropped,
            "PipeWire PCM consumer queue overflowed"
        );
    }
}

fn run_process(
    target_pid: u32,
    output_rate: u32,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Vec<f32>>,
    ready: &std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
    first_ready: &mut bool,
) -> Result<(), AudioError> {
    if !Path::new(&format!("/proc/{target_pid}")).exists() {
        return Err(AudioError::with_code(
            "audio.vrchat_not_running",
            "VRChat is not running",
        ));
    }
    let session = Session::connect(Some(stop), false)?;
    let timer = session.main_loop.loop_().add_timer(|_| {});
    timer
        .update_timer(Some(WAKE_INTERVAL), Some(WAKE_INTERVAL))
        .into_sync_result()
        .map_err(error)?;

    let state = Rc::new(RefCell::new(CaptureData::new(output_rate, 2)));
    let (stream, _listener) = connect_process_stream(&session, output_rate, state.clone())?;
    let mut active_links: HashMap<(u32, u32), pw::link::Link> = HashMap::new();
    let mut parent_cache = HashMap::new();
    let start_deadline = Instant::now() + START_TIMEOUT;
    let mut last_proc_check = Instant::now();
    let mut started = false;
    let mut reconciled_once = false;

    let result = loop {
        if stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        session.pump(pw::loop_::Timeout::Finite(WAKE_INTERVAL))?;
        if last_proc_check.elapsed() >= PROCESS_POLL_INTERVAL {
            last_proc_check = Instant::now();
            if !Path::new(&format!("/proc/{target_pid}")).exists() {
                break Err(AudioError::with_code(
                    "audio.vrchat_not_running",
                    "Target VRChat process exited",
                ));
            }
        }
        let our_node_id = stream.node_id();
        let our_ports_ready = {
            let mut inventory = session.inventory.borrow_mut();
            let dirty = std::mem::take(&mut inventory.graph_dirty);
            let ports_ready = our_node_id != PW_ID_ANY
                && inventory
                    .ports
                    .values()
                    .any(|p| p.node_id == our_node_id && p.direction == "in");
            (dirty, ports_ready)
        };
        if our_ports_ready.1 && (our_ports_ready.0 || !reconciled_once) {
            reconciled_once = true;
            reconcile_process_links(
                &session,
                target_pid,
                our_node_id,
                &mut active_links,
                &mut parent_cache,
            );
        }

        let mut data = state.borrow_mut();
        if let Some(failure) = data.failure.take() {
            break Err(failure);
        }
        if !started && data.format_ready && data.streaming {
            started = true;
            if !*first_ready {
                ready
                    .send(Ok(process_audio_device(target_pid, output_rate)))
                    .map_err(|_| error("Capture startup receiver was closed"))?;
                *first_ready = true;
            }
        } else if !*first_ready && our_ports_ready.1 && active_links.is_empty() {
            // Target app has no open PipeWire output stream yet; publish readiness
            // so capture stays alive until it opens one.
            ready
                .send(Ok(process_audio_device(target_pid, output_rate)))
                .map_err(|_| error("Capture startup receiver was closed"))?;
            *first_ready = true;
        }
        if !*first_ready && Instant::now() >= start_deadline {
            break Err(AudioError::retryable_with_code(
                "audio.start_timeout",
                "PipeWire process capture stream did not initialize in time",
            ));
        }
        if data.format_ready && data.streaming {
            publish(&mut data, tx, None);
        }
        if tx.is_closed() {
            break Ok(());
        }
    };

    for (_, link) in active_links {
        let _ = session.core.destroy_object(link);
    }
    result
}

fn run(
    target: &CaptureTarget,
    output_rate: u32,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Vec<f32>>,
    ready: &std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
    tap: Option<&MicrophoneTap>,
    first_ready: &mut bool,
) -> Result<(), AudioError> {
    if output_rate == 0 || output_rate > 384_000 {
        return Err(AudioError::with_code(
            "audio.unsupported_format",
            "Invalid PipeWire capture sample rate",
        ));
    }
    let (key, direction) = match target {
        CaptureTarget::Process(pid) => {
            return run_process(*pid, output_rate, stop, tx, ready, first_ready);
        }
        CaptureTarget::Device {
            backend_id,
            direction,
        } => (backend_id, *direction),
    };
    if key
        .as_deref()
        .is_some_and(|key| !key.starts_with("pipewire:"))
    {
        return Err(AudioError::with_code(
            "audio.device_reselection_required",
            "Selected device key is not a Linux PipeWire identity; select it again",
        ));
    }
    let session = Session::connect(Some(stop), false)?;
    // Timer wakes even an idle/no-signal graph so stop never waits for audio.
    let timer = session.main_loop.loop_().add_timer(|_| {});
    timer
        .update_timer(Some(WAKE_INTERVAL), Some(WAKE_INTERVAL))
        .into_sync_result()
        .map_err(error)?;
    'select: loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        let selected = session
            .inventory
            .borrow()
            .resolve(key.as_deref(), direction)?;
        let state = Rc::new(RefCell::new(CaptureData::new(output_rate, 1)));
        let (stream, _listener) =
            connect_device_stream(&session, &selected, output_rate, state.clone())?;
        let deadline = Instant::now() + START_TIMEOUT;
        let mut started = false;
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(());
            }
            session.pump(pw::loop_::Timeout::Finite(WAKE_INTERVAL))?;
            // Resolve before publishing: if a selected node vanished, no queued
            // audio from a silently substituted endpoint can enter the pipeline.
            let current = session
                .inventory
                .borrow()
                .resolve(key.as_deref(), direction)?;
            if current.id != selected.id {
                if let Some(tap) = tap {
                    tap.discontinuity("PipeWire default device changed");
                }
                let _ = stream.disconnect();
                continue 'select;
            }
            let mut state = state.borrow_mut();
            if let Some(failure) = state.failure.take() {
                return Err(failure);
            }
            if !started && state.format_ready && state.streaming {
                started = true;
                if let Some(tap) = tap {
                    tap.started();
                }
                if !*first_ready {
                    let mut device = selected.device(key.is_none());
                    device.sample_rate = output_rate;
                    device.channels = 1;
                    ready
                        .send(Ok(device))
                        .map_err(|_| error("Capture startup receiver was closed"))?;
                    *first_ready = true;
                }
            }
            if !started && Instant::now() >= deadline {
                return Err(AudioError::retryable_with_code(
                    "audio.start_timeout",
                    "PipeWire capture did not negotiate a format and enter Streaming state",
                ));
            }
            if started {
                publish(&mut state, tx, tap);
            }
            if tx.is_closed() && tap.is_none() {
                return Ok(());
            }
        }
    }
}

pub(crate) fn capture_main(
    target: CaptureTarget,
    output_rate: u32,
    stop: Arc<AtomicBool>,
    tx: mpsc::Sender<Vec<f32>>,
    ready: std::sync::mpsc::Sender<Result<AudioDevice, AudioError>>,
    microphone_tap: Option<MicrophoneTap>,
) {
    struct StopTap<'a>(Option<&'a MicrophoneTap>);
    impl Drop for StopTap<'_> {
        fn drop(&mut self) {
            if let Some(tap) = self.0 {
                tap.stopped("PipeWire microphone capture stopped");
            }
        }
    }
    let _lifetime = StopTap(microphone_tap.as_ref());
    let follows_default = matches!(
        &target,
        CaptureTarget::Device {
            backend_id: None,
            ..
        }
    );
    let process_pid = match target {
        CaptureTarget::Process(pid) => Some(pid),
        CaptureTarget::Device { .. } => None,
    };
    let mut first_ready = false;
    let mut backoff = RECOVERY_DELAY;
    let mut explicit_reconnect_started: Option<Instant> = None;

    loop {
        let attempt_started = Instant::now();
        let failure = match run(
            &target,
            output_rate,
            &stop,
            &tx,
            &ready,
            microphone_tap.as_ref(),
            &mut first_ready,
        ) {
            Ok(()) => return,
            Err(failure) => failure,
        };
        if stop.load(Ordering::Relaxed) {
            return;
        }
        if attempt_started.elapsed() >= HEALTHY_RESET_AGE {
            backoff = RECOVERY_DELAY;
            explicit_reconnect_started = None;
        }
        let is_transient = failure.is_retryable()
            || matches!(
                failure.code(),
                "audio.device_unavailable"
                    | "audio.service_not_running"
                    | "audio.service_disconnected"
                    | "audio.service_timeout"
                    | "audio.start_timeout"
            );
        let delay = if let Some(pid) = process_pid {
            if first_ready && is_transient && Path::new(&format!("/proc/{pid}")).exists() {
                let d = backoff;
                backoff = (backoff * 2).min(MAX_RECOVERY_DELAY);
                Some(d)
            } else {
                None
            }
        } else if follows_default && first_ready && is_transient {
            let d = backoff;
            backoff = (backoff * 2).min(MAX_RECOVERY_DELAY);
            Some(d)
        } else if !follows_default
            && first_ready
            && (failure.code() == "audio.device_unavailable" || failure.is_retryable())
        {
            let window_start = *explicit_reconnect_started.get_or_insert_with(Instant::now);
            if window_start.elapsed() < EXPLICIT_RECONNECT_WINDOW {
                Some(EXPLICIT_RETRY_DELAY)
            } else {
                None
            }
        } else {
            None
        };

        if let Some(delay) = delay {
            tracing::debug!(
                code = failure.code(),
                detail = %failure,
                retry_ms = delay.as_millis(),
                "rebuilding PipeWire audio capture"
            );
            if let Some(tap) = &microphone_tap {
                tap.stopped("PipeWire audio capture interrupted");
            }
            let retry_at = Instant::now() + delay;
            while Instant::now() < retry_at {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                std::thread::sleep(
                    WAKE_INTERVAL.min(retry_at.saturating_duration_since(Instant::now())),
                );
            }
            continue;
        }

        tracing::error!(code = failure.code(), detail = %failure, "PipeWire capture stopped");
        if let Some(tap) = &microphone_tap {
            tap.failed(&failure.to_string());
        }
        let _ = ready.send(Err(failure));
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packets_remain_512_frames_at_the_negotiated_rate_and_overflow_is_bounded() {
        let mut state = CaptureData::new(16_000, 1);
        let bytes: Vec<_> = (0..CHUNK_FRAMES)
            .flat_map(|_| 0.25f32.to_le_bytes())
            .collect();
        let start = Instant::now();
        for index in 0..QUEUED_CHUNKS + 3 {
            state.append(
                &bytes,
                start + Duration::from_millis((index as u64 + 1) * 32),
            );
        }
        assert_eq!(state.queue.len(), QUEUED_CHUNKS);
        assert_eq!(state.dropped, 3);
        assert!(state.discontinuity);
        let first = state.queue.pop_front().unwrap();
        assert_eq!(first.samples.len(), 512);
        assert!(first.samples.iter().all(|sample| *sample == 0.25));
        assert_eq!(
            first.end.duration_since(first.start),
            Duration::from_millis(32)
        );
        assert_eq!(first.end, state.queue.front().unwrap().start);
    }

    #[test]
    fn timing_gap_discards_partial_pcm_and_marks_microphone_discontinuity() {
        let mut state = CaptureData::new(16_000, 1);
        let bytes: Vec<_> = (0..256).flat_map(|_| 0.25f32.to_le_bytes()).collect();
        let now = Instant::now();
        state.append(&bytes, now);
        state.append(&bytes, now + Duration::from_secs(1));
        assert!(state.discontinuity);
        assert_eq!(state.pending_len, 256);
        assert!(state.queue.is_empty());
    }

    #[tokio::test]
    async fn full_pcm_queue_does_not_interrupt_tap_recording() {
        let (tx, mut rx) = mpsc::channel(1);
        tx.try_send(vec![9.0]).unwrap();
        let tap = MicrophoneTap::default();
        let mut window = tap.begin_capture(1).unwrap();
        tap.started();
        let mut state = CaptureData::new(TAP_RATE, 1);
        let bytes: Vec<_> = (0..CHUNK_FRAMES)
            .flat_map(|_| 0.25f32.to_le_bytes())
            .collect();
        let start = Instant::now();
        state.append(&bytes, start + state.duration(CHUNK_FRAMES as u64));

        publish(&mut state, &tx, Some(&tap));
        window.release();
        let samples = window.finish().await.unwrap();
        assert!(!samples.is_empty());
        assert!(samples.iter().all(|sample| *sample == 0.25));
        assert_eq!(rx.try_recv().unwrap(), vec![9.0]);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn tap_resampling_keeps_fractional_phase_across_44100_and_48000_packets() {
        for rate in [44_100, 48_000] {
            let mut state = CaptureData::new(rate, 1);
            let mut count = 0;
            let samples = [0.25; CHUNK_FRAMES];
            for _ in 0..100 {
                let chunk = state.tap.resample(&samples);
                assert!(chunk.iter().all(|sample| *sample == 0.25));
                count += chunk.len();
            }
            let expected = ((100 * CHUNK_FRAMES - 1) as u64 * u64::from(TAP_RATE) / u64::from(rate)
                + 1) as usize;
            assert_eq!(count, expected);
        }
    }

    #[test]
    fn stereo_process_capture_downmixes_to_mono() {
        let mut state = CaptureData::new(16_000, 2);
        let bytes: Vec<_> = (0..CHUNK_FRAMES)
            .flat_map(|_| [0.75f32.to_le_bytes(), 0.25f32.to_le_bytes()].concat())
            .collect();
        state.append(&bytes, Instant::now());
        let packet = state.queue.pop_front().unwrap();
        assert!(packet.samples.iter().all(|sample| *sample == 0.5));
    }
}
