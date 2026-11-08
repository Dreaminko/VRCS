use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use super::buffer::JitterBuffer;
use super::cache::{self, Lookup, SessionCache, Verification};
use super::crypto::SessionKey;
use super::opus::{Decoder, Opus};
use super::protocol::{self, Parser, VoicePacket};
use super::session::SharedOutputs;
use super::windows::{self, Connection, Network};
use super::{AudioEvent, Capture, SharedStatus};
use crate::config::{PlayerAudioConfig, PlayerProximityConfig};
use crate::models::SpeakerIdentity;

pub(super) fn start(
    config: PlayerAudioConfig,
    status: SharedStatus,
    outputs: SharedOutputs,
    cache: Arc<SessionCache>,
    settings: Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<Capture, String> {
    let tx = EventSender(outputs);
    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let thread = std::thread::Builder::new()
        .name("vrcs-vrchat-network".into())
        .spawn(move || {
            let result = supervise(config, tx, &thread_stop, &status, &cache, &settings);
            let mut state = status.lock().expect("player status");
            state.active_players = 0;
            if let Err(error) = result {
                state.phase = "failed";
                state.detail = Some(error);
            } else {
                state.phase = "stopped";
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Capture {
        stop,
        thread: Some(thread),
    })
}

#[derive(Clone)]
struct EventSender(SharedOutputs);

impl EventSender {
    fn try_send(&self, event: AudioEvent) -> Result<(), ()> {
        self.0
            .lock()
            .expect("network outputs")
            .audio(event)
            .then_some(())
            .ok_or(())
    }

    fn is_closed(&self) -> bool {
        let outputs = self.0.lock().expect("network outputs");
        !outputs.voice_enabled() && !outputs.chatbox_enabled()
    }
}

#[cfg(test)]
impl From<tokio::sync::mpsc::Sender<AudioEvent>> for EventSender {
    fn from(sender: tokio::sync::mpsc::Sender<AudioEvent>) -> Self {
        Self(Arc::new(std::sync::Mutex::new(
            super::session::Outputs::with_voice(sender),
        )))
    }
}

type Game = (u32, std::path::PathBuf);

fn wait_for_game(
    stop: &AtomicBool,
    status: &SharedStatus,
    mut discover: impl FnMut() -> Result<Option<Game>, String>,
) -> Result<Option<Game>, String> {
    {
        let mut state = status.lock().expect("player status");
        state.phase = "waiting_game";
        state.active_players = 0;
        state.players.clear();
        state.key_scan = None;
        state.detail = None;
    }
    while !stop.load(Ordering::Relaxed) {
        if let Some(game) = discover()? {
            return Ok(Some(game));
        }
        wait_cancellable(stop, Duration::from_millis(250));
    }
    Ok(None)
}

fn wait_cancellable(stop: &AtomicBool, duration: Duration) {
    let until = Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) && Instant::now() < until {
        std::thread::sleep(
            until
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(25)),
        );
    }
}

fn supervise(
    config: PlayerAudioConfig,
    tx: EventSender,
    stop: &Arc<AtomicBool>,
    status: &SharedStatus,
    cache: &SessionCache,
    settings: &Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<(), String> {
    while let Some((pid, path)) = wait_for_game(stop, status, windows::game)? {
        if tx.is_closed() {
            break;
        }
        // Game installation and sockets are opened only after a process exists;
        // starting transcription itself never requires being in VRChat.
        let owner = match windows::process_identity(pid) {
            Ok(owner) => owner,
            Err(_) => {
                wait_cancellable(stop, Duration::from_millis(250));
                continue;
            }
        };
        let default_opus_path = path
            .parent()
            .ok_or("Cannot find the VRChat installation")?
            .join("VRChat_Data/Plugins/x86_64/opus.dll");
        let network = Network::open()?;
        let ports = windows::game_ports(pid)?;
        status.lock().expect("player status").phase = "waiting_world";
        run(
            owner,
            default_opus_path,
            network,
            ports,
            &config,
            tx.clone(),
            stop,
            status,
            cache,
            settings,
        )?;
        if stop.load(Ordering::Relaxed) {
            break;
        }
    }
    Ok(())
}

type Pending = (Connection, bool, Vec<u8>, Instant, Option<u64>);

// Only identity and connection metadata survive a capture restart; no PCM,
// compressed audio or incomplete Photon fragments are kept here.
pub(super) struct RoomMetadata {
    owner: cache::ProcessIdentity,
    token: u64,
    session: String,
    next_flow: u64,
    next_speaker: u64,
    flows: HashMap<Connection, Flow>,
    identities: HashMap<String, SpeakerIdentity>,
    active_world: Option<String>,
}

fn acquisition_samples(
    pending: &VecDeque<Pending>,
    now: Instant,
    last_success: Option<Instant>,
) -> Vec<Vec<u8>> {
    // A short quiet interval plus two distinct failed packets detects a changed
    // session without the old fixed five-second delay or reacting to one bad tag.
    if last_success.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_millis(350))
    {
        return Vec::new();
    }
    let Some((connection, ..)) = pending.back() else {
        return Vec::new();
    };
    let mut seen = HashSet::new();
    pending
        .iter()
        .rev()
        .filter(|p| {
            &p.0 == connection && now.saturating_duration_since(p.3) < Duration::from_secs(2)
        })
        .filter(|p| last_success.is_none_or(|at| p.3 > at) && super::crypto::encrypted(&p.2))
        .filter(|p| seen.insert(<[u8; 12]>::try_from(&p.2[7..19]).unwrap()))
        .take(8)
        .map(|p| p.2.clone())
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn run(
    owner: cache::ProcessIdentity,
    opus_path: std::path::PathBuf,
    network: Network,
    mut ports: HashSet<u16>,
    config: &PlayerAudioConfig,
    tx: EventSender,
    stop: &Arc<AtomicBool>,
    status: &SharedStatus,
    cache: &SessionCache,
    settings: &Arc<std::sync::RwLock<crate::config::AppConfig>>,
) -> Result<(), String> {
    let pid = owner.pid;
    let mut engine = Engine::new(None, config.max_players, tx, Arc::clone(status));
    let mut packet_buffer = vec![0u8; 65_535];
    let mut pending: VecDeque<Pending> = VecDeque::new();
    let mut pending_bytes = 0usize;
    let mut startup_packets: VecDeque<Pending> = VecDeque::new();
    let mut startup_bytes = 0usize;
    let mut key = None;
    let mut candidate: Option<zeroize::Zeroizing<[u8; 32]>> = None;
    let mut scanner: Option<std::thread::JoinHandle<()>> = None;
    let (key_tx, key_rx) = mpsc::sync_channel(1);
    let started = Instant::now();
    let mut refresh = started;
    let mut world_poll = started - Duration::from_secs(1);
    let mut world = super::world::WorldWatcher::for_process(owner);
    let mut world_ready = false;
    let mut last_decrypted = started;
    let scan_stop = Arc::new(AtomicBool::new(false));
    let mut retry_at = started;
    let mut retry_delay = Duration::from_secs(5);
    let mut selection_poll = started - Duration::from_secs(1);
    let mut room_restored = false;
    let result = (|| {
        while !stop.load(Ordering::Relaxed) {
            engine.reconcile_voice(&opus_path);
            engine.reconcile_text();
            if selection_poll.elapsed() >= Duration::from_millis(50) {
                let (selection, proximity) = {
                    let config = settings.read().expect("config lock");
                    let player_audio = &config.audio.output.player_audio;
                    (
                        player_audio.selected_tracks.clone(),
                        player_audio.proximity.clone(),
                    )
                };
                engine.set_selection(selection);
                engine.set_proximity(proximity);
                selection_poll = Instant::now();
            }
            if refresh.elapsed()
                >= if key.is_none() {
                    Duration::from_millis(250)
                } else {
                    Duration::from_secs(1)
                }
            {
                if windows::process_identity(pid).ok() != Some(owner) {
                    cache.invalidate();
                    break;
                }
                ports = windows::game_ports(pid)?;
                refresh = Instant::now();
            }
            if world_poll.elapsed() >= Duration::from_millis(250) {
                let was_ready = world_ready;
                world_ready = world.poll();
                if was_ready && !world_ready {
                    engine.close_tracks_excluded();
                    engine.clear_text();
                    engine.clear_proximity();
                }
                world_poll = Instant::now();
                if key.is_some() && !room_restored {
                    if let Some(token) = world.room_token() {
                        engine.restore_room(cache, owner, token);
                        room_restored = true;
                    }
                }
                if scanner.is_none() && Instant::now() >= retry_at {
                    status.lock().expect("player status").phase = if !world_ready {
                        "waiting_world"
                    } else if key.is_some() {
                        "streaming"
                    } else {
                        "waiting_traffic"
                    };
                }
            }
            network.receive(&mut packet_buffer, |bytes| {
                let Some((connection, incoming, packet)) = windows::datagram(bytes, &ports) else {
                    return;
                };
                let now = Instant::now();
                engine.reconcile_text();
                let text_generation = engine.text_enabled.then_some(engine.text_generation);
                status.lock().expect("player status").captured_packets += 1;
                if let Some(plaintext) = key
                    .as_ref()
                    .and_then(|key: &SessionKey| key.decrypt(packet))
                {
                    last_decrypted = now;
                    engine.receive_packet(
                        connection,
                        incoming,
                        packet,
                        &plaintext,
                        now,
                        world_ready,
                        world_ready,
                    );
                } else {
                    pending_bytes += packet.len();
                    pending.push_back((
                        connection,
                        incoming,
                        packet.to_vec(),
                        now,
                        text_generation,
                    ));
                    // Keep join/property messages received during acquisition;
                    // voice older than 300 ms is discarded only after decrypting.
                    while pending.len() > 4096
                        || pending_bytes > 16 * 1024 * 1024
                        || pending
                            .front()
                            .is_some_and(|p| world_ready && p.3.elapsed() > Duration::from_secs(10))
                    {
                        if let Some(packet) = pending.pop_front() {
                            pending_bytes -= packet.2.len();
                            // Retain a bounded prefix of the join handshake even
                            // when a long world load pushes it out of the recent
                            // packet queue. Only metadata will be replayed later.
                            if key.is_none()
                                && startup_packets.len() < 512
                                && startup_bytes + packet.2.len() <= 2 * 1024 * 1024
                            {
                                startup_bytes += packet.2.len();
                                startup_packets.push_back(packet);
                            }
                        }
                    }
                }
            })?;
            if let Ok(result) = key_rx.try_recv() {
                if let Some(thread) = scanner.take() {
                    let _ = thread.join();
                }
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                match result {
                    Ok(bytes) => candidate = Some(bytes),
                    Err(error) => {
                        retry_at = Instant::now() + retry_delay;
                        retry_delay = (retry_delay * 2).min(Duration::from_secs(30));
                        let mut state = status.lock().expect("player status");
                        state.phase = "retrying_key";
                        state.detail = Some(error);
                    }
                }
            }
            if scanner.is_none() && Instant::now() >= retry_at && pending.len() >= 2 {
                if key.is_none() {
                    status.lock().expect("player status").phase = if world_ready {
                        "waiting_key"
                    } else {
                        "waiting_world"
                    };
                }
                let samples = acquisition_samples(
                    &pending,
                    Instant::now(),
                    key.as_ref().map(|_| last_decrypted),
                );
                if samples.len() >= 2 {
                    let verification_started = Instant::now();
                    let mut from_cache = false;
                    let lookup = if let Some(bytes) = candidate.as_ref() {
                        match cache::verify(bytes, &samples) {
                            Verification::Valid => Lookup::Verified(candidate.take().unwrap()),
                            Verification::Pending => Lookup::Pending,
                            Verification::Invalid => {
                                candidate = None;
                                Lookup::Missing
                            }
                        }
                    } else {
                        from_cache = true;
                        cache.lookup(owner, &samples)
                    };
                    match lookup {
                        Lookup::Verified(bytes) => {
                            let acquired = SessionKey::new(&bytes);
                            cache.store(owner, bytes);
                            if !room_restored {
                                if let Some(token) = world.room_token() {
                                    engine.restore_room(cache, owner, token);
                                    room_restored = true;
                                }
                            }
                            if key.take().is_some() {
                                engine.clear();
                            }
                            for (connection, incoming, packet, at, text_generation) in
                                startup_packets.drain(..).chain(pending.drain(..))
                            {
                                if let Some(plaintext) = acquired.decrypt(&packet) {
                                    engine.receive_packet(
                                        connection,
                                        incoming,
                                        &packet,
                                        &plaintext,
                                        Instant::now(),
                                        world_ready && at.elapsed() <= Duration::from_millis(300),
                                        world_ready
                                            && at.elapsed() < Duration::from_secs(8)
                                            && text_generation == Some(engine.text_generation),
                                    );
                                }
                            }
                            pending_bytes = 0;
                            startup_bytes = 0;
                            last_decrypted = Instant::now();
                            key = Some(acquired);
                            let mut state = status.lock().expect("player status");
                            state.phase = if world_ready {
                                "streaming"
                            } else {
                                "waiting_world"
                            };
                            state.detail = None;
                            retry_delay = Duration::from_secs(5);
                            if from_cache {
                                state.key_scan = Some(super::KeyScanProgress {
                                    stage: "cached",
                                    scanned_bytes: 0,
                                    total_bytes: 0,
                                    candidates_tested: 0,
                                    elapsed_ms: verification_started.elapsed().as_millis() as u64,
                                    scan_elapsed_ms: 0,
                                    workers: 0,
                                });
                                tracing::info!(
                                    "player audio reused authenticated login-session key"
                                );
                            }
                        }
                        Lookup::Pending => {}
                        Lookup::Missing => {
                            if key.take().is_some() {
                                engine.clear();
                            }
                            if world_ready {
                                let drive = config.pagefile_drive.clone();
                                let stop = Arc::clone(&scan_stop);
                                let key_tx = key_tx.clone();
                                {
                                    let mut state = status.lock().expect("player status");
                                    state.phase = "acquiring_key";
                                    state.key_scan = None;
                                    state.detail = None;
                                }
                                let scan_status = Arc::clone(status);
                                scanner = Some(std::thread::spawn(move || {
                                    let _ = key_tx.send(super::key::acquire(
                                        pid,
                                        &drive,
                                        &samples,
                                        &stop,
                                        &scan_status,
                                    ));
                                }));
                            }
                        }
                    }
                }
            }
            engine.tick(Instant::now());
            if engine.tx.is_closed() {
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    })();
    scan_stop.store(true, Ordering::Relaxed);
    if let Some(thread) = scanner {
        let _ = thread.join();
    }
    engine.close_tracks();
    engine.clear_text();
    engine.publish_roster(Instant::now());
    if result.is_ok() && key.is_some() && stop.load(Ordering::Relaxed) {
        if let Some(token) = world.room_token() {
            *cache.room.lock().expect("room identity cache") =
                Some(engine.take_metadata(owner, token));
        }
    }
    result
}

struct Flow {
    number: u64,
    epoch: u64,
    world: Option<String>,
    inbound: Parser,
    outbound: Parser,
    nonces: VecDeque<(bool, [u8; 12])>,
    last_seen: Instant,
    participants: HashMap<u16, Participant>,
    next_participant: u64,
}

#[derive(Clone)]
struct Participant {
    generation: u64,
    display_name: Option<String>,
    user_id: Option<String>,
}

impl Flow {
    fn participant(&mut self, actor: u16) -> Option<&mut Participant> {
        if !self.participants.contains_key(&actor) && self.participants.len() >= 512 {
            return None;
        }
        Some(self.participants.entry(actor).or_insert_with(|| {
            self.next_participant += 1;
            Participant {
                generation: self.next_participant,
                display_name: None,
                user_id: None,
            }
        }))
    }
}

struct Track {
    speaker: SpeakerIdentity,
    buffer: JitterBuffer,
    decoder: Decoder,
    received: Instant,
    next_playout: Instant,
    pending: VecDeque<f32>,
    position: u64,
}
struct Engine {
    opus: Option<Arc<Opus>>,
    voice_generation: u64,
    text_enabled: bool,
    text_generation: u64,
    recent_text: super::chatbox::Deduplicator,
    maximum: usize,
    session: String,
    next_flow: u64,
    next_speaker: u64,
    flows: HashMap<Connection, Flow>,
    tracks: HashMap<String, Track>,
    identities: HashMap<String, SpeakerIdentity>,
    selection: Option<HashSet<String>>,
    roster_dirty: bool,
    active_world: Option<String>,
    tx: EventSender,
    status: SharedStatus,
    proximity: super::proximity::Registry,
    proximity_config: PlayerProximityConfig,
    admitted_proximity: HashSet<String>,
    admission_pending: HashSet<String>,
    proximity_status_at: Instant,
}

impl Engine {
    fn reconcile_voice(&mut self, path: &std::path::Path) {
        let (enabled, generation, path) = {
            let outputs = self.tx.0.lock().expect("network outputs");
            self.maximum = outputs.max_players;
            (
                outputs.voice_enabled(),
                outputs.voice_generation,
                if outputs.opus_library_path.is_empty() {
                    path.to_path_buf()
                } else {
                    outputs.opus_library_path.clone().into()
                },
            )
        };
        if !enabled {
            self.tracks.clear();
            self.opus = None;
        } else if self.opus.is_none() || generation != self.voice_generation {
            self.tracks.clear();
            self.voice_generation = generation;
            match Opus::load(&path) {
                Ok(opus) => self.opus = Some(opus),
                Err(error) => self
                    .tx
                    .0
                    .lock()
                    .expect("network outputs")
                    .voice_failed(error),
            }
        }
    }

    fn reconcile_text(&mut self) {
        let (enabled, generation) = {
            let outputs = self.tx.0.lock().expect("network outputs");
            (outputs.chatbox_enabled(), outputs.text_generation)
        };
        if enabled != self.text_enabled || generation != self.text_generation {
            self.clear_text();
            self.text_enabled = enabled;
        }
    }

    fn clear_text(&mut self) {
        self.recent_text.clear();
        let mut outputs = self.tx.0.lock().expect("network outputs");
        outputs.invalidate_text();
        outputs.text_generation += 1;
        self.text_generation = outputs.text_generation;
    }

    fn text(&mut self, speaker: SpeakerIdentity, text: String, now: Instant) {
        use super::chatbox::Outcome;
        use crate::vrchat_chatbox::{ChatboxEvent, ChatboxMessage};
        let mut outputs = self.tx.0.lock().expect("network outputs");
        if !outputs.chatbox_enabled() || outputs.text_generation != self.text_generation {
            return;
        }
        outputs.chatbox_status.received += 1;
        match self.recent_text.classify(&speaker.id, &text, now) {
            Outcome::Duplicate => outputs.chatbox_status.duplicates += 1,
            Outcome::TooLong => outputs.chatbox_status.dropped += 1,
            Outcome::Cleared => {}
            Outcome::Candidate => {
                let validity = outputs.validity(&speaker.id, now);
                let event = ChatboxEvent::Message(ChatboxMessage {
                    text: text.clone(),
                    speaker: speaker.clone(),
                    received_at: crate::models::now_iso8601(),
                    validity,
                });
                if outputs
                    .chatbox
                    .as_ref()
                    .is_some_and(|tx| tx.try_send(event).is_ok())
                {
                    self.recent_text.commit(&speaker.id, &text, now);
                    outputs.chatbox_status.accepted += 1;
                } else {
                    outputs.chatbox_status.dropped += 1;
                }
            }
        }
    }

    fn take_metadata(&mut self, owner: cache::ProcessIdentity, token: u64) -> RoomMetadata {
        self.clear_proximity();
        let mut flows = std::mem::take(&mut self.flows);
        for flow in flows.values_mut() {
            flow.inbound = Parser::default();
            flow.outbound = Parser::default();
        }
        RoomMetadata {
            owner,
            token,
            session: self.session.clone(),
            next_flow: self.next_flow,
            next_speaker: self.next_speaker,
            flows,
            identities: std::mem::take(&mut self.identities),
            active_world: self.active_world.take(),
        }
    }
    fn restore_room(&mut self, cache: &SessionCache, owner: cache::ProcessIdentity, token: u64) {
        let Some(room) = cache.room.lock().expect("room identity cache").take() else {
            return;
        };
        // Any intervening join/leave invalidates actor assignments. Never guess
        // a name by list position or reuse an old actor after membership changed.
        if room.owner != owner || room.token != token || !self.identities.is_empty() {
            return;
        }
        self.session = room.session;
        self.next_flow = room.next_flow;
        self.next_speaker = room.next_speaker;
        self.flows = room.flows;
        self.identities = room.identities;
        self.active_world = room.active_world;
        self.roster_dirty = true;
    }
    fn new(
        opus: impl Into<Option<Arc<Opus>>>,
        maximum: usize,
        tx: impl Into<EventSender>,
        status: SharedStatus,
    ) -> Self {
        Self {
            opus: opus.into(),
            voice_generation: 0,
            text_enabled: false,
            text_generation: 0,
            recent_text: Default::default(),
            maximum,
            session: format!("vrc:{}", uuid::Uuid::new_v4()),
            next_flow: 0,
            next_speaker: 0,
            flows: HashMap::new(),
            tracks: HashMap::new(),
            identities: HashMap::new(),
            selection: None,
            roster_dirty: false,
            active_world: None,
            tx: tx.into(),
            status,
            proximity: super::proximity::Registry::default(),
            proximity_config: PlayerProximityConfig::default(),
            admitted_proximity: HashSet::new(),
            admission_pending: HashSet::new(),
            proximity_status_at: Instant::now() - Duration::from_secs(1),
        }
    }
    fn selected(&self, id: &str) -> bool {
        self.selection.as_ref().is_none_or(|ids| {
            self.identities
                .get(id)
                .and_then(|speaker| speaker.user_id.as_ref())
                .is_some_and(|user_id| ids.contains(user_id))
        })
    }
    fn set_selection(&mut self, ids: Option<Vec<String>>) {
        let selection = ids.map(|ids| ids.into_iter().collect());
        if self.selection == selection {
            return;
        }
        self.selection = selection;
        let closed: Vec<_> = self
            .tracks
            .keys()
            .filter(|id| !self.selected(id))
            .cloned()
            .collect();
        for id in closed {
            self.close(&id);
        }
        self.roster_dirty = true;
    }
    fn set_proximity(&mut self, config: PlayerProximityConfig) {
        if self.proximity_config == config {
            return;
        }
        let enabling = config.enabled && !self.proximity_config.enabled;
        let disabling = !config.enabled && self.proximity_config.enabled;
        self.proximity_config = config;
        self.status.lock().expect("player status").proximity.enabled =
            self.proximity_config.enabled;
        if enabling {
            for id in self.identities.keys().cloned().collect::<Vec<_>>() {
                self.close_excluded(&id);
            }
        }
        if disabling {
            let blocked = self
                .status
                .lock()
                .expect("player status")
                .proximity
                .blocked_players
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            for id in blocked {
                self.notify_admitted(&id);
            }
        }
        self.refresh_proximity(Instant::now());
    }
    fn remember(
        &mut self,
        id: String,
        name: Option<String>,
        user_id: Option<String>,
    ) -> SpeakerIdentity {
        let old = self.identities.get(&id);
        let changed = old.is_none_or(|speaker| {
            (name.is_some() && speaker.display_name != name)
                || (user_id.is_some() && speaker.user_id != user_id)
        });
        let speaker = self.identities.entry(id.clone()).or_insert_with(|| {
            let index = self.next_speaker;
            self.next_speaker += 1;
            SpeakerIdentity {
                id,
                index,
                display_name: None,
                user_id: None,
            }
        });
        if name.is_some() {
            speaker.display_name = name;
        }
        if user_id.is_some() {
            speaker.user_id = user_id;
        }
        let speaker = speaker.clone();
        if changed {
            self.roster_dirty = true;
            self.tx
                .0
                .lock()
                .expect("network outputs")
                .speaker(speaker.clone());
        }
        speaker
    }
    #[cfg(test)]
    fn receive(
        &mut self,
        connection: Connection,
        incoming: bool,
        packet: &[u8],
        plaintext: &[u8],
        now: Instant,
    ) {
        self.receive_packet(connection, incoming, packet, plaintext, now, true, true);
    }

    #[allow(clippy::too_many_arguments)]
    fn receive_packet(
        &mut self,
        connection: Connection,
        incoming: bool,
        packet: &[u8],
        plaintext: &[u8],
        now: Instant,
        allow_voice: bool,
        allow_text: bool,
    ) {
        self.status.lock().expect("player status").decrypted_packets += 1;
        if !self.flows.contains_key(&connection) {
            if self.flows.len() >= 32 {
                self.flows
                    .retain(|_, flow| flow.last_seen.elapsed() < Duration::from_secs(30));
            }
            if self.flows.len() >= 32 {
                return;
            }
            self.next_flow += 1;
            self.flows.insert(
                connection.clone(),
                Flow {
                    number: self.next_flow,
                    epoch: 0,
                    world: None,
                    inbound: Parser::default(),
                    outbound: Parser::default(),
                    nonces: VecDeque::new(),
                    last_seen: now,
                    participants: HashMap::new(),
                    next_participant: 0,
                },
            );
        }
        let flow = self.flows.get_mut(&connection).unwrap();
        let nonce: [u8; 12] = packet[7..19].try_into().unwrap();
        if flow.nonces.contains(&(incoming, nonce)) {
            return;
        }
        flow.nonces.push_back((incoming, nonce));
        if flow.nonces.len() > 512 {
            flow.nonces.pop_front();
        }
        flow.last_seen = now;
        let parser = if incoming {
            &mut flow.inbound
        } else {
            &mut flow.outbound
        };
        let messages = parser.messages(plaintext, now);
        let mut voices = Vec::new();
        let mut texts = Vec::new();
        let mut updates = Vec::new();
        let mut departures = Vec::new();
        let mut local_position = None;
        let mut player_positions = Vec::new();
        let mut changed = false;
        let mut joined_room = None;
        for message in messages {
            let position = allow_voice
                .then(|| super::proximity::decode_message(&message))
                .flatten();
            if !incoming {
                if let Some(world) = protocol::joined_world(&message) {
                    joined_room = Some(world.clone());
                    if flow.world.as_ref() != Some(&world) {
                        flow.world = Some(world);
                        flow.epoch += 1;
                        flow.inbound = Parser::default();
                        flow.participants.clear();
                        changed = true;
                    }
                }
                if let Some(super::proximity::DecodedMessage {
                    position: super::proximity::PositionDecode::Sample(position),
                    ..
                }) = position
                {
                    local_position = Some(position);
                }
            } else {
                if let Some(super::proximity::DecodedMessage {
                    actor: Some(actor),
                    position: super::proximity::PositionDecode::Sample(position),
                }) = position
                {
                    if let Some(participant) = flow.participant(actor) {
                        player_positions.push((actor, participant.generation, position));
                    }
                }
                for update in super::names::updates(&message) {
                    match update {
                        super::names::Update::Name {
                            actor,
                            display_name,
                            user_id,
                        } => {
                            if flow.participants.get(&actor).is_some_and(|participant| {
                                participant.user_id.is_some()
                                    && user_id.is_some()
                                    && participant.user_id != user_id
                            }) {
                                if let Some(previous) = flow.participants.remove(&actor) {
                                    departures.push((actor, previous.generation));
                                }
                            }
                            if let Some(participant) = flow.participant(actor) {
                                if (display_name.is_some()
                                    && participant.display_name != display_name)
                                    || (user_id.is_some() && participant.user_id != user_id)
                                {
                                    if display_name.is_some() {
                                        participant.display_name = display_name;
                                    }
                                    if user_id.is_some() {
                                        participant.user_id = user_id;
                                    }
                                    updates.push((actor, participant.clone()));
                                }
                            }
                        }
                        super::names::Update::Left(actor) => {
                            if let Some(participant) = flow.participants.remove(&actor) {
                                departures.push((actor, participant.generation));
                            }
                        }
                    }
                }
                if self.text_enabled && allow_text {
                    if let Some((actor, text)) = super::chatbox::parse(&message) {
                        if let Some(participant) = flow.participant(actor) {
                            texts.push((actor, text, participant.clone()));
                        }
                    }
                }
                if allow_voice && self.opus.is_some() {
                    if let Some(packets) = protocol::voice(&message) {
                        for packet in packets {
                            if let Some(participant) = flow.participant(packet.actor) {
                                voices.push((packet, participant.clone()));
                            }
                        }
                    }
                }
            }
        }
        let flow_number = flow.number;
        let epoch = flow.epoch;
        if let Some(world) = joined_room {
            if self.active_world.as_ref() != Some(&world) {
                self.close_tracks_excluded();
                self.clear_text();
                self.clear_proximity();
                self.identities.clear();
                self.flows.retain(|_, flow| flow.number == flow_number);
                self.active_world = Some(world);
                self.roster_dirty = true;
            }
        }
        if changed {
            self.clear_text();
            self.clear_proximity();
            let prefix = format!("{}:{flow_number}:", self.session);
            let ids: Vec<_> = self
                .tracks
                .keys()
                .filter(|id| id.starts_with(&prefix))
                .cloned()
                .collect();
            for id in ids {
                self.close_excluded(&id);
            }
            self.identities.retain(|id, _| !id.starts_with(&prefix));
            self.roster_dirty = true;
        }
        if let Some(position) = local_position {
            self.proximity
                .observe_local(flow_number, epoch, position, now);
        }
        for (actor, generation, position) in player_positions {
            let id = self.speaker_id(flow_number, epoch, actor, generation);
            self.proximity
                .observe_player(&id, flow_number, epoch, position, now);
        }
        self.update_proximity_status(now);
        for (voice, participant) in voices {
            self.voice(flow_number, epoch, voice, participant, now);
        }
        for (actor, text, participant) in texts {
            let id = self.speaker_id(flow_number, epoch, actor, participant.generation);
            let speaker = self.remember(id, participant.display_name, participant.user_id);
            self.text(speaker, text, now);
        }
        for (actor, participant) in updates {
            let id = self.speaker_id(flow_number, epoch, actor, participant.generation);
            let speaker = self.remember(id.clone(), participant.display_name, participant.user_id);
            if let Some(track) = self.tracks.get_mut(&id) {
                track.speaker = speaker;
            }
            if !self.selected(&id) {
                self.close(&id);
            }
        }
        for (actor, generation) in departures {
            let id = self.speaker_id(flow_number, epoch, actor, generation);
            self.close(&id);
            self.recent_text.remove(&id);
            self.proximity.remove(&id);
            self.admitted_proximity.remove(&id);
            self.admission_pending.remove(&id);
            self.status
                .lock()
                .expect("player status")
                .proximity
                .blocked_players
                .remove(&id);
            self.tx
                .0
                .lock()
                .expect("network outputs")
                .invalidate_scope(&id);
            self.identities.remove(&id);
            self.roster_dirty = true;
        }
    }

    fn speaker_id(&self, flow: u64, epoch: u64, actor: u16, generation: u64) -> String {
        format!("{}:{flow}:{epoch}:{actor}:{generation}", self.session)
    }

    fn voice(
        &mut self,
        flow: u64,
        epoch: u64,
        packet: VoicePacket,
        participant: Participant,
        now: Instant,
    ) {
        let id = self.speaker_id(flow, epoch, packet.actor, participant.generation);
        let speaker = self.remember(id.clone(), participant.display_name, participant.user_id);
        // Discover the track and its identity, then discard compressed speech
        // before allocating an Opus decoder, PCM, VAD or recognition connection.
        if !self.selected(&id) {
            return;
        }
        match self.proximity.admission(
            &id,
            self.proximity_config.enabled,
            self.proximity_config.radius_m,
            now,
        ) {
            super::proximity::Admission::Disabled => {
                self.admitted_proximity.remove(&id);
                self.notify_admitted(&id);
            }
            super::proximity::Admission::InRange => {
                self.admitted_proximity.insert(id.clone());
                self.notify_admitted(&id);
            }
            super::proximity::Admission::OutOfRange => {
                self.status
                    .lock()
                    .expect("player status")
                    .proximity
                    .excluded_packets += 1;
                self.close_excluded(&id);
                return;
            }
            super::proximity::Admission::Unknown => {
                self.status
                    .lock()
                    .expect("player status")
                    .proximity
                    .unknown_packets += 1;
                self.close_excluded(&id);
                return;
            }
        }
        if !self.tracks.contains_key(&id) {
            if self.maximum > 0 && self.tracks.len() >= self.maximum {
                let evict = self
                    .tracks
                    .iter()
                    .filter(|(_, t)| t.received.elapsed() > Duration::from_secs(1))
                    .min_by_key(|(_, t)| t.received)
                    .map(|(id, _)| id.clone());
                if let Some(id) = evict {
                    self.close(&id);
                } else {
                    self.status.lock().expect("player status").dropped_frames += 1;
                    return;
                }
            }
            let Some(opus) = &self.opus else { return };
            let decoder = match opus.decoder() {
                Ok(decoder) => decoder,
                Err(_) => {
                    self.status.lock().expect("player status").decode_errors += 1;
                    return;
                }
            };
            self.tracks.insert(
                id.clone(),
                Track {
                    speaker,
                    buffer: JitterBuffer::new(packet.sequence),
                    decoder,
                    received: now,
                    next_playout: now + Duration::from_millis(60),
                    pending: VecDeque::new(),
                    position: 0,
                },
            );
            self.roster_dirty = true;
        }
        let track = self.tracks.get_mut(&id).unwrap();
        if now.saturating_duration_since(track.received) > Duration::from_millis(150) {
            track.buffer.resume_after_idle(packet.sequence);
        }
        if track.buffer.push(packet.sequence, packet.opus) {
            track.received = now;
        }
    }

    fn tick(&mut self, now: Instant) {
        self.tx.0.lock().expect("network outputs").flush_speakers();
        if now.saturating_duration_since(self.proximity_status_at) >= Duration::from_millis(250) {
            self.refresh_proximity(now);
            self.proximity_status_at = now;
        }
        let expired: Vec<_> = self
            .tracks
            .iter()
            .filter(|(_, t)| now.saturating_duration_since(t.received) > Duration::from_secs(15))
            .map(|(id, _)| id.clone())
            .collect();
        for id in expired {
            self.tracks.remove(&id);
            self.roster_dirty = true;
            let _ = self.tx.try_send(AudioEvent::Idle(id));
        }
        for track in self.tracks.values_mut() {
            for _ in 0..5 {
                if now < track.next_playout {
                    break;
                }
                track.next_playout += Duration::from_millis(20);
                if std::mem::take(&mut track.buffer.reset) {
                    if let Some(Ok(decoder)) = self.opus.as_ref().map(|opus| opus.decoder()) {
                        track.decoder = decoder;
                    }
                }
                let samples = match track.buffer.pop() {
                    Some(packet) => match track.decoder.decode(&packet) {
                        Ok(samples) => samples,
                        Err(_) => {
                            self.status.lock().expect("player status").decode_errors += 1;
                            vec![0.0; 320]
                        }
                    },
                    None => vec![0.0; 320],
                };
                track.pending.extend(samples);
                while track.pending.len() >= 512 {
                    let samples = track.pending.drain(..512).collect();
                    let event = AudioEvent::Frame {
                        speaker: track.speaker.clone(),
                        samples,
                        position: track.position,
                    };
                    track.position += 512;
                    let mut status = self.status.lock().expect("player status");
                    if self.tx.try_send(event).is_ok() {
                        status.pcm_frames += 1;
                    } else {
                        status.dropped_frames += 1;
                    }
                }
            }
            if track.next_playout < now {
                // Tell the recognition supervisor that audio was discontinuous.
                // It will reset only this player's VAD/session instead of joining
                // words across the lost interval.
                track.position += 512;
                track.pending.clear();
                track.next_playout = now + Duration::from_millis(20);
                self.status.lock().expect("player status").dropped_frames += 1;
            }
        }
        self.status.lock().expect("player status").active_players = self.tracks.len();
        self.publish_roster(now);
    }
    fn publish_roster(&mut self, now: Instant) {
        if !std::mem::take(&mut self.roster_dirty) {
            return;
        }
        let mut players: Vec<_> = self
            .identities
            .values()
            .map(|speaker| super::PlayerInfo {
                speaker: speaker.clone(),
                selected: self.selected(&speaker.id),
                active: self.tracks.contains_key(&speaker.id),
                distance_m: self.proximity.distance(&speaker.id, now),
            })
            .collect();
        players.sort_by_key(|player| player.speaker.index);
        self.status.lock().expect("player status").players = players;
    }
    fn update_proximity_status(&mut self, now: Instant) {
        let mut status = self.status.lock().expect("player status");
        status.proximity.enabled = self.proximity_config.enabled;
        status.proximity.local_position = self.proximity.local_fresh(now);
        status.proximity.tracked_players = self.proximity.fresh_players(now);
    }
    fn refresh_proximity(&mut self, now: Instant) {
        let mut ids = self.admitted_proximity.clone();
        ids.extend(self.proximity.player_ids().map(str::to_owned));
        let mut blocked = self
            .status
            .lock()
            .expect("player status")
            .proximity
            .blocked_players
            .clone();
        if self.proximity_config.enabled {
            for id in ids {
                let admission =
                    self.proximity
                        .admission(&id, true, self.proximity_config.radius_m, now);
                if matches!(
                    admission,
                    super::proximity::Admission::OutOfRange | super::proximity::Admission::Unknown
                ) {
                    blocked.insert(id.clone());
                    if self.admitted_proximity.remove(&id) {
                        let mut status = self.status.lock().expect("player status");
                        if admission == super::proximity::Admission::Unknown {
                            status.proximity.unknown_packets += 1;
                        } else {
                            status.proximity.excluded_packets += 1;
                        }
                        drop(status);
                        self.close_excluded(&id);
                    }
                }
            }
        } else {
            self.admitted_proximity.clear();
        }
        {
            let mut status = self.status.lock().expect("player status");
            status.proximity.enabled = self.proximity_config.enabled;
            status.proximity.local_position = self.proximity.local_fresh(now);
            status.proximity.tracked_players = self.proximity.fresh_players(now);
            status.proximity.blocked_players = blocked;
        }
        self.refresh_player_distances(now);
    }
    fn refresh_player_distances(&mut self, now: Instant) {
        let mut status = self.status.lock().expect("player status");
        for player in &mut status.players {
            player.distance_m = self.proximity.distance(&player.speaker.id, now);
        }
    }
    fn notify_admitted(&mut self, id: &str) {
        let blocked = self
            .status
            .lock()
            .expect("player status")
            .proximity
            .blocked_players
            .contains(id);
        if !blocked {
            self.admission_pending.remove(id);
            return;
        }
        if self.admission_pending.insert(id.into())
            && self.tx.try_send(AudioEvent::Admitted(id.into())).is_err()
        {
            self.admission_pending.remove(id);
        }
    }
    fn clear_proximity(&mut self) {
        self.proximity.clear();
        self.admitted_proximity.clear();
        self.admission_pending.clear();
        let mut status = self.status.lock().expect("player status");
        status.proximity.local_position = false;
        status.proximity.tracked_players = 0;
        for player in &mut status.players {
            player.distance_m = None;
        }
    }
    fn close(&mut self, id: &str) {
        self.admitted_proximity.remove(id);
        self.tracks.remove(id);
        self.roster_dirty = true;
        let _ = self.tx.try_send(AudioEvent::Closed(id.into()));
    }
    fn close_excluded(&mut self, id: &str) {
        self.admitted_proximity.remove(id);
        let had_pending_admission = self.admission_pending.remove(id);
        let mut status = self.status.lock().expect("player status");
        if status.proximity.blocked_players.len() >= 2048
            && !status.proximity.blocked_players.contains(id)
        {
            status.proximity.blocked_players.clear();
        }
        let newly_blocked = status.proximity.blocked_players.insert(id.into());
        drop(status);
        if newly_blocked || had_pending_admission {
            self.retire_excluded(id);
        }
    }
    fn retire_excluded(&mut self, id: &str) {
        self.tracks.remove(id);
        self.roster_dirty = true;
        let _ = self.tx.try_send(AudioEvent::Excluded(id.into()));
    }
    fn clear(&mut self) {
        self.close_tracks_excluded();
        self.clear_text();
        self.clear_proximity();
        self.flows.clear();
        self.identities.clear();
        self.active_world = None;
        self.roster_dirty = true;
    }
    fn close_tracks(&mut self) {
        // Include idle players whose decoder is gone but recognition may still
        // hold a warm session. A world change must close those sessions too.
        let ids: Vec<_> = self.identities.keys().cloned().collect();
        for id in ids {
            self.close(&id);
        }
    }
    fn close_tracks_excluded(&mut self) {
        let ids: Vec<_> = self.identities.keys().cloned().collect();
        for id in ids {
            self.close_excluded(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proximity_filter_rejects_unknown_and_out_of_range_voice_before_decode() {
        let (voice_tx, mut voice_rx) = tokio::sync::mpsc::channel(8);
        let outputs = Arc::new(std::sync::Mutex::new(
            super::super::session::Outputs::with_voice(voice_tx),
        ));
        let status = Arc::default();
        let mut engine = Engine::new(
            None,
            0,
            EventSender(Arc::clone(&outputs)),
            Arc::clone(&status),
        );
        engine.set_proximity(crate::config::PlayerProximityConfig {
            enabled: true,
            radius_m: 10.0,
        });
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        let now = Instant::now();
        let mut nonce = 0;
        let mut receive = |engine: &mut Engine, message: Vec<u8>, incoming: bool| {
            nonce += 1;
            let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
            bytes.extend((message.len() as u32 + 12).to_be_bytes());
            bytes.extend([0; 4]);
            bytes.extend(message);
            let packet = super::super::crypto::seal(&[3; 32], nonce, &bytes);
            engine.receive_packet(
                connection.clone(),
                incoming,
                &packet,
                &bytes,
                now,
                true,
                false,
            );
        };

        receive(&mut engine, event_12(Some(7), [20.0, 0.0, 0.0]), true);
        engine.voice(
            1,
            0,
            VoicePacket {
                actor: 7,
                sequence: 1,
                opus: vec![255; 1275],
            },
            Participant {
                generation: 1,
                display_name: None,
                user_id: None,
            },
            now,
        );
        receive(&mut engine, event_12(None, [0.0, 0.0, 0.0]), false);
        engine.voice(
            1,
            0,
            VoicePacket {
                actor: 7,
                sequence: 2,
                opus: vec![255; 1275],
            },
            Participant {
                generation: 1,
                display_name: None,
                user_id: None,
            },
            now,
        );
        receive(&mut engine, event_12_at(Some(7), 1, [5.0, 0.0, 0.0]), true);
        for sequence in [3, 4] {
            engine.voice(
                1,
                0,
                VoicePacket {
                    actor: 7,
                    sequence,
                    opus: vec![255; 1275],
                },
                Participant {
                    generation: 1,
                    display_name: None,
                    user_id: None,
                },
                now,
            );
        }

        assert!(engine.tracks.is_empty());
        let status = status.lock().unwrap();
        assert!(status.proximity.local_position);
        assert_eq!(status.proximity.blocked_players.len(), 1);
        assert_eq!(status.proximity.unknown_packets, 1);
        assert_eq!(status.proximity.excluded_packets, 1);
        assert!(matches!(voice_rx.try_recv(), Ok(AudioEvent::Speaker(_))));
        assert!(matches!(voice_rx.try_recv(), Ok(AudioEvent::Excluded(_))));
        assert!(matches!(voice_rx.try_recv(), Ok(AudioEvent::Admitted(_))));
        assert!(voice_rx.try_recv().is_err());
    }

    fn event_12(actor: Option<u8>, [x, y, z]: [f32; 3]) -> Vec<u8> {
        event_12_at(actor, 0, [x, y, z])
    }

    fn event_12_at(actor: Option<u8>, tick: u32, [x, y, z]: [f32; 3]) -> Vec<u8> {
        let mut payload = tick.to_le_bytes().to_vec();
        payload.extend([0x47, 0x33]);
        payload.extend_from_slice(&x.to_le_bytes());
        payload.extend_from_slice(&y.to_le_bytes());
        payload.extend_from_slice(&z.to_le_bytes());
        let mut message = if actor.is_some() {
            vec![0xf3, 4, 12, 2, 245, 0x43]
        } else {
            vec![0xf3, 2, 0xfd, 2, 244, 11, 12, 245, 0x43]
        };
        message.push(payload.len() as u8);
        message.extend(payload);
        if let Some(actor) = actor {
            message.extend([254, 11, actor]);
        }
        message
    }

    #[test]
    fn chatbox_without_audio_preserves_identity_suppresses_refreshes_and_invalidates_departures() {
        use crate::vrchat_chatbox::ChatboxEvent;
        let (text_tx, mut text_rx) = tokio::sync::mpsc::channel(64);
        let (voice_tx, mut voice_rx) = tokio::sync::mpsc::channel(8);
        let mut output = super::super::session::Outputs::with_voice(voice_tx);
        output.chatbox = Some(text_tx);
        let outputs = Arc::new(std::sync::Mutex::new(output));
        let status = Arc::default();
        let mut engine = Engine::new(None, 1, EventSender(Arc::clone(&outputs)), status);
        engine.set_selection(Some(Vec::new()));
        engine.reconcile_voice(std::path::Path::new("missing-opus.dll"));
        assert!(voice_rx.try_recv().is_err());
        assert!(voice_rx.is_closed());
        engine.reconcile_text();
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        let now = Instant::now();
        let mut counter = 0;
        let mut receive = |engine: &mut Engine, message: Vec<u8>, incoming: bool| {
            counter += 1;
            let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
            bytes.extend((message.len() as u32 + 12).to_be_bytes());
            bytes.extend([0; 4]);
            bytes.extend(message);
            let packet = super::super::crypto::seal(&[3; 32], counter, &bytes);
            engine.receive_packet(
                connection.clone(),
                incoming,
                &packet,
                &bytes,
                now,
                false,
                true,
            );
        };
        let message = vec![
            0xf3, 4, 43, 2, 254, 11, 7, 245, 7, 5, b'h', b'e', b'l', b'l', b'o',
        ];
        receive(&mut engine, message.clone(), false);
        assert!(text_rx.try_recv().is_err());
        for _ in 0..6 {
            receive(&mut engine, message.clone(), true);
        }
        let mut accepted = Vec::new();
        while let Ok(event) = text_rx.try_recv() {
            if let ChatboxEvent::Message(message) = event {
                accepted.push(message);
            }
        }
        assert_eq!(accepted.len(), 1);
        assert_eq!(accepted[0].text, "hello");
        let old = accepted.remove(0);
        assert!(old.speaker.user_id.is_none());
        receive(
            &mut engine,
            super::super::names::tests::fixture(7, "Named player"),
            true,
        );
        let ChatboxEvent::Speaker(named) = text_rx.try_recv().unwrap() else {
            panic!("name update")
        };
        assert_eq!(named.id, old.speaker.id);
        assert_eq!(named.display_name.as_deref(), Some("Named player"));
        receive(&mut engine, vec![0xf3, 4, 254, 1, 254, 11, 7], true);
        assert!(!old.validity.is_current());
        receive(&mut engine, message, true);
        while let Ok(event) = text_rx.try_recv() {
            if let ChatboxEvent::Message(message) = event {
                assert_ne!(message.speaker.id, old.speaker.id);
            }
        }
        assert!(engine.tracks.is_empty());
        let outputs = outputs.lock().unwrap();
        assert_eq!(outputs.chatbox_status.accepted, 2);
        assert_eq!(outputs.chatbox_status.duplicates, 5);
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_CAPTURE, VRCS_TEST_PHOTON_KEY and VRCS_TEST_OPUS_DLL"]
    fn replay_local_capture_maps_names_to_actual_connection_tracks() {
        use std::io::BufRead;
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let secret = zeroize::Zeroizing::new(std::env::var("VRCS_TEST_PHOTON_KEY").unwrap());
        let key = zeroize::Zeroizing::new(super::super::replay_tests::hex(&secret));
        let key = SessionKey::new(key.as_slice().try_into().unwrap());
        let file = std::io::BufReader::new(
            std::fs::File::open(std::env::var("VRCS_TEST_CAPTURE").unwrap()).unwrap(),
        );
        let (tx, _rx) = tokio::sync::mpsc::channel(2048);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let mut engine = Engine::new(opus, 0, tx, status);
        let mut heard = HashSet::new();
        let mut named = HashSet::new();
        let mut authenticated = 0;
        for line in file.lines() {
            let line = line.unwrap();
            let parts: Vec<_> = line.split(',').collect();
            if parts.len() != 3 {
                continue;
            }
            let packet = super::super::replay_tests::hex(parts[2].trim());
            let Some(plain) = key.decrypt(&packet) else {
                continue;
            };
            authenticated += 1;
            let connection = Connection {
                local: "127.0.0.1:42000".parse().unwrap(),
                remote: "127.0.0.2:27002".parse().unwrap(),
                challenge: packet[3..7].try_into().unwrap(),
            };
            engine.receive(connection, parts[0] == "S", &packet, &plain, Instant::now());
            heard.extend(engine.tracks.keys().cloned());
            named.extend(
                engine
                    .identities
                    .values()
                    .filter(|speaker| speaker.display_name.is_some())
                    .map(|speaker| speaker.id.clone()),
            );
        }
        assert!(authenticated >= 2);
        assert!(!named.is_empty());
        assert!(
            heard.is_subset(&named),
            "Every speaking track must match a name on its own connection and generation"
        );
        println!(
            "authenticated={authenticated} speaking_tracks={} named_tracks={}",
            heard.len(),
            named.len()
        );
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn selection_discovers_every_track_without_decoding_unselected_audio() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let tone = opus.test_tone(440.0, 1).remove(0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let mut engine = Engine::new(opus, 0, tx, Arc::clone(&status));
        let now = Instant::now();
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        engine.set_selection(Some(Vec::new()));
        for actor in 1..=2 {
            let bytes = protocol::voice_fixture(actor, 1, &[255; 1275]);
            let packet = super::super::crypto::seal(&[3; 32], u64::from(actor), &bytes);
            engine.receive(connection.clone(), true, &packet, &bytes, now);
        }
        engine.tick(now + Duration::from_millis(100));
        assert!(engine.tracks.is_empty());
        assert_eq!(status.lock().unwrap().decode_errors, 0);
        assert_eq!(status.lock().unwrap().pcm_frames, 0);
        let players = status.lock().unwrap().players.clone();
        assert_eq!(players.len(), 2);
        assert!(players.iter().all(|player| !player.selected));
        let first = players[0].speaker.id.clone();
        let user_id = "usr_00000000-0000-4000-8000-000000000001";
        engine.set_selection(Some(vec![user_id.into()]));
        let message =
            super::super::names::tests::user_fixture(1, Some("Selected player"), Some(user_id));
        let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
        bytes.extend((message.len() as u32 + 12).to_be_bytes());
        bytes.extend([0; 4]);
        bytes.extend(message);
        let packet = super::super::crypto::seal(&[3; 32], 8, &bytes);
        engine.receive(connection.clone(), true, &packet, &bytes, now);
        for actor in 1..=2 {
            let bytes = protocol::voice_fixture(actor, 2, &tone);
            let packet = super::super::crypto::seal(&[3; 32], 10 + u64::from(actor), &bytes);
            engine.receive(connection.clone(), true, &packet, &bytes, now);
        }
        engine.tick(now + Duration::from_millis(100));
        let mut frames = 0;
        while let Ok(event) = rx.try_recv() {
            if let AudioEvent::Frame { speaker, .. } = event {
                assert_eq!(speaker.id, first);
                frames += 1;
            }
        }
        assert!(frames > 0);
        engine.set_selection(Some(Vec::new()));
        engine.tick(now + Duration::from_millis(200));
        assert!(engine.tracks.is_empty());
        assert_eq!(status.lock().unwrap().decode_errors, 0);
        assert_eq!(status.lock().unwrap().players.len(), 2);
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn saved_user_selection_follows_new_tracks_and_blocks_unknown_or_same_named_accounts() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let tone = opus.test_tone(440.0, 1).remove(0);
        let user = "usr_00000000-0000-4000-8000-000000000007";
        let other = "usr_00000000-0000-4000-8000-000000000008";
        let config = PlayerAudioConfig {
            selected_tracks: Some(vec![user.into()]),
            ..Default::default()
        };
        let saved = serde_json::to_string(&config).unwrap();
        let mut track_ids = HashSet::new();
        for actor in [7, 23] {
            let restored: PlayerAudioConfig = serde_json::from_str(&saved).unwrap();
            let (tx, mut rx) = tokio::sync::mpsc::channel(256);
            let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
            let mut engine = Engine::new(Arc::clone(&opus), 0, tx, Arc::clone(&status));
            engine.set_selection(restored.selected_tracks);
            let connection = Connection {
                local: "127.0.0.1:42000".parse().unwrap(),
                remote: "127.0.0.2:27002".parse().unwrap(),
                challenge: [actor; 4],
            };
            let now = Instant::now();
            let mut nonce = 0;
            let mut send = |engine: &mut Engine, bytes: Vec<u8>| {
                nonce += 1;
                let packet = super::super::crypto::seal(&[3; 32], nonce, &bytes);
                engine.receive(connection.clone(), true, &packet, &bytes, now);
            };
            let property = |user_id: Option<&str>| {
                let message =
                    super::super::names::tests::user_fixture(actor, Some("Same name"), user_id);
                let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
                bytes.extend((message.len() as u32 + 12).to_be_bytes());
                bytes.extend([0; 4]);
                bytes.extend(message);
                bytes
            };
            send(&mut engine, property(None));
            send(
                &mut engine,
                protocol::voice_fixture(u16::from(actor), 1, &[255; 1275]),
            );
            assert!(
                engine.tracks.is_empty(),
                "a name without a user ID cannot authorize audio"
            );
            send(&mut engine, property(Some(other)));
            send(
                &mut engine,
                protocol::voice_fixture(u16::from(actor), 2, &[255; 1275]),
            );
            assert!(
                engine.tracks.is_empty(),
                "the same display name cannot authorize another account"
            );
            send(&mut engine, property(Some(user)));
            send(
                &mut engine,
                protocol::voice_fixture(u16::from(actor), 3, &tone),
            );
            engine.tick(now + Duration::from_millis(100));
            let mut frames = 0;
            while let Ok(event) = rx.try_recv() {
                if let AudioEvent::Frame { speaker, .. } = event {
                    assert_eq!(speaker.user_id.as_deref(), Some(user));
                    track_ids.insert(speaker.id);
                    frames += 1;
                }
            }
            assert!(
                frames > 0,
                "the saved account is enabled under its new actor and capture session"
            );
            send(&mut engine, property(Some(other)));
            send(
                &mut engine,
                protocol::voice_fixture(u16::from(actor), 4, &[255; 1275]),
            );
            engine.tick(now + Duration::from_millis(200));
            assert!(
                engine.tracks.is_empty(),
                "actor reuse by another account closes the old decoder"
            );
            assert_eq!(status.lock().unwrap().decode_errors, 0);
            while let Ok(event) = rx.try_recv() {
                assert!(!matches!(event, AudioEvent::Frame { .. }));
            }
        }
        assert_eq!(
            track_ids.len(),
            2,
            "persistent selection must not merge runtime audio tracks"
        );
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn same_room_restart_preserves_track_identity_and_names_but_membership_change_does_not() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let mut engine = Engine::new(Arc::clone(&opus), 0, tx.clone(), Arc::clone(&status));
        let now = Instant::now();
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        let user_id = "usr_00000000-0000-4000-8000-000000000007";
        let message =
            super::super::names::tests::user_fixture(7, Some("Track name"), Some(user_id));
        let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
        bytes.extend((message.len() as u32 + 12).to_be_bytes());
        bytes.extend([0; 4]);
        bytes.extend(message);
        let packet = super::super::crypto::seal(&[3; 32], 1, &bytes);
        engine.receive(connection.clone(), true, &packet, &bytes, now);
        let identity = engine.identities.values().next().unwrap().clone();
        let owner = cache::ProcessIdentity {
            pid: 12,
            created: 34,
        };
        let cache = SessionCache::default();
        *cache.room.lock().unwrap() = Some(engine.take_metadata(owner, 100));
        let mut resumed = Engine::new(Arc::clone(&opus), 0, tx.clone(), Arc::clone(&status));
        resumed.restore_room(&cache, owner, 100);
        resumed.set_selection(Some(vec![user_id.into()]));
        let tone = opus.test_tone(440.0, 1).remove(0);
        let bytes = protocol::voice_fixture(7, 1, &tone);
        let packet = super::super::crypto::seal(&[3; 32], 2, &bytes);
        resumed.receive(connection, true, &packet, &bytes, now);
        resumed.tick(now + Duration::from_millis(100));
        let mut found = false;
        while let Ok(event) = rx.try_recv() {
            if let AudioEvent::Frame { speaker, .. } = event {
                assert_eq!(speaker, identity);
                found = true;
            }
        }
        assert!(found);
        *cache.room.lock().unwrap() = Some(resumed.take_metadata(owner, 100));
        let mut changed = Engine::new(opus, 0, tx, status);
        changed.restore_room(&cache, owner, 101);
        assert!(changed.identities.is_empty());
        assert!(changed.flows.is_empty());
    }

    #[tokio::test]
    #[ignore = "Requires VRChat to be closed; starts only the local waiting worker"]
    async fn native_capture_can_start_stop_and_restart_before_vrchat() {
        let game = windows::game().unwrap();
        assert!(
            game.is_none(),
            "Close the active VRChat process first: {game:?}"
        );
        let cache = Arc::new(SessionCache::default());
        for _ in 0..2 {
            let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
            let (sender, events) = tokio::sync::mpsc::channel(2048);
            let capture = start(
                PlayerAudioConfig::default(),
                Arc::clone(&status),
                Arc::new(std::sync::Mutex::new(
                    super::super::session::Outputs::with_voice(sender),
                )),
                Arc::clone(&cache),
                Arc::new(std::sync::RwLock::new(crate::config::AppConfig::default())),
            )
            .unwrap();
            tokio::time::sleep(Duration::from_millis(30)).await;
            assert!(!events.is_closed());
            assert_eq!(status.lock().unwrap().phase, "waiting_game");
            let stopping = Instant::now();
            capture.shutdown().await;
            assert!(stopping.elapsed() < Duration::from_secs(1));
            assert_eq!(status.lock().unwrap().phase, "stopped");
        }
    }

    #[test]
    fn waiting_for_game_is_cancellable_and_absence_is_not_a_start_failure() {
        let stop = AtomicBool::new(false);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let begin = Instant::now();
        let absent = wait_for_game(&stop, &status, || {
            assert_eq!(status.lock().unwrap().phase, "waiting_game");
            stop.store(true, Ordering::Relaxed);
            Ok(None)
        })
        .unwrap();
        assert!(absent.is_none());
        assert!(begin.elapsed() < Duration::from_millis(100));
        stop.store(false, Ordering::Relaxed);
        let mut polls = 0;
        let game = wait_for_game(&stop, &status, || {
            polls += 1;
            Ok((polls == 2).then(|| (42, "test/VRChat.exe".into())))
        })
        .unwrap()
        .unwrap();
        assert_eq!(game.0, 42);
        assert_eq!(polls, 2);
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn unlimited_capture_emits_pcm_for_more_than_eight_players() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let tone = opus.test_tone(440.0, 1).remove(0);
        for (limit, expected) in [(0, 12), (4, 4)] {
            let (tx, mut rx) = tokio::sync::mpsc::channel(2048);
            let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
            let mut engine = Engine::new(Arc::clone(&opus), limit, tx, Arc::clone(&status));
            let now = Instant::now();
            for actor in 1..=12 {
                let connection = Connection {
                    local: "127.0.0.1:42000".parse().unwrap(),
                    remote: "127.0.0.2:27002".parse().unwrap(),
                    challenge: [1; 4],
                };
                let bytes = protocol::voice_fixture(actor, 1, &tone);
                let packet = super::super::crypto::seal(&[3; 32], u64::from(actor), &bytes);
                engine.receive(connection, true, &packet, &bytes, now);
            }
            engine.tick(now + Duration::from_millis(100));
            assert_eq!(status.lock().unwrap().active_players, expected);
            let mut ids = HashSet::new();
            while let Ok(event) = rx.try_recv() {
                if let AudioEvent::Frame {
                    speaker, samples, ..
                } = event
                {
                    assert_eq!(samples.len(), 512);
                    ids.insert(speaker.id);
                }
            }
            assert_eq!(ids.len(), expected);
        }
    }

    #[test]
    fn acquisition_starts_without_fixed_wait_and_rekeys_only_for_fresh_distinct_samples() {
        let now = Instant::now();
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        let packet = |counter| super::super::crypto::seal(&[3; 32], counter, b"sample packet");
        let mut pending = VecDeque::from([
            (connection.clone(), true, packet(1), now, None),
            (connection.clone(), true, packet(2), now, None),
        ]);
        assert_eq!(acquisition_samples(&pending, now, None).len(), 2);
        assert!(acquisition_samples(&pending, now, Some(now)).is_empty());
        let before = now - Duration::from_millis(400);
        assert_eq!(acquisition_samples(&pending, now, Some(before)).len(), 2);
        pending[1].2 = packet(1);
        assert_eq!(acquisition_samples(&pending, now, Some(before)).len(), 1);
        pending[1].2 = packet(2);
        pending[0].3 = before - Duration::from_millis(1);
        assert_eq!(acquisition_samples(&pending, now, Some(before)).len(), 1);
        pending[0].3 = now;
        pending[1].0.challenge = [2; 4];
        assert_eq!(acquisition_samples(&pending, now, None).len(), 1);
        assert!(acquisition_samples(&pending, now + Duration::from_secs(3), None).is_empty());
    }
    use std::path::Path;

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn player_names_arrive_late_survive_idle_and_do_not_cross_worlds_or_reused_actors() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let tone = opus.test_tone(440.0, 1).remove(0);
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let mut engine = Engine::new(opus, 4, tx, status);
        let now = Instant::now();
        let connection = Connection {
            local: "127.0.0.1:42000".parse().unwrap(),
            remote: "127.0.0.2:27002".parse().unwrap(),
            challenge: [1; 4],
        };
        let wrap = |message: &[u8]| {
            let mut bytes = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
            bytes.extend((message.len() as u32 + 12).to_be_bytes());
            bytes.extend([0; 4]);
            bytes.extend(message);
            bytes
        };
        let mut counter = 0;
        let mut receive = |engine: &mut Engine, bytes: Vec<u8>, incoming: bool, voice: bool| {
            counter += 1;
            let packet = super::super::crypto::seal(&[3; 32], counter, &bytes);
            engine.receive_packet(
                connection.clone(),
                incoming,
                &packet,
                &bytes,
                now,
                voice,
                true,
            );
        };
        receive(
            &mut engine,
            protocol::voice_fixture(1, 1, &tone),
            true,
            true,
        );
        let old = engine.tracks.values().next().unwrap().speaker.clone();
        assert_eq!(old.display_name, None);
        let AudioEvent::Speaker(discovered) = rx.try_recv().unwrap() else {
            panic!("initial track discovery")
        };
        assert_eq!(discovered, old);
        // Buffered metadata remains useful even when its audio is too old.
        receive(
            &mut engine,
            wrap(&super::super::names::tests::fixture(1, "玩家 A")),
            true,
            false,
        );
        let AudioEvent::Speaker(named) = rx.try_recv().unwrap() else {
            panic!("name update")
        };
        assert_eq!(named.id, old.id);
        assert_eq!(named.display_name.as_deref(), Some("玩家 A"));
        engine.tick(now + Duration::from_secs(20));
        while rx.try_recv().is_ok() {}
        receive(
            &mut engine,
            protocol::voice_fixture(1, 2, &tone),
            true,
            true,
        );
        assert_eq!(engine.tracks.values().next().unwrap().speaker, named);
        receive(
            &mut engine,
            wrap(&[0xf3, 4, 254, 1, 254, 11, 1]),
            true,
            false,
        );
        receive(
            &mut engine,
            wrap(&super::super::names::tests::fixture(1, "玩家 B")),
            true,
            false,
        );
        receive(
            &mut engine,
            protocol::voice_fixture(1, 3, &tone),
            true,
            true,
        );
        let replacement = engine.tracks.values().next().unwrap().speaker.clone();
        assert_ne!(replacement.id, named.id);
        assert_eq!(replacement.display_name.as_deref(), Some("玩家 B"));
        let mut join = vec![0xf3, 2, 226];
        join.extend(b"wrld_01234567-0123-0123-0123-012345678901:123");
        receive(&mut engine, wrap(&join), false, false);
        assert!(engine.tracks.is_empty());
        receive(
            &mut engine,
            protocol::voice_fixture(1, 4, &tone),
            true,
            true,
        );
        let next_world = &engine.tracks.values().next().unwrap().speaker;
        assert_ne!(next_world.id, replacement.id);
        assert_eq!(next_world.display_name, None);
    }

    #[test]
    #[ignore = "Requires VRCS_TEST_OPUS_DLL"]
    fn native_realtime_pcm_is_separate_contiguous_and_expires() {
        let opus = Opus::load(Path::new(&std::env::var("VRCS_TEST_OPUS_DLL").unwrap())).unwrap();
        let frequencies = [440.0, 880.0, 1320.0];
        let tones: Vec<_> = frequencies.iter().map(|f| opus.test_tone(*f, 64)).collect();
        let (tx, mut rx) = tokio::sync::mpsc::channel(256);
        let status = Arc::new(std::sync::Mutex::new(super::super::Status::default()));
        let mut engine = Engine::new(opus, 3, tx, Arc::clone(&status));
        let start = Instant::now();
        let secret = [3; 32];
        let key = SessionKey::new(&secret);
        let mut streams: HashMap<String, (u64, Vec<f32>)> = HashMap::new();
        for step in 0..80 {
            let now = start + Duration::from_millis(step * 20);
            if step < 64 {
                for (track, tone) in tones.iter().enumerate() {
                    // Actor 1 in a second connection must get a separate identity.
                    let connection = Connection {
                        local: "127.0.0.1:42000".parse().unwrap(),
                        remote: "127.0.0.2:27002".parse().unwrap(),
                        challenge: [if track == 2 { 2 } else { 1 }; 4],
                    };
                    let actor = if track == 1 { 2 } else { 1 };
                    let plaintext = protocol::voice_fixture(
                        actor,
                        (65520u16).wrapping_add(step as u16),
                        &tone[step as usize],
                    );
                    let packet =
                        super::super::crypto::seal(&secret, step * 3 + track as u64, &plaintext);
                    let decoded = key.decrypt(&packet).unwrap();
                    engine.receive(connection.clone(), true, &packet, &decoded, now);
                    // Duplicate capture from overlapping adapters must be harmless.
                    engine.receive(connection, true, &packet, &decoded, now);
                }
            }
            engine.tick(now);
            while let Ok(event) = rx.try_recv() {
                if let AudioEvent::Frame {
                    speaker,
                    samples,
                    position,
                } = event
                {
                    assert_eq!(samples.len(), 512);
                    let stream = streams
                        .entry(speaker.id)
                        .or_insert((speaker.index, Vec::new()));
                    assert_eq!(position, stream.1.len() as u64);
                    stream.1.extend(samples);
                }
            }
        }
        assert_eq!(streams.len(), 3);
        for (index, samples) in streams.values() {
            let samples = &samples[1600..16000];
            let power = |frequency: f32| {
                let (sin, cos) =
                    samples
                        .iter()
                        .enumerate()
                        .fold((0.0f32, 0.0f32), |(s, c), (i, value)| {
                            let phase = std::f32::consts::TAU * frequency * i as f32 / 16_000.0;
                            (s + value * phase.sin(), c + value * phase.cos())
                        });
                sin.hypot(cos)
            };
            let own = power(frequencies[*index as usize]);
            assert!(own > 100.0);
            for (other, frequency) in frequencies.iter().enumerate() {
                if other != *index as usize {
                    assert!(own > 10.0 * power(*frequency));
                }
            }
        }
        assert_eq!(status.lock().unwrap().decode_errors, 0);
        assert_eq!(status.lock().unwrap().dropped_frames, 0);
        engine.tick(start + Duration::from_secs(20));
        assert_eq!(status.lock().unwrap().active_players, 0);
        assert_eq!(
            (0..3)
                .filter(|_| matches!(rx.try_recv(), Ok(AudioEvent::Idle(_))))
                .count(),
            3
        );
        // Idle decoder retirement must not look like a departure, but changing
        // world must still close all warm recognition sessions.
        engine.clear();
        assert_eq!(
            (0..3)
                .filter(|_| matches!(rx.try_recv(), Ok(AudioEvent::Closed(_))))
                .count(),
            3
        );
    }
}
