use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;
use serde_json::Value;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::config::UniTextConfig;
use crate::models::now_iso8601;
use crate::subtitle_output::PresentationEvent;

use layout::{UniTextLayout, build_test_screen};
use protocol::{
    BaseViewport, DiffScheduler, LiveOscParamEvent, OSC_PARAM_HEIGHT, OSC_PARAM_LOCAL_CLEAR,
    OSC_PARAM_WIDTH, OscQueryAvatarInfo, Packet, ShadowCrt, UniTextTier, ViewportConfig,
    VirtualScreen, decode_live_osc_events, encode_osc_bundle, encode_visible_packet,
    parse_oscquery_avatar,
};

mod advance;
mod layout;
mod lut12;
mod protocol;

const SERVICE_TYPE: &str = "_oscjson._tcp.local.";
const DEFAULT_OSC_RECEIVE_PORT: u16 = 9001;
const QUEUE_CAPACITY: usize = 64;
const OVERFLOW_SCROLL_DWELL: Duration = Duration::from_millis(900);
const RESIZE_DEBOUNCE: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, Serialize)]
pub struct UniTextRuntimeStatus {
    pub enabled: bool,
    pub status: String,
    pub target: String,
    pub detected_tier: Option<String>,
    pub active_tier: Option<String>,
    pub last_error: Option<String>,
    pub last_sent_at: Option<String>,
}

enum UniTextCommand {
    Config {
        config: UniTextConfig,
        self_player_id: String,
    },
    Muted(Option<bool>),
    Test,
    Clear,
}

#[derive(Clone, PartialEq)]
struct ProbeState {
    connection: &'static str,
    avatar: OscQueryAvatarInfo,
    error: Option<String>,
}

impl Default for ProbeState {
    fn default() -> Self {
        Self {
            connection: "discovering",
            avatar: OscQueryAvatarInfo::default(),
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct UniTextDispatcher {
    sender: mpsc::Sender<UniTextCommand>,
    status: Arc<Mutex<UniTextRuntimeStatus>>,
}

impl UniTextDispatcher {
    pub fn new(
        config: UniTextConfig,
        self_player_id: String,
        presentation_rx: broadcast::Receiver<PresentationEvent>,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        let (sender, cmd_rx) = mpsc::channel(QUEUE_CAPACITY);
        let initial_status = initial_runtime_status(&config);
        let status = Arc::new(Mutex::new(initial_status));
        let (probe_enable_tx, probe_enable_rx) = watch::channel(config.enabled);
        let (probe_tx, probe_rx) = watch::channel(ProbeState::default());
        let (live_osc_tx, live_osc_rx) = mpsc::channel(QUEUE_CAPACITY);

        tokio::spawn(run_udp_osc_listener(
            DEFAULT_OSC_RECEIVE_PORT,
            probe_enable_rx.clone(),
            shutdown.clone(),
            live_osc_tx.clone(),
        ));
        tokio::spawn(run_probe(
            probe_enable_rx,
            shutdown.clone(),
            probe_tx,
            live_osc_tx,
        ));
        tokio::spawn(run_worker(
            config,
            self_player_id,
            cmd_rx,
            presentation_rx,
            live_osc_rx,
            probe_enable_tx,
            probe_rx,
            shutdown,
            Arc::clone(&status),
        ));

        Self { sender, status }
    }

    pub fn update_config(&self, config: UniTextConfig, self_player_id: String) {
        let _ = self.sender.try_send(UniTextCommand::Config {
            config,
            self_player_id,
        });
    }

    pub fn update_mute_status(&self, muted: Option<bool>) {
        let _ = self.sender.try_send(UniTextCommand::Muted(muted));
    }

    pub fn queue_test(&self) -> Result<(), &'static str> {
        let status = self.status();
        if !status.enabled {
            return Err("unitext.disabled");
        }
        if status.active_tier.is_none() {
            return Err("unitext.not_detected");
        }
        self.sender
            .try_send(UniTextCommand::Test)
            .map_err(|_| "unitext.queue_full")
    }

    pub fn clear(&self) -> Result<(), &'static str> {
        let status = self.status();
        if !status.enabled {
            return Err("unitext.disabled");
        }
        self.sender
            .try_send(UniTextCommand::Clear)
            .map_err(|_| "unitext.queue_full")
    }

    pub fn status(&self) -> UniTextRuntimeStatus {
        self.status.lock().expect("UniText status lock").clone()
    }
}

fn initial_runtime_status(config: &UniTextConfig) -> UniTextRuntimeStatus {
    let manual_tier = UniTextTier::from_str_name(&config.tier);
    UniTextRuntimeStatus {
        enabled: config.enabled,
        status: if !config.enabled {
            "disabled".into()
        } else if manual_tier.is_some() {
            "ready".into()
        } else {
            "discovering".into()
        },
        target: format!("127.0.0.1:{}", config.port),
        detected_tier: None,
        active_tier: if config.enabled {
            manual_tier.map(|t| t.as_str().into())
        } else {
            None
        },
        last_error: None,
        last_sent_at: None,
    }
}

struct WorkerSession {
    config: UniTextConfig,
    layout: UniTextLayout,
    target: VirtualScreen,
    shadow: ShadowCrt,
    scheduler: DiffScheduler,
    base_viewport: BaseViewport,
    live_width: f64,
    live_height: f64,
    last_polled_axes: Option<(f64, f64)>,
    had_connected_probe: bool,
    viewport: ViewportConfig,
    pending_viewport: Option<(ViewportConfig, Instant)>,
    showing_test_screen: bool,
    force_clear_all: bool,
    refresh_row: usize,
    idle_ticks: u8,
    active_since_view_refresh: u8,
    needs_nop: bool,
    need_visible_sync: bool,
    muted: Option<bool>,
    last_content_at: Option<Instant>,
    overflow_ready_since: Option<Instant>,
    last_avatar_id: Option<String>,
    last_local_clear: bool,
    send_geometry: bool,
    active_tier: Option<UniTextTier>,
}

fn probe_supports_geometry(probe: &ProbeState) -> bool {
    probe.connection == "connected"
        && (probe.avatar.proto_ver.unwrap_or(1) >= 2 || probe.avatar.has_resize_axes)
}

impl WorkerSession {
    fn new(config: UniTextConfig, initial_probe: &ProbeState) -> Self {
        let base_viewport = resolve_base_viewport(initial_probe);
        let live_width = initial_probe.avatar.width;
        let live_height = initial_probe.avatar.height;
        let had_connected_probe = initial_probe.connection == "connected";
        let last_polled_axes = had_connected_probe.then_some((live_width, live_height));
        let viewport = base_viewport.resolve_live(live_width, live_height);
        let active_tier = resolve_active_tier(&config, initial_probe);
        let send_geometry = probe_supports_geometry(initial_probe);
        let mut target = VirtualScreen::new(viewport.cols, viewport.rows);
        target.geometry = send_geometry.then_some((viewport.rows, viewport.row_px));
        let need_visible_sync = config.enabled && active_tier.is_some();

        Self {
            config,
            layout: UniTextLayout::default(),
            target,
            shadow: ShadowCrt::new(viewport.rows),
            scheduler: DiffScheduler::default(),
            base_viewport,
            live_width,
            live_height,
            last_polled_axes,
            had_connected_probe,
            viewport,
            pending_viewport: None,
            showing_test_screen: false,
            force_clear_all: false,
            refresh_row: 0,
            idle_ticks: 0,
            active_since_view_refresh: 0,
            needs_nop: false,
            need_visible_sync,
            muted: None,
            last_content_at: None,
            overflow_ready_since: None,
            last_avatar_id: None,
            last_local_clear: false,
            send_geometry,
            active_tier,
        }
    }

