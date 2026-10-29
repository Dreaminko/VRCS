use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::ocr_status::OcrStatus;
use serde::Serialize;
use tauri::Manager as _;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, watch};
use vrcs_core::{PresentationEvent, VrOcrConfig, VrOverlayConfig};

use super::backend::{OpenVrBackend, OverlayKind};
use super::control_protocol::{ControlAction, ControlHub, ControlSource};
use super::dashboard::{DashboardPointerEvent, DashboardState, DashboardViewModel};
use super::placement::{Mode, Placement};
use super::presentation::{
    HeadsetPresentation, MessageSide, PresentationContent, PresentationFrame, WristMessage,
    WristPresentation,
};
use super::process::{steamvr_pid, SteamVrSession};

pub const STATUS_EVENT: &str = "vr-overlay-status-changed";
const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
pub(super) const SURFACE_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_700);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_QUEUE_CAPACITY: usize = 256;
const DROP_WARNING_INTERVAL: Duration = Duration::from_secs(5);

pub(super) fn dispatch_control(
    app: &AppHandle,
    controls: &Mutex<ControlHub>,
    source: ControlSource,
    action: ControlAction,
) {
    let request = controls
        .lock()
        .ok()
        .and_then(|mut hub| hub.begin(source, action));
    if let Some(request) = request {
        if let Err(error) = app.emit_to("main", super::control_protocol::REQUEST_EVENT, &request) {
            if let Ok(mut hub) = controls.lock() {
                hub.finish(&request, Some(error.to_string()));
            }
        }
    }
}