    fn sync_target_geometry(&mut self) {
        self.target.geometry = self
            .send_geometry
            .then_some((self.viewport.rows, self.viewport.row_px));
    }

    fn clear_display(&mut self, emit_clear_all: bool) {
        self.layout.clear(&mut self.target);
        self.shadow.reset_cleared();
        self.scheduler.reset();
        self.showing_test_screen = false;
        self.needs_nop = false;
        self.overflow_ready_since = None;
        self.last_content_at = None;
        self.force_clear_all = emit_clear_all && self.config.enabled && self.active_tier.is_some();
    }

    fn apply_viewport_transition(&mut self, next_viewport: ViewportConfig) {
        if next_viewport.rows != self.viewport.rows || next_viewport.cols != self.viewport.cols {
            self.viewport = next_viewport;
            self.target = VirtualScreen::new(self.viewport.cols, self.viewport.rows);
            self.shadow = ShadowCrt::new(self.viewport.rows);
            self.scheduler.reset();
            self.refresh_row = 0;
            self.overflow_ready_since = None;
            if self.showing_test_screen {
                self.target = build_test_screen(self.viewport);
            } else {
                self.layout.reset_viewport();
                self.layout
                    .sync_to_screen(&mut self.target, &self.config, self.viewport);
            }
            self.sync_target_geometry();
            if self.config.enabled && self.active_tier.is_some() {
                self.need_visible_sync = true;
                self.force_clear_all = true;
                self.needs_nop = false;
            }
        } else if next_viewport != self.viewport {
            let row_page_changed = next_viewport.send_row_page != self.viewport.send_row_page;
            self.viewport = next_viewport;
            self.overflow_ready_since = None;
            if self.showing_test_screen {
                self.target = build_test_screen(self.viewport);
            } else {
                self.layout.reset_viewport();
                self.layout
                    .sync_to_screen(&mut self.target, &self.config, self.viewport);
            }
            self.sync_target_geometry();
            if row_page_changed && !self.target.is_empty() {
                self.shadow.invalidate_all();
            }
        }
    }

    fn schedule_resize_debounce(&mut self, now: Instant) {
        let candidate = self
            .base_viewport
            .resolve_live(self.live_width, self.live_height);
        if candidate == self.viewport {
            self.pending_viewport = None;
        } else if self.pending_viewport.is_none_or(|(p, _)| p != candidate) {
            self.pending_viewport = Some((candidate, now + RESIZE_DEBOUNCE));
        }
    }

    fn flush_pending_resize(&mut self) {
        if let Some((next_viewport, _)) = self.pending_viewport.take()
            && next_viewport != self.viewport
        {
            self.apply_viewport_transition(next_viewport);
        }
    }

    fn handle_probe_update(&mut self, probe: &ProbeState) {
        if !self.last_local_clear && probe.avatar.local_clear {
            self.clear_display(false);
        }
        self.last_local_clear = probe.avatar.local_clear;

        let avatar_changed =
            probe.avatar.avatar_id.is_some() && probe.avatar.avatar_id != self.last_avatar_id;
        if probe.avatar.avatar_id.is_some() {
            self.last_avatar_id = probe.avatar.avatar_id.clone();
        }

        let next_base = resolve_base_viewport(probe);
        let base_changed = next_base != self.base_viewport;
        let newly_connected = !self.had_connected_probe && probe.connection == "connected";
        let polled_axes = (probe.avatar.width, probe.avatar.height);
        let polled_axes_changed = self.last_polled_axes != Some(polled_axes);
        if probe.connection == "connected" {
            self.had_connected_probe = true;
            self.last_polled_axes = Some(polled_axes);
        }
        if newly_connected || avatar_changed || base_changed || polled_axes_changed {
            self.live_width = probe.avatar.width;
            self.live_height = probe.avatar.height;
        }
        self.base_viewport = next_base;
        self.pending_viewport = None;

        let next_viewport = self
            .base_viewport
            .resolve_live(self.live_width, self.live_height);
        let size_changed =
            next_viewport.rows != self.viewport.rows || next_viewport.cols != self.viewport.cols;
        let viewport_changed = next_viewport != self.viewport;
        let next_tier = resolve_active_tier(&self.config, probe);
        let tier_changed = next_tier != self.active_tier;
        self.active_tier = next_tier;
        self.send_geometry = probe_supports_geometry(probe);

        if viewport_changed {
            self.apply_viewport_transition(next_viewport);
        } else {
            self.sync_target_geometry();
        }

        if avatar_changed {
            self.shadow = ShadowCrt::new(self.viewport.rows);
        }

        if self.config.enabled
            && self.active_tier.is_some()
            && (avatar_changed || size_changed || tier_changed)
        {
            self.need_visible_sync = true;
            self.scheduler.reset();
            self.overflow_ready_since = None;
            if size_changed || self.target.is_empty() {
                self.force_clear_all = true;
            } else {
                self.shadow.invalidate_all();
            }
        }
    }

    fn handle_live_osc_event(&mut self, event: LiveOscParamEvent, now: Instant) {
        if !self.config.enabled {
            return;
        }
        match event {
            LiveOscParamEvent::Width(w) => {
                self.live_width = w;
                self.schedule_resize_debounce(now);
            }
            LiveOscParamEvent::Height(h) => {
                self.live_height = h;
                self.schedule_resize_debounce(now);
            }
            LiveOscParamEvent::LocalClear(val) => {
                if !self.last_local_clear && val {
                    self.clear_display(false);
                }
                self.last_local_clear = val;
            }
        }
    }

    fn handle_presentation_event(&mut self, event: PresentationEvent, now: Instant) {
        if !self.config.enabled {
            return;
        }
        if apply_presentation_event(
            &mut self.layout,
            &mut self.target,
            event,
            &self.config,
            self.muted,
        ) {
            self.showing_test_screen = false;
            self.layout
                .sync_to_screen(&mut self.target, &self.config, self.viewport);
            self.overflow_ready_since = None;
            self.last_content_at = Some(now);
        }
    }