pub(super) fn dispatch_player_display(
    app: &AppHandle,
    controls: &Mutex<ControlHub>,
    source: ControlSource,
    expected: (u32, u32),
    action: super::player_display::Action,
) {
    let request = controls
        .lock()
        .ok()
        .and_then(|mut hub| hub.begin_player_display(source, expected, action));
    if let Some(request) = request {
        if let Err(error) = app.emit_to("main", super::player_display::REQUEST_EVENT, &request) {
            if let Ok(mut hub) = controls.lock() {
                hub.finish_player_display(&request, Some(error.to_string()));
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SampleKind {
    Headset,
    Wrist,
}

impl SampleKind {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "headset" => Ok(Self::Headset),
            "wrist" => Ok(Self::Wrist),
            _ => Err("Unknown VR Overlay kind".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    Unsupported,
    #[allow(dead_code)]
    Disabled,
    WaitingRuntime,
    Initializing,
    Ready,
    Reconnecting,
    #[allow(dead_code)]
    Error,
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    Disabled,
    Creating,
    ReadyHidden,
    Visible,
    Fading,
    DeviceUnavailable,
    #[allow(dead_code)]
    Recreating,
    Error,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ResourceStatus {
    pub state: ResourceState,
    pub sample_visible: bool,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct WristStatus {
    pub state: ResourceState,
    pub sample_visible: bool,
    pub bound_role: Option<String>,
    pub tracked_device_available: bool,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardStatus {
    pub state: ResourceState,
    pub visible: bool,
    pub last_error_code: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VrOverlayStatus {
    pub state: RuntimeState,
    pub runtime_installed: bool,
    pub hmd_present: bool,
    pub last_connected_at: Option<String>,
    pub reconnect_attempt: u32,
    pub headset: ResourceStatus,
    pub wrist: WristStatus,
    pub ocr: OcrStatus,
    pub dashboard: DashboardStatus,
    pub last_error_detail: Option<String>,
}

impl VrOverlayStatus {
    pub fn initial() -> Self {
        let supported = cfg!(windows);
        Self {
            state: if supported {
                RuntimeState::Initializing
            } else {
                RuntimeState::Unsupported
            },
            runtime_installed: false,
            hmd_present: false,
            last_connected_at: None,
            reconnect_attempt: 0,
            headset: ResourceStatus {
                state: ResourceState::Disabled,
                sample_visible: false,
                last_error_code: None,
            },
            wrist: WristStatus {
                state: ResourceState::Disabled,
                sample_visible: false,
                bound_role: None,
                tracked_device_available: false,
                last_error_code: None,
            },
            ocr: OcrStatus::default(),
            dashboard: DashboardStatus {
                state: ResourceState::Disabled,
                visible: false,
                last_error_code: None,
            },
            last_error_detail: None,
        }
    }
}

#[derive(Default)]
struct PendingControl {
    retry: bool,
    ocr_bindings: bool,
    headset_sample: Option<bool>,
    wrist_sample: Option<bool>,
    placement_mode: Option<Mode>,
}

pub struct Manager {
    app: AppHandle,
    pub(super) controls: Arc<Mutex<ControlHub>>,
    pub(super) placement: Arc<Mutex<Placement>>,
    status: Arc<Mutex<VrOverlayStatus>>,
    event_sender: Mutex<Option<SyncSender<PresentationEvent>>>,
    pending_control: Arc<Mutex<PendingControl>>,
    latest_config: Arc<Mutex<Option<(VrOverlayConfig, VrOcrConfig)>>>,
    latest_dashboard: Arc<Mutex<Option<Arc<DashboardViewModel>>>>,
    stopping: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    bridges: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
}

impl Manager {
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub(super) fn preview_view(&self) -> Option<DashboardViewModel> {
        self.latest_dashboard.lock().ok()?.as_deref().cloned()
    }
    pub fn revoke_controls(&self) {
        if let Ok(mut hub) = self.controls.lock() {
            hub.revoke();
        }
        if let Ok(mut placement) = self.placement.lock() {
            placement.cancel();
        }
    }
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            controls: Arc::new(Mutex::new(ControlHub::default())),
            placement: Arc::new(Mutex::new(Placement::default())),
            status: Arc::new(Mutex::new(VrOverlayStatus::initial())),
            event_sender: Mutex::new(None),
            pending_control: Arc::new(Mutex::new(PendingControl::default())),
            latest_config: Arc::new(Mutex::new(None)),
            latest_dashboard: Arc::new(Mutex::new(None)),
            stopping: Arc::new(AtomicBool::new(false)),
            worker: Mutex::new(None),
            bridges: Mutex::new(Vec::new()),
        }
    }

    pub fn status(&self) -> Result<VrOverlayStatus, String> {
        self.status
            .lock()
            .map(|status| status.clone())
            .map_err(|error| error.to_string())
    }

    pub fn open_ocr_bindings(&self) -> Result<(), String> {
        let status = self.status()?;
        if !cfg!(windows) || !status.hmd_present || !status.runtime_installed {
            return Err("SteamVR headset is unavailable".into());
        }
        self.update_control(|control| control.ocr_bindings = true)
    }

    pub fn update_dashboard(&self, view: DashboardViewModel) -> Result<(), String> {
        *self
            .latest_dashboard
            .lock()
            .map_err(|error| error.to_string())? = Some(Arc::new(view));
        Ok(())
    }

    pub fn start(
        &self,
        mut events: broadcast::Receiver<PresentationEvent>,
        mut config: watch::Receiver<(VrOverlayConfig, VrOcrConfig)>,
        features: watch::Receiver<vrcs_core::FeatureConfig>,
        ocr_service: Option<vrcs_core::ocr::VrOcrService>,
        vision_service: vrcs_core::vision::VisionService,
    ) -> Result<(), String> {
        #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
        if super::preview::mode().is_some() {
            return Ok(());
        }
        tracing::info!("Starting VR Overlay manager");
        self.stop();
        let initial_config = config.borrow().clone();
        let (event_sender, event_receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        self.stopping.store(false, Ordering::Release);

        let app = self.app.clone();
        let status = self.status.clone();
        let pending_control = self.pending_control.clone();
        let latest_config = self.latest_config.clone();
        let latest_dashboard = self.latest_dashboard.clone();
        let controls = self.controls.clone();
        let placement = self.placement.clone();
        let stopping = self.stopping.clone();
        let worker = std::thread::Builder::new()
            .name("vrcs-vr-overlay".into())
            .spawn(move || {
                worker_loop(
                    app,
                    status,
                    event_receiver,
                    pending_control,
                    latest_config,
                    latest_dashboard,
                    controls,
                    placement,
                    stopping,
                    initial_config,
                    features,
                    ocr_service,
                    vision_service,
                )
            })
            .map_err(|error| format!("Failed to start VR Overlay thread: {error}"))?;
        *self
            .event_sender
            .lock()
            .map_err(|error| error.to_string())? = Some(event_sender.clone());
        *self.worker.lock().map_err(|error| error.to_string())? = Some(worker);

        let event_bridge = tauri::async_runtime::spawn(async move {
            let mut dropped = 0_u64;
            let mut last_warning = None;
            loop {
                match events.recv().await {
                    Ok(event) => match event_sender.try_send(event) {
                        Ok(()) => {}
                        Err(TrySendError::Full(_)) => {
                            dropped = dropped.saturating_add(1);
                            if last_warning
                                .is_none_or(|last: Instant| last.elapsed() >= DROP_WARNING_INTERVAL)
                            {
                                tracing::warn!(
                                    dropped,
                                    "VR Overlay dropped stale presentation events"
                                );
                                dropped = 0;
                                last_warning = Some(Instant::now());
                            }
                        }
                        Err(TrySendError::Disconnected(_)) => break,
                    },
                    Err(broadcast::error::RecvError::Lagged(count)) => {
                        tracing::warn!(count, "VR Overlay presentation events lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let latest_config = self.latest_config.clone();
        let config_bridge = tauri::async_runtime::spawn(async move {
            while config.changed().await.is_ok() {
                let next = config.borrow().clone();
                match latest_config.lock() {
                    Ok(mut pending) => *pending = Some(next),
                    Err(error) => {
                        tracing::warn!(%error, "VR Overlay config lock poisoned");
                        break;
                    }
                }
            }
        });
        *self.bridges.lock().map_err(|error| error.to_string())? =
            vec![event_bridge, config_bridge];
        Ok(())
    }

    pub fn retry(&self) -> Result<(), String> {
        self.update_control(|control| control.retry = true)
    }

    pub fn place_dialogue(&self, mode: Mode) -> Result<(), String> {
        self.update_control(|control| control.placement_mode = Some(mode))
    }

    pub fn set_sample(&self, kind: SampleKind, visible: bool) -> Result<(), String> {
        self.update_control(|control| match kind {
            SampleKind::Headset => control.headset_sample = Some(visible),
            SampleKind::Wrist => control.wrist_sample = Some(visible),
        })
    }

    pub fn stop(&self) {
        if let Ok(mut bridges) = self.bridges.lock() {
            for task in bridges.drain(..) {
                task.abort();
            }
        }
        self.stopping.store(true, Ordering::Release);
        if let Ok(mut sender) = self.event_sender.lock() {
            sender.take();
        }
        if let Ok(mut control) = self.pending_control.lock() {
            *control = PendingControl::default();
        }
        if let Ok(mut config) = self.latest_config.lock() {
            config.take();
        }
        if let Ok(mut worker) = self.worker.lock() {
            if let Some(worker) = worker.take() {
                if worker.join().is_err() {
                    tracing::warn!("VR Overlay thread panicked during shutdown");
                }
            }
        }
    }

    fn update_control(&self, update: impl FnOnce(&mut PendingControl)) -> Result<(), String> {
        let sender = self
            .event_sender
            .lock()
            .map_err(|error| error.to_string())?;
        if sender.is_none() {
            return Err("VR Overlay is waiting for Core startup".into());
        }
        let mut control = self
            .pending_control
            .lock()
            .map_err(|error| error.to_string())?;
        update(&mut control);
        Ok(())
    }
}

struct WorkerState {
    caption_status: super::caption_status::CaptionStatus,
    controls: Arc<Mutex<ControlHub>>,
    #[cfg(windows)]
    ui: super::ui_runtime::UiRuntime,
    #[cfg(windows)]
    player_audio: super::player_audio_ui::Runtime,
    config: VrOverlayConfig,
    ocr_config: VrOcrConfig,
    headset: HeadsetPresentation,
    wrist: WristPresentation,
    headset_sample: bool,
    wrist_sample: bool,
    #[cfg(windows)]
    headset_surface: super::surface::SurfaceRenderer,
    backend: Option<OpenVrBackend>,
    session: SteamVrSession,
    dashboard_view: Option<Arc<DashboardViewModel>>,
    dashboard: DashboardState,
    #[cfg(windows)]
    dashboard_surface: super::surface::SurfaceRenderer,
    #[cfg(windows)]
    dashboard_pressed_key: Option<super::surface::SurfaceKey>,
    dashboard_retry_at: Option<Instant>,
    next_reconnect: Instant,
    next_probe: Instant,
    #[cfg(windows)]
    ocr: super::ocr_runtime::OcrRuntime,
}

impl WorkerState {
    #[cfg(windows)]
    fn reset_ui(&mut self) {
        self.ui.reset();
        self.player_audio.reset();
    }

    fn reset_dashboard(&mut self) {
        #[cfg(windows)]
        {
            self.dashboard_surface.reset();
            self.dashboard_pressed_key = None;
        }
        self.dashboard.clear_pointer();
        self.dashboard_retry_at = None;
    }

    fn reset_headset(&mut self) {
        #[cfg(windows)]
        self.headset_surface.reset();
    }
}

// Keep the existing worker spawn boundary explicit.
#[allow(clippy::too_many_arguments)]
fn worker_loop(
    app: AppHandle,
    shared_status: Arc<Mutex<VrOverlayStatus>>,
    event_receiver: Receiver<PresentationEvent>,
    pending_control: Arc<Mutex<PendingControl>>,
    latest_config: Arc<Mutex<Option<(VrOverlayConfig, VrOcrConfig)>>>,
    latest_dashboard: Arc<Mutex<Option<Arc<DashboardViewModel>>>>,
    controls: Arc<Mutex<ControlHub>>,
    placement: Arc<Mutex<Placement>>,
    stopping: Arc<AtomicBool>,
    config: (VrOverlayConfig, VrOcrConfig),
    features: watch::Receiver<vrcs_core::FeatureConfig>,
    ocr_service: Option<vrcs_core::ocr::VrOcrService>,
    vision_service: vrcs_core::vision::VisionService,
) {
    #[cfg(windows)]
    let ocr_directory = app
        .path()
        .app_config_dir()
        .ok()
        .map(|path| path.join("steamvr-ocr"));
    #[cfg(windows)]
    let ocr = {
        let mut ocr = super::ocr_runtime::OcrRuntime::new(ocr_service, ocr_directory);
        ocr.vision.set_service(vision_service);
        ocr
    };
    let (config, ocr_config) = config;
    let mut state = WorkerState {
        caption_status: Default::default(),
        controls,
        #[cfg(windows)]
        ui: super::ui_runtime::UiRuntime::new(placement),
        #[cfg(windows)]
        player_audio: Default::default(),
        config,
        ocr_config,
        headset: HeadsetPresentation::default(),
        wrist: WristPresentation::default(),
        headset_sample: false,
        wrist_sample: false,
        #[cfg(windows)]
        headset_surface: super::surface::SurfaceRenderer::new(super::surface::SurfaceId::Headset),
        backend: None,
        session: SteamVrSession::default(),
        dashboard_view: None,
        dashboard: DashboardState::default(),
        #[cfg(windows)]
        dashboard_surface: super::surface::SurfaceRenderer::new(
            super::surface::SurfaceId::Dashboard,
        ),
        #[cfg(windows)]
        dashboard_pressed_key: None,
        dashboard_retry_at: None,
        next_reconnect: Instant::now(),
        next_probe: Instant::now(),
        #[cfg(windows)]
        ocr,
    };
    let mut status = VrOverlayStatus::initial();
    let mut next_tick = Instant::now();
    #[cfg(windows)]
    let mut next_pointer = Instant::now();

    loop {
        let started = Instant::now();
        if stopping.load(Ordering::Acquire) {
            status.state = RuntimeState::ShuttingDown;
            update_status(&app, &shared_status, &status);
            #[cfg(windows)]
            {
                state
                    .headset_surface
                    .stop(&app, state.backend.as_mut(), OverlayKind::Headset);
                state
                    .dashboard_surface
                    .stop(&app, state.backend.as_mut(), OverlayKind::Dashboard);
                state.ui.stop_surfaces(&app, state.backend.as_mut());
                state
                    .player_audio
                    .stop_surfaces(&app, state.backend.as_mut());
                state.ocr.stop_surfaces(&app, state.backend.as_mut());
            }
            if let Some(mut backend) = state.backend.take() {
                backend.hide_all();
            }
            break;
        }

        if started < next_tick {
            let mut wake = next_tick;
            #[cfg(windows)]
            if state.ui.fast_tick()
                || state.player_audio.active()
                || state.ocr.status.wrist_state == super::ocr_status::OcrWristState::Visible
            {
                if let Some(backend) = state.backend.as_mut() {
                    if started >= next_pointer {
                        if state.player_audio.active() {
                            state.player_audio.update_pointer(&app, backend);
                        } else {
                            state.ui.update_pointer(&app, backend);
                        }
                        next_pointer = Instant::now() + backend.cursor_interval();
                    }
                    wake = wake.min(next_pointer);
                }
            }
            if let Some(delay) = wake.checked_duration_since(Instant::now()) {
                std::thread::sleep(delay);
            }
            continue;
        }

        if let Ok(mut pending) = latest_config.lock() {
            if let Some((config, ocr_config)) = pending.take() {
                #[cfg(windows)]
                if state.ocr_config != ocr_config && !state.ocr.vision.visible() {
                    state.ocr.clear();
                }
                state.wrist.set_max_entries(config.wrist.max_entries);
                state.headset.set_show_partials(
                    config.headset.show_partials || config.headset.show_translation_partials,
                );
                state.wrist.set_show_partials(
                    config.wrist.show_partials || config.wrist.show_translation_partials,
                );
                if !config.enabled || !config.headset.enabled {
                    state.headset_sample = false;
                }
                if !config.enabled || !config.wrist.enabled {
                    state.wrist_sample = false;
                }
                state.config = config;
                state.ocr_config = ocr_config;
                status.last_error_detail = None;
            }
        }
        if let Ok(mut pending) = latest_dashboard.lock() {
            if let Some(view) = pending.take() {
                if state.dashboard_view.as_ref() != Some(&view) {
                    #[cfg(windows)]
                    {
                        state.ocr.progress_labels = view.labels.ocr_progress.clone();
                        state
                            .ocr
                            .set_reader_labels(view.labels.wrist_reader.clone());
                    }
                    state
                        .caption_status
                        .update(view.caption_notice.clone(), Instant::now());
                    state.dashboard.update_view(&view);
                    state.dashboard_view = Some(view);
                }
            }
        }
        #[cfg(windows)]
        let mut open_ocr_bindings = false;
        let mut placement_mode = None;
        if let Ok(mut pending) = pending_control.lock() {
            let control = std::mem::take(&mut *pending);
            placement_mode = control.placement_mode;
            #[cfg(windows)]
            {
                open_ocr_bindings = control.ocr_bindings;
            }
            if control.retry {
                if let Some(mut backend) = state.backend.take() {
                    backend.hide_all();
                }
                #[cfg(windows)]
                {
                    state.ocr.clear();
                    state.reset_ui();
                }
                state.reset_headset();
                state.reset_dashboard();
                #[cfg(windows)]
                app.state::<super::surface::SurfaceHub>().retry(&app);
                state.next_reconnect = Instant::now();
                state.next_probe = Instant::now();
                status.reconnect_attempt = 0;
                status.last_error_detail = None;
            }
            if let Some(visible) = control.headset_sample {
                state.headset_sample = visible;
            }
            if let Some(visible) = control.wrist_sample {
                state.wrist_sample = visible;
            }
        }
        // Send uncapped candidates: the WebViews share the desktop visibility
        // policy and apply player/history limits after filtering original text.
        state.headset.set_player_limit(Some(0));
        state.wrist.set_player_limit(Some(0));
        for event in event_receiver.try_iter().take(EVENT_QUEUE_CAPACITY) {
            let now = Instant::now();
            state
                .headset
                .apply(event.clone(), now, &state.config.headset);
            state.wrist.apply(event, now, &state.config.wrist);
        }

        let available = features.borrow().vr_overlay;
        #[cfg(windows)]
        if let Some(mode) = placement_mode {
            if let Some(backend) = state.backend.as_ref() {
                state.ui.place(&app, backend, &state.controls, mode);
            }
        }
        // The panic hook records details; keep the worker alive and reconnect.
        let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tick(&app, &mut state, &mut status, available)
        }));
        if ticked.is_err() {
            recover_from_panic(&mut state, &mut status);
        }
        #[cfg(windows)]
        if open_ocr_bindings
            && available
            && (features.borrow().ocr || features.borrow().visual_question_answering)
        {
            state.ocr.clear();
            let result = state
                .backend
                .as_mut()
                .ok_or("SteamVR is unavailable".to_string())
                .and_then(|backend| {
                    backend.hide_ocr();
                    backend.open_ocr_bindings()
                });
            if let Err(error) = result {
                state.ocr.status.last_error = Some(error);
            }
        }
        #[cfg(windows)]
        {
            state
                .headset_surface
                .finish_tick(&app, state.backend.as_mut(), OverlayKind::Headset);
            state.dashboard_surface.finish_tick(
                &app,
                state.backend.as_mut(),
                OverlayKind::Dashboard,
            );
            state.ui.finish_surfaces(&app, state.backend.as_mut());
            state
                .player_audio
                .finish_surfaces(&app, state.backend.as_mut());
            state.ocr.finish_surfaces(&app, state.backend.as_mut());
            status.ocr = state.ocr.status.clone();
        }
        update_status(&app, &shared_status, &status);
        if let Ok(mut hub) = state.controls.lock() {
            let ready = |resource: &ResourceState| {
                matches!(
                    resource,
                    ResourceState::ReadyHidden | ResourceState::Visible | ResourceState::Fading
                )
            };
            hub.set_overlay_effective(
                status.state == RuntimeState::Ready && ready(&status.wrist.state),
                status.state == RuntimeState::Ready && ready(&status.headset.state),
            );
        }
        #[cfg(windows)]
        let ui_active = state.ui.fast_tick()
            || state.player_audio.active()
            || state.ocr.status.wrist_state == super::ocr_status::OcrWristState::Visible;
        #[cfg(not(windows))]
        let ui_active = false;
        let interval = if status.dashboard.visible || ui_active {
            SURFACE_FRAME_INTERVAL
        } else {
            UPDATE_INTERVAL
        };
        next_tick = started + interval;
        #[cfg(windows)]
        if let Some(backend) = state.backend.as_mut() {
            if Instant::now() >= next_pointer || !ui_active {
                if state.player_audio.active() {
                    state.player_audio.update_pointer(&app, backend);
                } else {
                    state.ui.update_pointer(&app, backend);
                }
                next_pointer = Instant::now() + backend.cursor_interval();
            }
        }
    }
}

fn recover_from_panic(state: &mut WorkerState, status: &mut VrOverlayStatus) {
    tracing::error!("VR Overlay worker panicked; reconnecting");
    #[cfg(windows)]
    {
        state.ocr.clear();
        state.reset_ui();
    }
    drop(state.backend.take());
    state.reset_headset();
    state.reset_dashboard();
    state.next_reconnect = Instant::now() + RECONNECT_INTERVAL;
    status.state = RuntimeState::Reconnecting;
    status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
    status.last_error_detail = Some("VR Overlay worker panicked".into());
}

fn tick(app: &AppHandle, state: &mut WorkerState, status: &mut VrOverlayStatus, available: bool) {
    if state
        .backend
        .as_mut()
        .is_some_and(OpenVrBackend::quit_requested)
    {
        tracing::info!("SteamVR requested quit; detaching OpenVR and keeping VRCS running");
        state.session.quit();
        drop(state.backend.take());
        #[cfg(windows)]
        {
            state.ocr.unavailable(state.ocr_config.enabled);
            state.reset_ui();
        }
        state.reset_headset();
        state.reset_dashboard();
        state.next_reconnect = Instant::now() + RECONNECT_INTERVAL;
        status.state = RuntimeState::WaitingRuntime;
        status.hmd_present = false;
        status.reconnect_attempt = 0;
        status.last_error_detail = None;
        status.headset.state = if state.config.enabled && state.config.headset.enabled {
            ResourceState::ReadyHidden
        } else {
            ResourceState::Disabled
        };
        status.headset.last_error_code = None;
        status.wrist.state = if state.config.enabled && state.config.wrist.enabled {
            ResourceState::DeviceUnavailable
        } else {
            ResourceState::Disabled
        };
        status.wrist.last_error_code = None;
        status.wrist.tracked_device_available = false;
        status.wrist.bound_role = None;
        status.dashboard.state = ResourceState::DeviceUnavailable;
        status.dashboard.visible = false;
        status.dashboard.last_error_code = None;
        return;
    }

    if !available {
        #[cfg(windows)]
        {
            state.ocr.unavailable(false);
            state.reset_ui();
        }
        if let Some(mut backend) = state.backend.take() {
            backend.hide_all();
        }
        state.headset_sample = false;
        state.wrist_sample = false;
        state.headset = HeadsetPresentation::default();
        state.wrist = WristPresentation::default();
        state.reset_headset();
        state.reset_dashboard();
        status.state = RuntimeState::Disabled;
        status.headset.state = ResourceState::Disabled;
        status.wrist.state = ResourceState::Disabled;
        status.dashboard.state = ResourceState::Disabled;
        status.dashboard.visible = false;
        status.headset.sample_visible = false;
        status.wrist.sample_visible = false;
        status.wrist.tracked_device_available = false;
        status.wrist.bound_role = None;
        return;
    }
    // These probes read openvrpaths and may load vrclient.dll, so avoid running them per frame.
    if Instant::now() >= state.next_probe {
        status.runtime_installed = OpenVrBackend::runtime_installed();
        status.hmd_present = OpenVrBackend::hmd_present();
        state.next_probe = Instant::now() + RECONNECT_INTERVAL;
    }
    status.headset.sample_visible = state.headset_sample;
    status.wrist.sample_visible = state.wrist_sample;

    if !cfg!(windows) {
        status.state = RuntimeState::Unsupported;
        return;
    }
    let any_enabled =
        state.config.enabled && (state.config.headset.enabled || state.config.wrist.enabled);
    let ocr_enabled = state.ocr_config.enabled;
    #[cfg(windows)]
    let vision_enabled = state.ocr.vision.enabled();
    #[cfg(not(windows))]
    let vision_enabled = false;
    if !status.runtime_installed || !status.hmd_present {
        #[cfg(windows)]
        {
            state.ocr.unavailable(ocr_enabled);
            state.reset_ui();
        }
        if let Some(mut backend) = state.backend.take() {
            backend.hide_all();
        }
        state.reset_headset();
        state.reset_dashboard();
        status.state = RuntimeState::WaitingRuntime;
        status.headset.state = if state.config.headset.enabled {
            ResourceState::ReadyHidden
        } else {
            ResourceState::Disabled
        };
        status.wrist.state = if state.config.wrist.enabled {
            ResourceState::DeviceUnavailable
        } else {
            ResourceState::Disabled
        };
        status.wrist.tracked_device_available = false;
        status.dashboard.state = ResourceState::DeviceUnavailable;
        status.dashboard.visible = false;
        return;
    }

    if state.backend.is_none() && Instant::now() >= state.next_reconnect {
        #[cfg(windows)]
        state.ocr.unavailable(ocr_enabled);
        let pid = steamvr_pid();
        if !state.session.can_connect(pid) {
            status.state = RuntimeState::WaitingRuntime;
            status.reconnect_attempt = 0;
            status.last_error_detail = None;
            state.next_reconnect = Instant::now() + RECONNECT_INTERVAL;
            return;
        }
        status.state = if status.reconnect_attempt == 0 {
            RuntimeState::Initializing
        } else {
            RuntimeState::Reconnecting
        };
        match OpenVrBackend::connect() {
            Ok(backend) => {
                state
                    .session
                    .connected(pid.expect("SteamVR process checked"));
                state.backend = Some(backend);
                #[cfg(windows)]
                state.reset_ui();
                status.state = RuntimeState::Ready;
                status.last_connected_at = Some(unix_timestamp());
                status.reconnect_attempt = 0;
                status.last_error_detail = None;
                state.reset_headset();
                state.reset_dashboard();
                #[cfg(windows)]
                state.ocr.reset_surfaces();
            }
            Err(error) => {
                status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
                tracing::warn!(
                    error = %error,
                    reconnect_attempt = status.reconnect_attempt,
                    "VR Overlay connection failed"
                );
                status.last_error_detail = Some(error);
                status.state = RuntimeState::Reconnecting;
                state.next_reconnect = Instant::now() + RECONNECT_INTERVAL;
                return;
            }
        }
    }

    let Some(mut backend) = state.backend.take() else {
        #[cfg(windows)]
        state.ocr.unavailable(ocr_enabled);
        return;
    };
    status.state = RuntimeState::Ready;
    status.last_error_detail = None;
    #[cfg(windows)]
    {
        if let Err(error) = state.ocr.prepare_input(&mut backend) {
            state.reset_ui();
            if state.ocr.status.last_error.as_ref() != Some(&error) {
                tracing::warn!(%error, "VR controller input unavailable");
            }
            state.ocr.status.last_error = Some(error);
        }
        let modal = state.player_audio.tick(app, &mut backend, &state.controls);
        let (events, focused) = state.ui.tick(
            app,
            &mut backend,
            &state.config,
            &state.controls,
            state.ocr.input_revision(),
            state.ocr.owns_input() || modal,
        );
        state.ocr.pointer_input(events, focused || modal);
        let player_progress = state
            .controls
            .lock()
            .ok()
            .and_then(|hub| hub.player_audio().progress.clone());
        state.ocr.tick(
            app,
            &mut backend,
            &state.ocr_config,
            &state.config.wrist,
            player_progress.as_ref(),
        );
    }
    update_headset(app, state, status, &mut backend);
    update_wrist(app, state, status, &mut backend);
    update_dashboard(app, state, status, &mut backend);
    let headset_failed =
        state.config.headset.enabled && status.headset.state == ResourceState::Error;
    let wrist_failed = state.config.wrist.enabled && status.wrist.state == ResourceState::Error;
    #[cfg(windows)]
    let (headset_failed, wrist_failed) = {
        let hub = app.state::<super::surface::SurfaceHub>();
        // Wrist captions render through Dialogue; the wrist menu is a separate surface.
        (
            headset_failed && !hub.recovery_failed(super::surface::SurfaceId::Headset),
            wrist_failed && !hub.recovery_failed(super::surface::SurfaceId::Dialogue),
        )
    };
    // A terminal Surface failure stays in Entry until business hide or explicit retry.
    if any_enabled
        && !ocr_enabled
        && !vision_enabled
        && (headset_failed || !state.config.headset.enabled)
        && (wrist_failed || !state.config.wrist.enabled)
    {
        let error = status
            .last_error_detail
            .clone()
            .unwrap_or_else(|| "all enabled overlay resources failed".into());
        tracing::warn!(
            error = %error,
            headset_failed,
            wrist_failed,
            "VR Overlay resources failed; reconnecting"
        );
        backend.hide_all();
        status.state = RuntimeState::Reconnecting;
        status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
        state.next_reconnect = Instant::now() + RECONNECT_INTERVAL;
    } else {
        state.backend = Some(backend);
    }
}

#[cfg(windows)]
fn update_dashboard(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut OpenVrBackend,
) {
    use super::dashboard::{DASHBOARD_HEIGHT, DASHBOARD_WIDTH};
    use super::surface::{SurfaceHub, SurfaceId, SurfaceView};
    let Some(view) = state.dashboard_view.as_mut() else {
        status.dashboard.state = ResourceState::Disabled;
        status.dashboard.visible = false;
        return;
    };
    let notice = state
        .controls
        .lock()
        .ok()
        .and_then(|hub| hub.player_audio().notice.clone());
    if view.player_audio_notice != notice {
        Arc::make_mut(view).player_audio_notice = notice;
    }
    let view = view.clone();
    // Error backoff is not a user hide and must not reset the Surface recovery budget.
    if backend.dashboard_visible() {
        let _ = state.dashboard_surface.request(app);
    }
    if state
        .dashboard_retry_at
        .is_some_and(|until| Instant::now() < until)
    {
        return;
    }
    state.dashboard_retry_at = None;
    let result = (|| -> Result<bool, String> {
        backend.ensure_dashboard()?;
        let hub = app.state::<SurfaceHub>();
        // Keep the tab registered, but never wake its WebView before it is opened.
        if !backend.dashboard_visible() {
            let _ = backend.poll_dashboard_events()?;
            state.dashboard.clear_pointer();
            state.dashboard_pressed_key = None;
            return Ok(false);
        }
        let dark = state
            .controls
            .lock()
            .map(|hub| hub.snapshot().dark)
            .unwrap_or(false);
        // Revoke stale input before polling; only the renderer owns recovery decisions.
        hub.publish(
            app,
            SurfaceId::Dashboard,
            SurfaceView::Dashboard {
                dark,
                dashboard: view.clone(),
                ui: state.dashboard.ui.clone(),
            },
            state.dashboard.hovered,
            state.dashboard.pressed,
        )?;
        let mut layout = hub.layout(SurfaceId::Dashboard);
        for event in backend.poll_dashboard_events()? {
            let (x, y) = match event {
                DashboardPointerEvent::Move { x, y }
                | DashboardPointerEvent::Down { x, y }
                | DashboardPointerEvent::Up { x, y } => (x, y),
                DashboardPointerEvent::Shown | DashboardPointerEvent::Hidden => {
                    state.dashboard.clear_pointer();
                    state.dashboard_pressed_key = None;
                    hub.point(app, SurfaceId::Dashboard, None)?;
                    continue;
                }
            };
            let point = [x / DASHBOARD_WIDTH as f32, y / DASHBOARD_HEIGHT as f32];
            hub.point(app, SurfaceId::Dashboard, Some(point))?;
            let hit = layout.as_ref().and_then(|layout| layout.hit(point));
            state.dashboard.hovered = hit;
            match event {
                DashboardPointerEvent::Down { .. } => {
                    state.dashboard.pressed = hit;
                    state.dashboard_pressed_key = layout.as_ref().map(|layout| layout.key);
                }
                DashboardPointerEvent::Up { .. } => {
                    let pressed = state.dashboard.pressed.take();
                    let key = state.dashboard_pressed_key.take();
                    let command = layout
                        .as_ref()
                        .filter(|layout| Some(layout.key) == key)
                        .filter(|_| hit.is_some() && hit == pressed)
                        .and_then(|layout| {
                            layout.regions.iter().find(|region| Some(region.id) == hit)
                        })
                        .and_then(|region| region.dashboard.clone());
                    if let Some(command) = command {
                        if let super::dashboard::DashboardCommand::PlayerAudio {
                            request_id,
                            action,
                        } = command
                        {
                            super::player_audio_ui::activate(
                                app,
                                &state.controls,
                                SurfaceId::Dashboard,
                                request_id,
                                action,
                            );
                        } else if view.player_audio_notice.is_none() {
                            if let Some(action) = state.dashboard.activate(command, &view) {
                                dispatch_dashboard_action(app, state, backend, action);
                            }
                        }
                        // Any subsequent input must wait for the newly rendered layout.
                        layout = None;
                    }
                }
                _ => {}
            }
        }
        let ready = state.dashboard_surface.present(
            app,
            backend,
            OverlayKind::Dashboard,
            SurfaceView::Dashboard {
                dark,
                dashboard: view,
                ui: state.dashboard.ui.clone(),
            },
            state.dashboard.hovered,
            state.dashboard.pressed,
        )?;
        if !ready {
            backend.show_dashboard_placeholder(None)?;
        }
        Ok(ready)
    })();
    match result {
        Ok(ready) => {
            status.dashboard.visible = ready && backend.dashboard_visible();
            status.dashboard.state = if status.dashboard.visible {
                ResourceState::Visible
            } else if ready || !backend.dashboard_visible() {
                ResourceState::ReadyHidden
            } else {
                ResourceState::Creating
            };
            status.dashboard.last_error_code = None;
        }
        Err(error) => {
            // Release the captured texture while keeping the registered dashboard tab.
            backend.reset(OverlayKind::Dashboard);
            state.reset_dashboard();
            if let Err(placeholder_error) = backend.show_dashboard_placeholder(Some(&error)) {
                tracing::warn!(%placeholder_error, "SteamVR dashboard error display failed");
            }
            state.dashboard_retry_at = Some(Instant::now() + Duration::from_secs(1));
            status.dashboard.visible = false;
            status.dashboard.state = ResourceState::Error;
            status.dashboard.last_error_code = Some("dashboard_surface".into());
            if status.last_error_detail.as_deref() != Some(&error) {
                tracing::warn!(%error, "SteamVR dashboard surface unavailable");
            }
            status.last_error_detail = Some(error);
        }
    }
}

#[cfg(windows)]
fn dispatch_dashboard_action(
    app: &AppHandle,
    state: &mut WorkerState,
    backend: &OpenVrBackend,
    action: super::dashboard::DashboardAction,
) {
    use super::dashboard::DashboardAction;
    if matches!(
        action,
        DashboardAction::RecenterDialogue | DashboardAction::AttachDialogue
    ) {
        let mode = if action == DashboardAction::RecenterDialogue {
            Mode::World
        } else {
            Mode::Wrist
        };
        state.ui.place(app, backend, &state.controls, mode);
        return;
    }
    let control = state.controls.lock().ok().and_then(|hub| {
        let view = hub.snapshot();
        match &action {
            DashboardAction::ToggleWrist => {
                Some(ControlAction::SetWristEnabled(!view.items[0].requested))
            }
            DashboardAction::ToggleHeadset => {
                Some(ControlAction::SetHeadsetEnabled(!view.items[1].requested))
            }
            DashboardAction::ToggleOsc => {
                Some(ControlAction::SetOscEnabled(!view.items[2].requested))
            }
            DashboardAction::ToggleCapture => {
                Some(ControlAction::SetCaptureRunning(!view.items[3].requested))
            }
            _ => None,
        }
    });
    if let Some(control) = control {
        dispatch_control(app, &state.controls, ControlSource::Dashboard, control);
    } else if let Err(error) = app.emit_to("main", "vr-dashboard-action", action) {
        tracing::warn!(%error, "SteamVR dashboard action emit failed");
    }
}

#[cfg(not(windows))]
fn update_dashboard(
    _app: &AppHandle,
    _state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    _backend: &mut OpenVrBackend,
) {
    status.dashboard.state = ResourceState::Disabled;
    status.dashboard.visible = false;
}

fn question_draft_frame(draft: &str) -> PresentationFrame {
    PresentationFrame::headset(
        if draft.trim().is_empty() {
            "Listening · release trigger to ask"
        } else {
            draft
        },
        1.0,
    )
}

fn update_headset(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut OpenVrBackend,
) {
    #[cfg(windows)]
    let question_draft = state.ocr.vision.draft();
    #[cfg(not(windows))]
    let question_draft: Option<String> = None;
    // A held visual question temporarily owns the subtitle surface, without
    // changing subtitle history or requiring the subtitle display to be enabled.
    if question_draft.is_none() && (!state.config.enabled || !state.config.headset.enabled) {
        backend.reset(OverlayKind::Headset);
        state.reset_headset();
        status.headset.state = ResourceState::Disabled;
        status.headset.last_error_code = None;
        return;
    }
    status.headset.state = ResourceState::Creating;
    if let Err(error) = backend.ensure_headset(&state.config.headset) {
        backend.reset(OverlayKind::Headset);
        state.reset_headset();
        resource_error(
            &mut status.headset,
            "headset_setup",
            error,
            &mut status.last_error_detail,
        );
        return;
    }

    let frame = if let Some(draft) = question_draft {
        Some(question_draft_frame(&draft))
    } else if state.headset_sample {
        Some(PresentationFrame::headset(
            "VRCS Headset Overlay Sample / 视野字幕预览",
            1.0,
        ))
    } else {
        state.headset.frame_with_translation_display(
            Instant::now(),
            &state.config.headset,
            &state.config.translation_display,
        )
    };
    let notice = state.caption_status.text(Instant::now());
    let frame = frame.or_else(|| {
        notice.as_ref().map(|_| PresentationFrame {
            content: PresentationContent::Headset(Vec::new()),
            opacity: 1.0,
        })
    });
    let Some(frame) = frame else {
        backend.hide(OverlayKind::Headset);
        status.headset.state = ResourceState::ReadyHidden;
        status.headset.last_error_code = None;
        return;
    };

    #[cfg(windows)]
    let presented = (|| {
        use super::surface::SurfaceView;
        let PresentationContent::Headset(messages) = frame.content else {
            return Err("Invalid headset content".into());
        };
        let dark = state
            .controls
            .lock()
            .map(|hub| hub.snapshot().dark)
            .unwrap_or(false);
        let visible = state.headset_surface.present(
            app,
            backend,
            OverlayKind::Headset,
            SurfaceView::Headset {
                visibility: state
                    .dashboard_view
                    .as_ref()
                    .map(|view| view.caption_visibility.clone())
                    .unwrap_or_default(),
                max_visible_players: state
                    .dashboard_view
                    .as_ref()
                    .and_then(|view| view.max_visible_players),
                dark,
                messages,
                notice: notice.clone(),
                labels: state
                    .dashboard_view
                    .as_ref()
                    .map(|view| view.labels.captions.clone())
                    .unwrap_or_default(),
                lines_per_language: state.config.headset.lines_per_language,
                font_size: state.config.headset.font_size_px,
                opacity: state.config.headset.background_opacity,
            },
            None,
            None,
        )?;
        if visible {
            let shown = backend
                .set_opacity(
                    OverlayKind::Headset,
                    state.config.headset.opacity
                        * if notice.is_some() { 1.0 } else { frame.opacity },
                )
                .and_then(|()| backend.show(OverlayKind::Headset));
            if shown.is_err() {
                backend.reset(OverlayKind::Headset);
                state.reset_headset();
            }
            shown?;
        } else {
            backend.hide(OverlayKind::Headset);
        }
        Ok(visible)
    })();
    #[cfg(not(windows))]
    let presented: Result<bool, String> = Err("VR Overlay is only supported on Windows".into());
    match presented {
        Ok(visible) => {
            status.headset.state = if !visible {
                ResourceState::Creating
            } else if frame.opacity < 1.0 {
                ResourceState::Fading
            } else {
                ResourceState::Visible
            };
            status.headset.last_error_code = None;
        }
        Err(error) => {
            backend.reset(OverlayKind::Headset);
            resource_error(
                &mut status.headset,
                "headset_render",
                error,
                &mut status.last_error_detail,
            );
        }
    }
}

#[cfg(windows)]
fn update_wrist(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut OpenVrBackend,
) {
    use super::surface::SurfaceView;
    if !state.config.enabled || !state.config.wrist.enabled {
        backend.reset(OverlayKind::Wrist);
        state.ui.dialogue_revision = None;
        status.wrist.state = ResourceState::Disabled;
        status.wrist.bound_role = None;
        status.wrist.tracked_device_available = false;
        status.wrist.last_error_code = None;
        return;
    }
    let (mode, pose, relative, follow_view, busy, error, revision) = {
        let placement = state.ui.placement.lock().unwrap_or_else(|e| e.into_inner());
        (
            placement.display_mode(),
            placement.absolute_pose(),
            placement.head_relative_pose(),
            placement.follow_view(),
            placement.busy(),
            placement.error.clone(),
            placement.revision,
        )
    };
    let setup = if mode == Mode::World {
        status.wrist.bound_role = None;
        match (pose, backend.ocr_tracking()) {
            (Some(pose), Ok((_, _, origin))) => {
                backend.ensure_world_wrist(&state.config.wrist, pose, origin, relative)
            }
            _ => {
                backend.hide(OverlayKind::Wrist);
                status.wrist.state = ResourceState::DeviceUnavailable;
                status.wrist.tracked_device_available = false;
                return;
            }
        }
    } else {
        backend
            .ensure_wrist(&state.config.wrist)
            .and_then(|binding| {
                status.wrist.bound_role = binding.role;
                if binding.available {
                    Ok(())
                } else {
                    Err("Controller tracking unavailable".into())
                }
            })
    };
    if let Err(error) = setup {
        backend.hide(OverlayKind::Wrist);
        status.wrist.tracked_device_available = false;
        if error == "Controller tracking unavailable" {
            status.wrist.state = ResourceState::DeviceUnavailable;
        } else {
            backend.reset(OverlayKind::Wrist);
            wrist_error(status, "wrist_setup", error);
        }
        return;
    }
    status.wrist.tracked_device_available = true;
    if !state.ui.dialogue_visible && !state.wrist_sample {
        backend.hide(OverlayKind::Wrist);
        status.wrist.state = ResourceState::ReadyHidden;
        return;
    }
    let frame = if state.wrist_sample {
        Some(PresentationFrame::wrist(vec![
            WristMessage {
                original: None,
                speaker: None,
                text: "你好，欢迎使用 VRCS。".into(),
                side: MessageSide::Left,
            },
            WristMessage {
                original: None,
                speaker: None,
                text: "Conversation stays on your wrist or in front of you.".into(),
                side: MessageSide::Right,
            },
        ]))
    } else {
        state.wrist.frame_with_translation_display(
            Instant::now(),
            &state.config.wrist,
            &state.config.translation_display,
        )
    };
    let notice = state.caption_status.text(Instant::now());
    let frame = frame.or_else(|| {
        notice
            .as_ref()
            .map(|_| PresentationFrame::wrist(Vec::new()))
    });
    let Some(frame) = frame else {
        backend.hide(OverlayKind::Wrist);
        state.ui.dialogue_revision = None;
        status.wrist.state = ResourceState::ReadyHidden;
        status.wrist.last_error_code = None;
        return;
    };
    let controls = state
        .controls
        .lock()
        .map(|hub| hub.snapshot())
        .unwrap_or_default();
    let messages = match frame.content {
        PresentationContent::Wrist(messages) => messages,
        _ => Vec::new(),
    };
    let result = (|| {
        let visible = state.ui.dialogue_surface.present(
            app,
            backend,
            OverlayKind::Wrist,
            SurfaceView::Dialogue {
                visibility: state
                    .dashboard_view
                    .as_ref()
                    .map(|view| view.caption_visibility.clone())
                    .unwrap_or_default(),
                max_visible_players: state
                    .dashboard_view
                    .as_ref()
                    .and_then(|view| view.max_visible_players),
                max_entries: state.config.wrist.max_entries.clamp(3, 10) as usize,
                controls,
                messages,
                notice,
                labels: state
                    .dashboard_view
                    .as_ref()
                    .map(|view| view.labels.captions.clone())
                    .unwrap_or_default(),
                mode,
                follow_view,
                busy,
                error,
                font_size: state.config.wrist.font_size_px,
                opacity: state.config.wrist.background_opacity,
            },
            state.ui.dialogue_hover,
            None,
        )?;
        backend.set_opacity(OverlayKind::Wrist, state.config.wrist.opacity)?;
        if visible {
            backend.show(OverlayKind::Wrist)?;
        } else {
            backend.hide(OverlayKind::Wrist);
        }
        Ok::<_, String>(visible)
    })();
    match result {
        Ok(visible) => {
            state.ui.dialogue_revision = visible.then_some(revision);
            status.wrist.state = if visible {
                ResourceState::Visible
            } else {
                ResourceState::Creating
            };
            status.wrist.last_error_code = None;
        }
        Err(error) => {
            state.ui.dialogue_revision = None;
            backend.reset(OverlayKind::Wrist);
            wrist_error(status, "wrist_render", error);
        }
    }
}

#[cfg(not(windows))]
fn update_wrist(
    _app: &AppHandle,
    _state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    _backend: &mut OpenVrBackend,
) {
    status.wrist.state = ResourceState::Disabled;
}

fn resource_error(
    resource: &mut ResourceStatus,
    code: &str,
    detail: String,
    global_detail: &mut Option<String>,
) {
    resource.state = ResourceState::Error;
    resource.last_error_code = Some(code.into());
    *global_detail = Some(detail);
}

fn wrist_error(status: &mut VrOverlayStatus, code: &str, detail: String) {
    status.wrist.state = ResourceState::Error;
    status.wrist.last_error_code = Some(code.into());
    status.last_error_detail = Some(detail);
}

fn update_status(app: &AppHandle, shared: &Arc<Mutex<VrOverlayStatus>>, next: &VrOverlayStatus) {
    let changed = match shared.lock() {
        Ok(mut current) if *current != *next => {
            *current = next.clone();
            true
        }
        Ok(_) => false,
        Err(error) => {
            tracing::warn!(%error, "VR Overlay status lock poisoned");
            false
        }
    };
    if changed {
        let _ = app.emit(STATUS_EVENT, next);
    }
}

fn unix_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn question_draft_uses_the_headset_subtitle_frame() {
        assert_eq!(
            question_draft_frame("updated question"),
            PresentationFrame::headset("updated question", 1.0)
        );
        assert_eq!(
            question_draft_frame("  "),
            PresentationFrame::headset("Listening · release trigger to ask", 1.0)
        );
    }

    #[test]
    fn sample_kind_rejects_unknown_values() {
        assert_eq!(SampleKind::parse("headset").unwrap(), SampleKind::Headset);
        assert!(SampleKind::parse("dashboard").is_err());
    }

    #[test]
    fn initial_status_has_stable_disabled_resources() {
        let status = VrOverlayStatus::initial();
        assert!(!status.runtime_installed);
        assert_eq!(status.headset.state, ResourceState::Disabled);
        assert!(!status.wrist.sample_visible);
    }
}