    fn next_tick_packet(&mut self, tier: UniTextTier, now: Instant) -> Option<Packet> {
        if self.config.clear_after_seconds > 0
            && !self.target.is_empty()
            && !self.layout.has_pending_overflow()
            && self.last_content_at.is_some_and(|at| {
                now.saturating_duration_since(at)
                    >= Duration::from_secs(self.config.clear_after_seconds as u64)
            })
        {
            self.clear_display(true);
        }

        if self.force_clear_all {
            self.force_clear_all = false;
            self.needs_nop = true;
            self.idle_ticks = 0;
            self.overflow_ready_since = None;
            return Some(Packet::clear_all(tier));
        }

        if let Some(pkt) = self
            .scheduler
            .next_packet(&self.target, &self.shadow, tier, now)
        {
            self.idle_ticks = 0;
            self.overflow_ready_since = None;
            self.needs_nop = pkt.ctrl != protocol::CTRL_NOP;
            return Some(pkt);
        }

        if self.needs_nop {
            self.needs_nop = false;
            self.overflow_ready_since = None;
            return Some(Packet::nop(tier));
        }

        if self.layout.has_pending_overflow() {
            self.idle_ticks = 0;
            let ready_at = *self.overflow_ready_since.get_or_insert(now);
            if now.duration_since(ready_at) >= OVERFLOW_SCROLL_DWELL
                && self
                    .layout
                    .advance_overflow(&mut self.target, &self.config, self.viewport)
            {
                self.overflow_ready_since = None;
                self.last_content_at = Some(now);
                return self
                    .scheduler
                    .next_packet(&self.target, &self.shadow, tier, now)
                    .inspect(|pkt| {
                        self.needs_nop = pkt.ctrl != protocol::CTRL_NOP;
                    });
            }
            return None;
        }

        if !self.target.is_empty() {
            self.overflow_ready_since = None;
            self.idle_ticks = self.idle_ticks.saturating_add(1);
            if self.idle_ticks >= 3 {
                self.idle_ticks = 0;
                let rows = self.target.rows;
                let prev_refresh = self.refresh_row;
                let mut invalidated = false;
                for step in 0..rows {
                    let vr = (self.refresh_row + step) % rows;
                    let phys = self.target.visual_to_physical(vr);
                    if self.target.row_has_content_or_style(phys) {
                        self.shadow.invalidate_occupied_row(phys, &self.target);
                        self.refresh_row = (vr + 1) % rows;
                        invalidated = true;
                        break;
                    }
                }
                if !invalidated {
                    self.shadow.invalidate_view();
                    self.refresh_row = (self.refresh_row + 1) % rows;
                }
                if self.refresh_row <= prev_refresh && self.target.geometry.is_some() {
                    self.shadow.invalidate_geometry();
                }
            }
            return None;
        }

        self.overflow_ready_since = None;
        None
    }
}

async fn wait_resize_debounce(pending: Option<(ViewportConfig, Instant)>) {
    match pending {
        Some((_, deadline)) => {
            tokio::time::sleep(deadline.saturating_duration_since(Instant::now())).await;
        }
        None => std::future::pending().await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_worker(
    config: UniTextConfig,
    self_player_id: String,
    mut cmd_rx: mpsc::Receiver<UniTextCommand>,
    mut presentation_rx: broadcast::Receiver<PresentationEvent>,
    mut live_osc_rx: mpsc::Receiver<LiveOscParamEvent>,
    probe_enable_tx: watch::Sender<bool>,
    mut probe_rx: watch::Receiver<ProbeState>,
    mut shutdown: watch::Receiver<bool>,
    status: Arc<Mutex<UniTextRuntimeStatus>>,
) {
    let socket = match UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).await {
        Ok(socket) => Some(socket),
        Err(error) => {
            set_error_status(&status, &config, format!("UDP bind failed: {error}"));
            None
        }
    };

    let mut session = WorkerSession::new(config, &probe_rx.borrow());
    session.layout.set_self_player_name(&self_player_id);

    let mut ticker = tokio::time::interval(Duration::from_millis(
        session.config.interval_ms.max(80) as u64,
    ));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    refresh_status(
        &status,
        &session.config,
        &probe_rx.borrow(),
        session.active_tier,
    );

    loop {
        if *shutdown.borrow() {
            break;
        }
        tokio::select! {
            _ = shutdown.changed() => break,
            cmd = cmd_rx.recv() => match cmd {
                None => break,
                Some(UniTextCommand::Config {
                    config: next,
                    self_player_id,
                }) => {
                    let interval_changed = next.interval_ms != session.config.interval_ms;
                    let became_enabled = !session.config.enabled && next.enabled;
                    let preserve_changed =
                        session.config.preserve_original_text != next.preserve_original_text;
                    let player_name_changed =
                        session.layout.set_self_player_name(&self_player_id);
                    session.config = next;
                    let _ = probe_enable_tx.send(session.config.enabled);
                    if interval_changed {
                        ticker = tokio::time::interval(Duration::from_millis(session.config.interval_ms.max(80) as u64));
                        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    }
                    let next_tier = resolve_active_tier(&session.config, &probe_rx.borrow());
                    if !session.config.enabled {
                        session.force_clear_all = false;
                        session.needs_nop = false;
                        session.overflow_ready_since = None;
                        session.pending_viewport = None;
                        session.scheduler.reset();
                    } else if became_enabled || next_tier != session.active_tier {
                        session.active_tier = next_tier;
                        if session.active_tier.is_some() {
                            session.need_visible_sync = true;
                            session.scheduler.reset();
                            session.overflow_ready_since = None;
                            if !session.showing_test_screen {
                                session.layout.sync_to_screen(
                                    &mut session.target,
                                    &session.config,
                                    session.viewport,
                                );
                            }
                            if session.target.is_empty() {
                                session.force_clear_all = true;
                            } else {
                                session.shadow.invalidate_all();
                            }
                        }
                    } else if (preserve_changed || player_name_changed)
                        && !session.showing_test_screen
                    {
                        session.overflow_ready_since = None;
                        session.layout.sync_to_screen(
                            &mut session.target,
                            &session.config,
                            session.viewport,
                        );
                    }
                    session.active_tier = next_tier;
                    refresh_status(&status, &session.config, &probe_rx.borrow(), session.active_tier);
                }
                Some(UniTextCommand::Muted(next_muted)) => {
                    session.muted = next_muted;
                }
                Some(UniTextCommand::Test) => {
                    if session.config.enabled && session.active_tier.is_some() {
                        session.need_visible_sync = true;
                        session.layout.clear(&mut session.target);
                        session.target = build_test_screen(session.viewport);
                        session.sync_target_geometry();
                        session.showing_test_screen = true;
                        session.shadow.reset_cleared();
                        session.scheduler.reset();
                        session.needs_nop = false;
                        session.overflow_ready_since = None;
                        session.force_clear_all = true;
                        session.last_content_at = Some(Instant::now());
                    }
                }
                Some(UniTextCommand::Clear) => {
                    session.clear_display(true);
                }
            },
            changed = probe_rx.changed() => {
                if changed.is_ok() {
                    let probe = probe_rx.borrow().clone();
                    session.handle_probe_update(&probe);
                    refresh_status(&status, &session.config, &probe, session.active_tier);
                }
            }
            live_ev = live_osc_rx.recv() => match live_ev {
                Some(ev) => session.handle_live_osc_event(ev, Instant::now()),
                None => break,
            },
            _ = wait_resize_debounce(session.pending_viewport) => {
                session.flush_pending_resize();
            }
            event = presentation_rx.recv() => match event {
                Ok(ev) => session.handle_presentation_event(ev, Instant::now()),
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            },
            _ = ticker.tick() => {
                if !session.config.enabled {
                    continue;
                }
                let Some(tier) = session.active_tier else {
                    continue;
                };
                let Some(sock) = socket.as_ref() else {
                    continue;
                };

                let now = Instant::now();
                let packet_to_send = session.next_tick_packet(tier, now);

                if session.need_visible_sync {
                    session.need_visible_sync = false;
                    if let Ok(bytes) = encode_visible_packet(true) {
                        let _ = sock
                            .send_to(
                                &bytes,
                                SocketAddrV4::new(Ipv4Addr::LOCALHOST, session.config.port),
                            )
                            .await;
                    }
                }

                if let Some(pkt) = packet_to_send {
                    let n_bytes = tier.payload_bytes();
                    match encode_osc_bundle(
                        &pkt,
                        n_bytes,
                        session.viewport.send_bank,
                        session.viewport.send_row_page,
                    )
                    .map(|bytes| {
                        (
                            bytes,
                            SocketAddrV4::new(Ipv4Addr::LOCALHOST, session.config.port),
                        )
                    }) {
                        Ok((bytes, addr)) => match sock.send_to(&bytes, addr).await {
                            Ok(_) => {
                                session.shadow.apply(&pkt, n_bytes);
                                if matches!(
                                    pkt.ctrl,
                                    protocol::CTRL_SET_VIEW
                                        | protocol::CTRL_CLEAR_ALL
                                        | protocol::CTRL_NOP
                                ) {
                                    session.active_since_view_refresh = 0;
                                } else if !session.target.is_empty() {
                                    session.active_since_view_refresh =
                                        session.active_since_view_refresh.saturating_add(1);
                                    if session.active_since_view_refresh >= 8 {
                                        session.active_since_view_refresh = 0;
                                        session.shadow.invalidate_view();
                                    }
                                }
                                mark_sent(&status);
                            }
                            Err(error) => {
                                set_error_status(&status, &session.config, error.to_string());
                            }
                        },
                        Err(error) => set_error_status(&status, &session.config, error),
                    }
                }
            }
        }
    }
}

fn source_allowed(source: &str, config: &UniTextConfig, muted: Option<bool>) -> bool {
    match source {
        "microphone" => {
            config.include_microphone && !(config.mute_sync_enabled && muted == Some(true))
        }
        "speaker" | "vrchat_chatbox" => config.include_speaker,
        _ => false,
    }
}

fn apply_presentation_event(
    layout: &mut UniTextLayout,
    target: &mut VirtualScreen,
    event: PresentationEvent,
    config: &UniTextConfig,
    muted: Option<bool>,
) -> bool {
    match event {
        PresentationEvent::RecognitionPartial { source, .. } => {
            if source_allowed(&source, config, muted) {
                layout.set_status(1, target);
            }
            false
        }
        PresentationEvent::TranslationPartial { .. } => {
            layout.set_status(2, target);
            false
        }
        PresentationEvent::RecognitionCancelled {
            utterance_id,
            source,
        } => {
            if source_allowed(&source, config, muted) {
                return layout.cancel_utterance(&utterance_id);
            }
            false
        }
        PresentationEvent::RecognitionReset { .. } => {
            layout.set_status(0, target);
            false
        }
        PresentationEvent::Final {
            utterance_id,
            subtitle,
        } => {
            if !source_allowed(&subtitle.source, config, muted) {
                return false;
            }
            let first_translation = subtitle.translations.first().map(|t| t.text.clone());
            layout.upsert_final(
                utterance_id.as_deref(),
                subtitle.id,
                &subtitle.source,
                subtitle.speaker.as_ref(),
                subtitle.text,
            );
            if let (Some(id), Some(text)) = (subtitle.id, first_translation) {
                layout.update_translation(id, text, true);
            }
            true
        }
        PresentationEvent::TranslationCompleted {
            subtitle_id,
            translation,
            preferred,
        } => layout.update_translation(subtitle_id, translation.text, preferred),
        PresentationEvent::LiveTranslationUpdated { source, snapshot } => {
            if !source_allowed(&source, config, muted) {
                return false;
            }
            layout.upsert_live(
                &snapshot.utterance_id,
                snapshot.source_utterance_id.as_deref(),
                &source,
                snapshot.speaker.as_ref(),
                snapshot.text,
                snapshot.translation,
            );
            true
        }
        PresentationEvent::SpeakerUpdated { speaker } => layout.update_speaker(&speaker),
    }
}

fn resolve_base_viewport(probe: &ProbeState) -> BaseViewport {
    if probe.connection == "connected" && probe.avatar.has_unitext_params {
        probe.avatar.base_viewport
    } else {
        BaseViewport::default()
    }
}

fn resolve_active_tier(config: &UniTextConfig, probe: &ProbeState) -> Option<UniTextTier> {
    if !config.enabled {
        return None;
    }
    UniTextTier::from_str_name(&config.tier).or(probe.avatar.tier)
}

fn refresh_status(
    status_lock: &Arc<Mutex<UniTextRuntimeStatus>>,
    config: &UniTextConfig,
    probe: &ProbeState,
    active_tier: Option<UniTextTier>,
) {
    let mut guard = status_lock.lock().expect("UniText status lock");
    let status = if !config.enabled {
        "disabled"
    } else if active_tier.is_some() {
        "ready"
    } else if probe.connection == "connected" {
        "not_detected"
    } else {
        "discovering"
    };
    guard.enabled = config.enabled;
    guard.status = status.into();
    guard.target = format!("127.0.0.1:{}", config.port);
    guard.detected_tier = probe.avatar.tier.map(|t| t.as_str().into());
    guard.active_tier = active_tier.map(|t| t.as_str().into());
    guard.last_error = if config.enabled && active_tier.is_none() {
        probe.error.clone()
    } else {
        None
    };
}

fn mark_sent(status_lock: &Arc<Mutex<UniTextRuntimeStatus>>) {
    let mut guard = status_lock.lock().expect("UniText status lock");
    guard.last_sent_at = Some(now_iso8601());
    if guard.status == "error" && guard.active_tier.is_some() {
        guard.status = "ready".into();
        guard.last_error = None;
    }
}

fn set_error_status(
    status_lock: &Arc<Mutex<UniTextRuntimeStatus>>,
    config: &UniTextConfig,
    error: String,
) {
    let mut guard = status_lock.lock().expect("UniText status lock");
    guard.enabled = config.enabled;
    guard.target = format!("127.0.0.1:{}", config.port);
    guard.status = "error".into();
    guard.last_error = Some(error);
}

async fn run_udp_osc_listener(
    port: u16,
    mut enabled: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    live_osc_tx: mpsc::Sender<LiveOscParamEvent>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        if !*enabled.borrow() {
            tokio::select! {
                _ = shutdown.changed() => continue,
                _ = enabled.changed() => continue,
            }
        }

        match UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await {
            Ok(socket) => {
                serve_udp_osc_socket(&socket, &mut enabled, &mut shutdown, &live_osc_tx).await;
            }
            Err(error) => {
                tracing::debug!(
                    port,
                    %error,
                    "UniText UDP OSC receive port unavailable; relying on OSCQuery"
                );
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(10)) => {},
                    _ = shutdown.changed() => {},
                    _ = enabled.changed() => {},
                }
            }
        }
    }
}

async fn serve_udp_osc_socket(
    socket: &UdpSocket,
    enabled: &mut watch::Receiver<bool>,
    shutdown: &mut watch::Receiver<bool>,
    live_osc_tx: &mpsc::Sender<LiveOscParamEvent>,
) {
    let mut buf = [0u8; 4096];
    loop {
        if *shutdown.borrow() || !*enabled.borrow() {
            break;
        }
        tokio::select! {
            recv = socket.recv_from(&mut buf) => match recv {
                Ok((len, _)) => {
                    for ev in decode_live_osc_events(&buf[..len]) {
                        let _ = live_osc_tx.try_send(ev);
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "UniText UDP OSC receive failed");
                    break;
                }
            },
            _ = enabled.changed() => {},
            _ = shutdown.changed() => break,
        }
    }
}

async fn run_probe(
    mut enabled: watch::Receiver<bool>,
    mut shutdown: watch::Receiver<bool>,
    state: watch::Sender<ProbeState>,
    live_osc_tx: mpsc::Sender<LiveOscParamEvent>,
) {
    loop {
        if *shutdown.borrow() {
            break;
        }
        if !*enabled.borrow() {
            let _ = state.send(ProbeState::default());
            tokio::select! {
                _ = shutdown.changed() => continue,
                _ = enabled.changed() => continue,
            }
        }

        let _ = state.send(ProbeState::default());
        if let Err(error) =
            discover_and_poll(&mut enabled, &mut shutdown, &state, &live_osc_tx).await
        {
            let _ = state.send(ProbeState {
                connection: "discovering",
                avatar: OscQueryAvatarInfo::default(),
                error: Some(error),
            });
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(3)) => {},
                _ = shutdown.changed() => {},
                _ = enabled.changed() => {},
            }
        }
    }
}

async fn next_ws_message(
    ws_opt: &mut Option<
        tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
    >,
) -> Option<Result<Message, tokio_tungstenite::tungstenite::Error>> {
    match ws_opt {
        Some(ws) => ws.next().await,
        None => std::future::pending().await,
    }
}

async fn discover_and_poll(
    enabled: &mut watch::Receiver<bool>,
    shutdown: &mut watch::Receiver<bool>,
    state: &watch::Sender<ProbeState>,
    live_osc_tx: &mpsc::Sender<LiveOscParamEvent>,
) -> Result<(), String> {
    let daemon = ServiceDaemon::new().map_err(|error| error.to_string())?;
    let services = daemon
        .browse(SERVICE_TYPE)
        .map_err(|error| error.to_string())?;
    let service = loop {
        tokio::select! {
            event = services.recv_async() => match event.map_err(|error| error.to_string())? {
                ServiceEvent::ServiceResolved(info)
                    if info.get_fullname().to_ascii_lowercase().contains("vrchat-client") => break info,
                _ => continue,
            },
            _ = enabled.changed() => {
                let _ = daemon.shutdown();
                return Ok(());
            },
            _ = shutdown.changed() => {
                let _ = daemon.shutdown();
                return Ok(());
            },
        }
    };
    let port = service.get_port();
    let _ = daemon.stop_browse(SERVICE_TYPE);
    let _ = daemon.shutdown();

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|error| error.to_string())?;

    let mut websocket =
        match tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{port}")).await {
            Ok((mut ws, _)) => {
                let mut subscribed = true;
                for path in [OSC_PARAM_WIDTH, OSC_PARAM_HEIGHT, OSC_PARAM_LOCAL_CLEAR] {
                    if ws
                        .send(Message::Text(
                            serde_json::json!({ "COMMAND": "LISTEN", "DATA": path })
                                .to_string()
                                .into(),
                        ))
                        .await
                        .is_err()
                    {
                        subscribed = false;
                        break;
                    }
                }
                subscribed.then_some(ws)
            }
            Err(error) => {
                tracing::debug!(
                    %error,
                    "VRChat OSCQuery WebSocket unavailable; relying on HTTP polling and UDP"
                );
                None
            }
        };

    let mut tick = tokio::time::interval(Duration::from_secs(4));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            _ = tick.tick() => {
                let tree: Value = client
                    .get(format!("http://127.0.0.1:{port}/avatar"))
                    .send()
                    .await
                    .map_err(|error| error.to_string())?
                    .error_for_status()
                    .map_err(|error| error.to_string())?
                    .json()
                    .await
                    .map_err(|error| error.to_string())?;
                let next = ProbeState {
                    connection: "connected",
                    avatar: parse_oscquery_avatar(&tree),
                    error: None,
                };
                state.send_if_modified(|current| {
                    if *current == next {
                        false
                    } else {
                        *current = next;
                        true
                    }
                });
            }
            ws_msg = next_ws_message(&mut websocket) => match ws_msg {
                Some(Ok(Message::Binary(bytes))) => {
                    for ev in decode_live_osc_events(&bytes) {
                        let _ = live_osc_tx.try_send(ev);
                    }
                }
                Some(Ok(Message::Ping(bytes))) => {
                    if let Some(ws) = websocket.as_mut()
                        && ws.send(Message::Pong(bytes)).await.is_err()
                    {
                        websocket = None;
                    }
                }
                Some(Ok(Message::Close(_))) | Some(Err(_)) | None => {
                    websocket = None;
                }
                _ => {}
            },
            _ = enabled.changed() => return Ok(()),
            _ = shutdown.changed() => return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rosc::{OscBundle, OscMessage, OscPacket, OscTime, OscType, encoder};

    #[tokio::test]
    async fn udp_osc_listener_forwards_width_and_height_packets() {
        let receiver = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let addr = receiver.local_addr().unwrap();
        let sender = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();

        let (enabled_tx, mut enabled_rx) = watch::channel(true);
        let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
        let (tx, mut rx) = mpsc::channel(16);

        let handle = tokio::spawn(async move {
            serve_udp_osc_socket(&receiver, &mut enabled_rx, &mut shutdown_rx, &tx).await;
        });

        let msg = encoder::encode(&OscPacket::Message(OscMessage {
            addr: OSC_PARAM_WIDTH.into(),
            args: vec![OscType::Float(-0.5)],
        }))
        .unwrap();
        sender.send_to(&msg, addr).await.unwrap();

        let bundle = encoder::encode(&OscPacket::Bundle(OscBundle {
            timetag: OscTime {
                seconds: 0,
                fractional: 1,
            },
            content: vec![OscPacket::Message(OscMessage {
                addr: OSC_PARAM_HEIGHT.into(),
                args: vec![OscType::Float(0.5)],
            })],
        }))
        .unwrap();
        sender.send_to(&bundle, addr).await.unwrap();

        let first = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let second = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(first, LiveOscParamEvent::Width(-0.5));
        assert_eq!(second, LiveOscParamEvent::Height(0.5));

        let _ = enabled_tx.send(false);
        let _ = shutdown_tx.send(true);
        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .unwrap()
            .unwrap();
    }

    #[test]
    fn live_resize_reflows_history_and_clears_on_row_count_change() {
        let config = UniTextConfig {
            enabled: true,
            ..UniTextConfig::default()
        };
        let initial_tree = serde_json::json!({
            "CONTENTS": {
                "change": {
                    "FULL_PATH": "/avatar/change",
                    "VALUE": ["avtr_resize_test"]
                },
                "parameters": {
                    "CONTENTS": {
                        "UniText_ProtoVer": { "VALUE": [2] },
                        "UniText_D7": { "FULL_PATH": "/avatar/parameters/UniText_D7" },
                        "UniText_Bank": { "FULL_PATH": "/avatar/parameters/UniText_Bank" },
                        "UniText_RowPage": { "FULL_PATH": "/avatar/parameters/UniText_RowPage" },
                        "UniText_Rows": { "VALUE": [6] },
                        "UniText_RowPx8": { "VALUE": [48] },
                        "UniText_Width": { "VALUE": [0.0] },
                        "UniText_Height": { "VALUE": [0.0] }
                    }
                }
            }
        });
        let probe = ProbeState {
            connection: "connected",
            avatar: parse_oscquery_avatar(&initial_tree),
            error: None,
        };

        let mut session = WorkerSession::new(config, &ProbeState::default());
        session.handle_probe_update(&probe);
        assert_eq!(session.viewport.cols, 64);
        assert_eq!(session.viewport.rows, 6);
        assert_eq!(session.viewport.row_px, 384);
        assert!(session.viewport.send_bank);
        assert!(!session.viewport.send_row_page);

        let now = Instant::now();
        for i in 1..=3 {
            session.handle_presentation_event(
                PresentationEvent::Final {
                    utterance_id: Some(format!("u{i}")),
                    subtitle: crate::models::Subtitle {
                        id: Some(i),
                        conversation_id: None,
                        text: format!(
                            "第{i}句实时字幕：支持宽窄缩放与行高调整的统一版本协议测试内容！"
                        ),
                        speaker: None,
                        language: Some("zh".into()),
                        started_at: None,
                        ended_at: None,
                        source: "speaker".into(),
                        created_at: "00:00:00".into(),
                        translations: vec![crate::models::SubtitleTranslation {
                            source_group: None,
                            text: format!(
                                "Line {i}: Unified UniText v1 supports live width and height reflow in VRChat!"
                            ),
                            source_language: Some("zh".into()),
                            target_language: "en".into(),
                            provider: "test".into(),
                            model: None,
                            created_at: "00:00:00".into(),
                        }],
                    },
                },
                now,
            );
        }

        // Drain initial 6x384 packets to shadow CRT.
        let tier = session.active_tier.unwrap();
        while let Some(pkt) = session.next_tick_packet(tier, now) {
            session.shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(session.shadow.read_geometry(), Some((6, 384)));
        assert_eq!(
            session.shadow.read_visual_cells(64, 6),
            session.target.read_visual_cells()
        );

        // 1. Narrow width via live OSC (b_px=384, min=192 -> w = -2/3 gives 256 px, rows unchanged = 6).
        session.handle_live_osc_event(LiveOscParamEvent::Width(-2.0 / 3.0), now);
        assert!(session.pending_viewport.is_some());
        session.flush_pending_resize();
        assert_eq!(session.viewport.rows, 6);
        assert_eq!(session.viewport.row_px, 256);
        assert!(!session.force_clear_all);
        while let Some(pkt) = session.next_tick_packet(tier, now) {
            session.shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(session.shadow.read_geometry(), Some((6, 256)));
        assert_eq!(
            session.shadow.read_visual_cells(64, 6),
            session.target.read_visual_cells()
        );
        for r in 0..session.viewport.rows {
            assert!(session.target.visual_row_px(r) <= 256);
        }

        // 2. Widen via live OSC (w = 1.0 -> 576 px, rows still 6).
        session.handle_live_osc_event(LiveOscParamEvent::Width(1.0), now);
        session.flush_pending_resize();
        assert_eq!(session.viewport.rows, 6);
        assert_eq!(session.viewport.row_px, 576);
        assert!(!session.force_clear_all);
        while let Some(pkt) = session.next_tick_packet(tier, now) {
            session.shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(session.shadow.read_geometry(), Some((6, 576)));
        assert_eq!(
            session.shadow.read_visual_cells(64, 6),
            session.target.read_visual_cells()
        );
        for r in 0..session.viewport.rows {
            assert!(session.target.visual_row_px(r) <= 576);
        }

        // 3. Stale HTTP poll with old (0.0, 0.0) must NOT snap back live axes.
        session.handle_probe_update(&probe);
        assert_eq!(session.viewport.row_px, 576);
        assert_eq!(session.viewport.rows, 6);

        // 4. Expand height via live OSC (h = 2/3 of 6..12 -> 10 rows): must force CMD_CLEAR_ALL
        //    resetting topRow=0, emit CMD_SET_GEOMETRY (10, 576), and enable UniText_RowPage for rows 6..9.
        session.handle_live_osc_event(LiveOscParamEvent::Height(2.0 / 3.0), now);
        session.flush_pending_resize();
        assert_eq!(session.viewport.rows, 10);
        assert!(session.viewport.send_row_page);
        assert_eq!(session.target.top_row, 0);
        assert!(session.force_clear_all);

        let first = session.next_tick_packet(tier, now).unwrap();
        assert_eq!(first.ctrl, protocol::CTRL_CLEAR_ALL);
        session.shadow.apply(&first, tier.payload_bytes());

        let second = session.next_tick_packet(tier, now).unwrap();
        assert_eq!(second.ctrl, protocol::CTRL_SET_GEOMETRY);
        session.shadow.apply(&second, tier.payload_bytes());

        let mut saw_row_page_1 = false;
        while let Some(pkt) = session.next_tick_packet(tier, now) {
            if pkt.row_page == 1 {
                saw_row_page_1 = true;
            }
            session.shadow.apply(&pkt, tier.payload_bytes());
        }
        assert_eq!(session.shadow.read_geometry(), Some((10, 576)));
        assert!(
            saw_row_page_1,
            "10-row layout must write rows 6..9 with row_page=1"
        );
        assert_eq!(session.shadow.read_top_row(), 0);
        assert_eq!(
            session.shadow.read_visual_cells(64, 10),
            session.target.read_visual_cells()
        );
        for r in 0..10 {
            assert!(session.target.visual_row_px(r) <= 576);
        }
    }

    #[test]
    fn speaker_updated_event_refreshes_header_row_in_worker_session() {
        let config = UniTextConfig {
            enabled: true,
            ..UniTextConfig::default()
        };
        let mut session = WorkerSession::new(config, &ProbeState::default());
        let now = Instant::now();
        let unresolved = crate::models::SpeakerIdentity {
            id: "track:1".into(),
            index: 0,
            display_name: None,
            user_id: None,
        };

        session.handle_presentation_event(
            PresentationEvent::Final {
                utterance_id: Some("u1".into()),
                subtitle: crate::models::Subtitle {
                    id: Some(1),
                    conversation_id: None,
                    text: "Hello".into(),
                    speaker: Some(unresolved),
                    language: Some("en".into()),
                    started_at: None,
                    ended_at: None,
                    source: "speaker".into(),
                    created_at: "00:00:00".into(),
                    translations: Vec::new(),
                },
            },
            now,
        );

        let read_row = |target: &VirtualScreen, row: usize| -> String {
            target
                .visual_row_cells(row)
                .into_iter()
                .take_while(|&cp| cp != 0)
                .map(|cp| char::from_u32(cp as u32).unwrap_or('\u{FFFD}'))
                .collect()
        };
        assert_eq!(read_row(&session.target, 0), "#1：");
        assert_eq!(read_row(&session.target, 1), "Hello");

        session.handle_presentation_event(
            PresentationEvent::SpeakerUpdated {
                speaker: crate::models::SpeakerIdentity {
                    id: "track:1".into(),
                    index: 0,
                    display_name: Some("Alice".into()),
                    user_id: Some("usr_alice".into()),
                },
            },
            now,
        );
        assert_eq!(read_row(&session.target, 0), "Alice：");
        assert_eq!(read_row(&session.target, 1), "Hello");
    }
}
