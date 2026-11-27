use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::ocr_status::OcrStatus;
use serde::Serialize;
#[cfg(any(windows, linux_vr))]
use tauri::Manager as _;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, watch};
use vrcs_core::{PresentationEvent, VrOcrConfig, VrOverlayConfig};

use super::backend::{Backend, BackendKind, OpenXrProbe, OverlayKind};
use super::control_protocol::ControlHub;
#[cfg(any(windows, linux_vr))]
use super::control_protocol::{ControlAction, ControlSource};
#[cfg(any(windows, linux_vr))]
use super::dashboard::DashboardPointerEvent;
use super::dashboard::{DashboardState, DashboardViewModel};
use super::placement::{Mode, Placement};
#[cfg(any(windows, linux_vr, test))]
use super::presentation::PresentationFrame;
use super::presentation::{HeadsetPresentation, WristPresentation};
#[cfg(any(windows, linux_vr))]
use super::presentation::{MessageSide, PresentationContent, WristMessage};
use super::process::{OpenVrSession, openvr_server_pid};

pub const STATUS_EVENT: &str = "vr-overlay-status-changed";
const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
const IDLE_WAITING_INTERVAL: Duration = Duration::from_millis(250);
const IDLE_DISABLED_INTERVAL: Duration = Duration::from_millis(500);
pub(super) const SURFACE_FRAME_INTERVAL: Duration = Duration::from_nanos(16_666_700);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const MAX_RECONNECT_INTERVAL: Duration = Duration::from_secs(30);
const EVENT_QUEUE_CAPACITY: usize = 256;
const DROP_WARNING_INTERVAL: Duration = Duration::from_secs(5);

struct BusinessClock(Instant);

impl BusinessClock {
    fn due(&mut self, now: Instant) -> bool {
        if now < self.0 {
            return false;
        }
        self.0 = now + UPDATE_INTERVAL;
        true
    }
}

pub(super) fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(any(windows, linux_vr))]
pub(super) fn dispatch_control(
    app: &AppHandle,
    controls: &Mutex<ControlHub>,
    source: ControlSource,
    action: ControlAction,
) {
    let request = lock_unpoisoned(controls).begin(source, action);
    if let Some(request) = request
        && let Err(error) = app.emit_to("main", super::control_protocol::REQUEST_EVENT, &request)
    {
        lock_unpoisoned(controls).finish(&request, Some(error.to_string()));
    }
}

#[cfg(any(windows, linux_vr))]
pub(super) fn dispatch_player_display(
    app: &AppHandle,
    controls: &Mutex<ControlHub>,
    source: ControlSource,
    expected: (u32, u32),
    action: super::player_display::Action,
) {
    let request = lock_unpoisoned(controls).begin_player_display(source, expected, action);
    if let Some(request) = request
        && let Err(error) = app.emit_to("main", super::player_display::REQUEST_EVENT, &request)
    {
        lock_unpoisoned(controls).finish_player_display(&request, Some(error.to_string()));
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
    Disabled,
    WaitingRuntime,
    Initializing,
    Ready,
    Reconnecting,
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
    pub requested_backend: String,
    pub backend: Option<BackendKind>,
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
        let supported = cfg!(any(windows, linux_vr));
        Self {
            state: if supported {
                RuntimeState::Initializing
            } else {
                RuntimeState::Unsupported
            },
            requested_backend: "auto".into(),
            backend: None,
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
    stopping: Mutex<Option<Arc<AtomicBool>>>,
    wakeup: Arc<AtomicBool>,
    worker_thread: Arc<Mutex<Option<std::thread::Thread>>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    bridges: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
}

impl Manager {
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    pub(super) fn preview_view(&self) -> Option<DashboardViewModel> {
        lock_unpoisoned(&self.latest_dashboard).as_deref().cloned()
    }
    pub fn revoke_controls(&self) {
        lock_unpoisoned(&self.controls).revoke();
        lock_unpoisoned(&self.placement).cancel();
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
            stopping: Mutex::new(None),
            wakeup: Arc::new(AtomicBool::new(false)),
            worker_thread: Arc::new(Mutex::new(None)),
            worker: Mutex::new(None),
            bridges: Mutex::new(Vec::new()),
        }
    }

    fn wake_worker(&self) {
        wake_worker_thread(&self.wakeup, &self.worker_thread);
    }

    pub fn status(&self) -> Result<VrOverlayStatus, String> {
        Ok(lock_unpoisoned(&self.status).clone())
    }

    pub fn open_ocr_bindings(&self) -> Result<(), String> {
        let status = self.status()?;
        if !cfg!(windows) || !status.hmd_present || !status.runtime_installed {
            return Err("SteamVR headset is unavailable".into());
        }
        self.update_control(|control| control.ocr_bindings = true)
    }

    pub fn update_dashboard(&self, view: DashboardViewModel) -> Result<(), String> {
        *lock_unpoisoned(&self.latest_dashboard) = Some(Arc::new(view));
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
        let stopping = Arc::new(AtomicBool::new(false));
        *lock_unpoisoned(&self.stopping) = Some(stopping.clone());

        let app = self.app.clone();
        let status = self.status.clone();
        let pending_control = self.pending_control.clone();
        let latest_config = self.latest_config.clone();
        let latest_dashboard = self.latest_dashboard.clone();
        let controls = self.controls.clone();
        let placement = self.placement.clone();
        let wakeup = self.wakeup.clone();
        self.wakeup.store(false, Ordering::Release);
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
                    wakeup,
                    initial_config,
                    features,
                    ocr_service,
                    vision_service,
                )
            })
            .map_err(|error| format!("Failed to start VR Overlay thread: {error}"))?;
        *lock_unpoisoned(&self.worker_thread) = Some(worker.thread().clone());
        *lock_unpoisoned(&self.event_sender) = Some(event_sender.clone());
        *lock_unpoisoned(&self.worker) = Some(worker);

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
        let wakeup = self.wakeup.clone();
        let worker_thread = self.worker_thread.clone();
        let config_bridge = tauri::async_runtime::spawn(async move {
            while config.changed().await.is_ok() {
                let next = config.borrow().clone();
                *lock_unpoisoned(&latest_config) = Some(next);
                wake_worker_thread(&wakeup, &worker_thread);
            }
        });
        *lock_unpoisoned(&self.bridges) = vec![event_bridge, config_bridge];
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

    pub fn stop(&self) -> bool {
        for task in lock_unpoisoned(&self.bridges).drain(..) {
            task.abort();
        }
        if let Some(stopping) = lock_unpoisoned(&self.stopping).take() {
            stopping.store(true, Ordering::Release);
        }
        self.wake_worker();
        lock_unpoisoned(&self.event_sender).take();
        *lock_unpoisoned(&self.pending_control) = PendingControl::default();
        lock_unpoisoned(&self.latest_config).take();
        let worker = lock_unpoisoned(&self.worker).take();
        let mut stopped_cleanly = true;
        if let Some(worker) = worker {
            // A running OpenXR session can block the worker in `xrWaitFrame`
            // when the headset stops delivering frames. Give shutdown a brief
            // grace window so native host teardown runs after the worker exits,
            // without holding the caller hostage if the runtime stays hung.
            let timeout = Duration::from_secs(if cfg!(linux_vr) { 2 } else { 5 });
            let (done, wait) = mpsc::channel();
            let reaper = std::thread::Builder::new()
                .name("vrcs-vr-overlay-join".into())
                .spawn(move || {
                    if worker.join().is_err() {
                        tracing::warn!("VR Overlay thread panicked during shutdown");
                    }
                    let _ = done.send(());
                });
            match reaper {
                Ok(_) if wait.recv_timeout(timeout).is_err() => {
                    tracing::warn!("VR Overlay worker is blocked; detaching it");
                    stopped_cleanly = false;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(%error, "VR Overlay join thread failed");
                    stopped_cleanly = false;
                }
            }
        }
        lock_unpoisoned(&self.worker_thread).take();
        stopped_cleanly
    }

    fn update_control(&self, update: impl FnOnce(&mut PendingControl)) -> Result<(), String> {
        let sender = lock_unpoisoned(&self.event_sender);
        if sender.is_none() {
            return Err("VR Overlay is waiting for Core startup".into());
        }
        let mut control = lock_unpoisoned(&self.pending_control);
        update(&mut control);
        drop(control);
        drop(sender);
        self.wake_worker();
        Ok(())
    }
}

fn wake_worker_thread(wakeup: &AtomicBool, worker_thread: &Mutex<Option<std::thread::Thread>>) {
    wakeup.store(true, Ordering::Release);
    if let Some(thread) = lock_unpoisoned(worker_thread).as_ref() {
        thread.unpark();
    }
}

#[cfg(any(windows, linux_vr))]
fn active_caption_notice(state: &WorkerState, now: Instant) -> (Option<String>, bool) {
    #[cfg(windows)]
    if state.player_progress.is_some() {
        return (None, false);
    }
    (
        state.caption_status.text(now),
        state.caption_status.is_error(now),
    )
}

#[cfg(any(windows, linux_vr))]
struct HeadsetPublishCache {
    headset_revision: u64,
    config_revision: u64,
    dashboard_revision: u64,
    sample: bool,
    dark: bool,
    draft: Option<String>,
    notice: Option<String>,
    notice_error: bool,
    frame_opacity: Option<f32>,
    next_change: Option<Instant>,
}

#[cfg(any(windows, linux_vr))]
struct DialoguePublishCache {
    wrist_revision: u64,
    config_revision: u64,
    dashboard_revision: u64,
    controls_version: u64,
    sample: bool,
    notice: Option<String>,
    notice_error: bool,
    mode: Mode,
    follow_view: bool,
    busy: bool,
    error: Option<String>,
    idle_expires_at: Option<Instant>,
    has_frame: bool,
}

#[cfg(any(windows, linux_vr))]
struct DashboardRepeat {
    region: u8,
    command: super::dashboard::DashboardCommand,
    next_at: Instant,
    repeated: bool,
}

struct WorkerState {
    caption_status: super::caption_status::CaptionStatus,
    controls: Arc<Mutex<ControlHub>>,
    #[cfg(any(windows, linux_vr))]
    ui: super::ui_runtime::UiRuntime,
    #[cfg(any(windows, linux_vr))]
    dialogue_policy: u64,
    #[cfg(any(windows, linux_vr))]
    player_audio: super::player_audio_ui::Runtime,
    #[cfg(windows)]
    player_progress: Option<super::ocr_progress::ProgressView>,
    config: VrOverlayConfig,
    ocr_config: VrOcrConfig,
    selected_backend: Option<BackendKind>,
    headset: HeadsetPresentation,
    wrist: WristPresentation,
    headset_sample: bool,
    wrist_sample: bool,
    #[cfg(any(windows, linux_vr))]
    headset_surface: super::surface::SurfaceRenderer,
    #[cfg(any(windows, linux_vr))]
    config_revision: u64,
    #[cfg(any(windows, linux_vr))]
    dashboard_caption_revision: u64,
    #[cfg(any(windows, linux_vr))]
    headset_cache: Option<HeadsetPublishCache>,
    #[cfg(any(windows, linux_vr))]
    dialogue_cache: Option<DialoguePublishCache>,
    backend: Option<Backend>,
    session: OpenVrSession,
    dashboard_view: Option<Arc<DashboardViewModel>>,
    dashboard: DashboardState,
    #[cfg(any(windows, linux_vr))]
    dashboard_surface: super::surface::SurfaceRenderer,
    #[cfg(any(windows, linux_vr))]
    dashboard_pressed_key: Option<super::surface_protocol::SurfaceKey>,
    #[cfg(any(windows, linux_vr))]
    dashboard_repeat: Option<DashboardRepeat>,
    dashboard_retry_at: Option<Instant>,
    last_dashboard_error: Option<String>,
    #[cfg(linux_vr)]
    ocr_manifest: Option<std::path::PathBuf>,
    #[cfg(linux_vr)]
    last_input_error: Option<String>,
    next_reconnect: Instant,
    next_probe: Instant,
    probe_ready: bool,
    probe_incompatibility: Option<String>,
    incompatible_openvr: Option<(u32, String)>,
    xr_session_ready: bool,
    xr_stall_count: u32,
    last_connect_error: Option<String>,
    #[cfg(any(windows, linux_vr))]
    surfaces_dormant: bool,
    #[cfg(windows)]
    ocr: super::ocr_runtime::OcrRuntime,
}

impl WorkerState {
    #[cfg(any(windows, linux_vr))]
    fn reset_ui(&mut self) {
        self.ui.reset();
        #[cfg(any(windows, linux_vr))]
        self.player_audio.reset();
        self.dialogue_cache = None;
    }

    fn reset_dashboard(&mut self) {
        #[cfg(any(windows, linux_vr))]
        {
            self.dashboard_surface.reset();
            self.dashboard_pressed_key = None;
            self.dashboard_repeat = None;
        }
        self.dashboard.clear_pointer();
        self.dashboard_retry_at = None;
        self.last_dashboard_error = None;
    }

    fn reset_headset(&mut self) {
        #[cfg(any(windows, linux_vr))]
        {
            self.headset_surface.reset();
            self.headset_cache = None;
        }
    }
}

fn reconnect_backoff(attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(4);
    let secs = RECONNECT_INTERVAL.as_secs().saturating_mul(1_u64 << shift);
    Duration::from_secs(secs).min(MAX_RECONNECT_INTERVAL)
}

fn is_openxr_incompatibility(detail: &str) -> bool {
    matches!(
        detail,
        "vr.openxr_backend_unavailable"
            | "vr.openxr_overlay_unsupported"
            | "vr.openxr_vulkan_unavailable"
            | "vr.runtime_arch_unverified"
    )
}

fn is_openxr_session_end(error: &str) -> bool {
    matches!(error, "vr.openxr_session_ended" | "vr.openxr_instance_lost")
        || error.contains("ERROR_SESSION_LOST")
        || error.contains("ERROR_INSTANCE_LOST")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectFailureKind {
    MissingRuntime,
    Incompatible,
    Transient,
}

fn classify_connect_error(error: &str) -> ConnectFailureKind {
    if is_openxr_incompatibility(error)
        || error.contains("Init_InvalidApplicationType")
        || error.starts_with("Unsupported VR Overlay backend:")
    {
        ConnectFailureKind::Incompatible
    } else if matches!(
        error,
        "vr.openxr_headset_unavailable" | "vr.openxr_loader_unavailable"
    ) || error.starts_with("OpenXR runtime unavailable")
        || error.contains("Init_NoServerForBackgroundApp")
        || error.contains("Init_HmdNotFound")
        || error.contains("Init_PathRegistryNotFound")
        || error.contains("Init_InstallationNotFound")
    {
        ConnectFailureKind::MissingRuntime
    } else {
        ConnectFailureKind::Transient
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProbeSnapshot {
    selected_backend: Option<BackendKind>,
    runtime_installed: bool,
    hmd_present: bool,
    openvr_pid: Option<u32>,
    ready_to_connect: bool,
    incompatibility_detail: Option<String>,
    clear_incompatible_openvr: bool,
}

fn evaluate_probe(
    requested_backend: &str,
    incompatible_openvr: Option<(u32, &str)>,
    openxr: impl FnOnce() -> OpenXrProbe,
    openvr_installed: impl FnOnce() -> bool,
    openvr_server_pid: impl FnOnce(bool) -> Option<u32>,
    openvr_can_connect: impl FnOnce(Option<u32>) -> bool,
    openvr_hmd_present: impl FnOnce() -> bool,
) -> ProbeSnapshot {
    match requested_backend {
        "openxr" => {
            let xr = openxr();
            let ready = xr.runtime_installed && xr.hmd_present;
            let incompatibility_detail = (!ready)
                .then(|| xr.detail.filter(|detail| is_openxr_incompatibility(detail)))
                .flatten();
            ProbeSnapshot {
                selected_backend: Some(BackendKind::OpenXr),
                runtime_installed: xr.runtime_installed,
                hmd_present: xr.hmd_present,
                openvr_pid: None,
                ready_to_connect: ready,
                incompatibility_detail,
                clear_incompatible_openvr: true,
            }
        }
        "openvr" => {
            let installed = openvr_installed();
            if !installed {
                return ProbeSnapshot {
                    selected_backend: Some(BackendKind::OpenVr),
                    runtime_installed: false,
                    hmd_present: false,
                    openvr_pid: None,
                    ready_to_connect: false,
                    incompatibility_detail: None,
                    clear_incompatible_openvr: true,
                };
            }
            let pid = openvr_server_pid(true);
            if let Some((bad_pid, reason)) = incompatible_openvr
                && pid == Some(bad_pid)
            {
                return ProbeSnapshot {
                    selected_backend: Some(BackendKind::OpenVr),
                    runtime_installed: true,
                    hmd_present: false,
                    openvr_pid: pid,
                    ready_to_connect: false,
                    incompatibility_detail: Some(reason.to_owned()),
                    clear_incompatible_openvr: false,
                };
            }
            let server_ready = openvr_can_connect(pid);
            let hmd = server_ready && openvr_hmd_present();
            ProbeSnapshot {
                selected_backend: Some(BackendKind::OpenVr),
                runtime_installed: true,
                hmd_present: hmd,
                openvr_pid: pid,
                ready_to_connect: server_ready && hmd,
                incompatibility_detail: None,
                clear_incompatible_openvr: true,
            }
        }
        "auto" => {
            let xr = openxr();
            if xr.runtime_installed && xr.hmd_present {
                return ProbeSnapshot {
                    selected_backend: Some(BackendKind::OpenXr),
                    runtime_installed: true,
                    hmd_present: true,
                    openvr_pid: None,
                    ready_to_connect: true,
                    incompatibility_detail: None,
                    clear_incompatible_openvr: true,
                };
            }
            let ovr_installed = openvr_installed();
            if ovr_installed {
                let pid = openvr_server_pid(false);
                if let Some((bad_pid, reason)) = incompatible_openvr
                    && pid == Some(bad_pid)
                {
                    return ProbeSnapshot {
                        selected_backend: Some(BackendKind::OpenVr),
                        runtime_installed: true,
                        hmd_present: false,
                        openvr_pid: pid,
                        ready_to_connect: false,
                        incompatibility_detail: Some(reason.to_owned()),
                        clear_incompatible_openvr: false,
                    };
                }
                let server_ready = openvr_can_connect(pid);
                let ovr_hmd = server_ready && openvr_hmd_present();
                if server_ready && ovr_hmd {
                    return ProbeSnapshot {
                        selected_backend: Some(BackendKind::OpenVr),
                        runtime_installed: true,
                        hmd_present: true,
                        openvr_pid: pid,
                        ready_to_connect: true,
                        incompatibility_detail: None,
                        clear_incompatible_openvr: true,
                    };
                }
            }
            ProbeSnapshot {
                selected_backend: Some(if xr.runtime_installed {
                    BackendKind::OpenXr
                } else {
                    BackendKind::OpenVr
                }),
                runtime_installed: xr.runtime_installed || ovr_installed,
                hmd_present: false,
                openvr_pid: None,
                ready_to_connect: false,
                incompatibility_detail: None,
                clear_incompatible_openvr: true,
            }
        }
        other => ProbeSnapshot {
            selected_backend: None,
            runtime_installed: false,
            hmd_present: false,
            openvr_pid: None,
            ready_to_connect: false,
            incompatibility_detail: Some(format!("Unsupported VR Overlay backend: {other}")),
            clear_incompatible_openvr: true,
        },
    }
}

fn reset_detached_state(state: &mut WorkerState) {
    if let Some(mut backend) = state.backend.take() {
        backend.hide_all();
        if state.config.backend == "auto" {
            state.selected_backend = None;
        }
    }
    #[cfg(windows)]
    {
        state.ocr.unavailable(state.ocr_config.enabled);
        state.reset_ui();
    }
    #[cfg(linux_vr)]
    {
        state.reset_ui();
        state.last_input_error = None;
    }
    state.reset_headset();
    state.reset_dashboard();
    state.probe_ready = false;
    state.xr_session_ready = false;
}

fn apply_waiting_runtime(
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    detail: Option<String>,
) {
    status.state = RuntimeState::WaitingRuntime;
    status.backend = None;
    status.reconnect_attempt = 0;
    status.last_error_detail = detail;
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
}

fn detach_to_waiting_runtime(
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    detail: Option<String>,
    retry_delay: Duration,
) {
    reset_detached_state(state);
    status.hmd_present = false;
    apply_waiting_runtime(state, status, detail);
    let now = Instant::now();
    state.next_probe = now + RECONNECT_INTERVAL;
    state.next_reconnect = now + retry_delay;
}

fn detach_for_reconnect(
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    error: String,
    immediate_probe: bool,
) {
    reset_detached_state(state);
    status.state = RuntimeState::Reconnecting;
    status.backend = None;
    status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
    status.last_error_detail = Some(error);
    let now = Instant::now();
    state.next_reconnect = now + reconnect_backoff(status.reconnect_attempt);
    state.next_probe = if immediate_probe {
        now
    } else {
        now + RECONNECT_INTERVAL
    };
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
    wakeup: Arc<AtomicBool>,
    config: (VrOverlayConfig, VrOcrConfig),
    features: watch::Receiver<vrcs_core::FeatureConfig>,
    ocr_service: Option<vrcs_core::ocr::VrOcrService>,
    vision_service: vrcs_core::vision::VisionService,
) {
    #[cfg(all(not(windows), not(linux_vr)))]
    let _ = (placement, ocr_service, vision_service);
    #[cfg(linux_vr)]
    let _ = (ocr_service, vision_service);
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
    let (mut headset, mut wrist) = (HeadsetPresentation::default(), WristPresentation::default());
    // Send uncapped candidates: the WebViews share the desktop visibility
    // policy and apply player/history limits after filtering original text.
    headset.set_player_limit(Some(0));
    wrist.set_player_limit(Some(0));
    let mut state = WorkerState {
        caption_status: Default::default(),
        controls,
        #[cfg(any(windows, linux_vr))]
        ui: super::ui_runtime::UiRuntime::new(placement),
        #[cfg(any(windows, linux_vr))]
        dialogue_policy: 1,
        #[cfg(any(windows, linux_vr))]
        player_audio: Default::default(),
        #[cfg(windows)]
        player_progress: None,
        config,
        ocr_config,
        selected_backend: None,
        headset,
        wrist,
        headset_sample: false,
        wrist_sample: false,
        #[cfg(any(windows, linux_vr))]
        headset_surface: super::surface::SurfaceRenderer::new(
            super::surface_protocol::SurfaceId::Headset,
        ),
        #[cfg(any(windows, linux_vr))]
        config_revision: 0,
        #[cfg(any(windows, linux_vr))]
        dashboard_caption_revision: 0,
        #[cfg(any(windows, linux_vr))]
        headset_cache: None,
        #[cfg(any(windows, linux_vr))]
        dialogue_cache: None,
        backend: None,
        session: OpenVrSession::default(),
        dashboard_view: None,
        dashboard: DashboardState::default(),
        #[cfg(any(windows, linux_vr))]
        dashboard_surface: super::surface::SurfaceRenderer::new(
            super::surface_protocol::SurfaceId::Dashboard,
        ),
        #[cfg(any(windows, linux_vr))]
        dashboard_pressed_key: None,
        #[cfg(any(windows, linux_vr))]
        dashboard_repeat: None,
        dashboard_retry_at: None,
        last_dashboard_error: None,
        #[cfg(linux_vr)]
        ocr_manifest: None,
        #[cfg(linux_vr)]
        last_input_error: None,
        next_reconnect: Instant::now(),
        next_probe: Instant::now(),
        probe_ready: false,
        probe_incompatibility: None,
        incompatible_openvr: None,
        xr_session_ready: false,
        xr_stall_count: 0,
        last_connect_error: None,
        #[cfg(any(windows, linux_vr))]
        surfaces_dormant: false,
        #[cfg(windows)]
        ocr,
    };
    let mut status = VrOverlayStatus::initial();
    status.requested_backend = state.config.backend.clone();
    let mut next_tick = Instant::now();
    let mut business_clock = BusinessClock(next_tick);
    #[cfg(any(windows, linux_vr))]
    let mut next_pointer = Instant::now();
    let mut xr_not_ready: Option<Instant> = None;

    loop {
        let started = Instant::now();
        let woken = wakeup.swap(false, Ordering::Acquire);
        if stopping.load(Ordering::Acquire) {
            status.state = RuntimeState::ShuttingDown;
            update_status(&app, &shared_status, &status);
            #[cfg(any(windows, linux_vr))]
            state
                .headset_surface
                .stop(&app, state.backend.as_mut(), OverlayKind::Headset);
            #[cfg(any(windows, linux_vr))]
            state
                .dashboard_surface
                .stop(&app, state.backend.as_mut(), OverlayKind::Dashboard);
            #[cfg(any(windows, linux_vr))]
            state.ui.stop_surfaces(&app, state.backend.as_mut());
            #[cfg(any(windows, linux_vr))]
            state
                .player_audio
                .stop_surfaces(&app, state.backend.as_mut());
            #[cfg(windows)]
            {
                state.ocr.stop_surfaces(&app, state.backend.as_mut());
            }
            if let Some(mut backend) = state.backend.take() {
                backend.hide_all();
            }
            break;
        }

        // Drive frames from the backend that is actually connected. A cached
        // preference can differ after a probe/connect fallback; using it here
        // would either leave the XR session unpumped or busy-spin OpenVR.
        let xr_driving = state
            .backend
            .as_ref()
            .is_some_and(|backend| backend.kind() == BackendKind::OpenXr);
        if !xr_driving {
            xr_not_ready = None;
        }
        if xr_driving {
            match state
                .backend
                .as_mut()
                .expect("XR backend checked")
                .begin_frame()
            {
                Ok(true) => {
                    xr_not_ready = None;
                    if !state.xr_session_ready {
                        state.xr_session_ready = true;
                        state.xr_stall_count = 0;
                        state.last_connect_error = None;
                        status.state = RuntimeState::Ready;
                        status.last_connected_at = Some(unix_timestamp());
                        status.reconnect_attempt = 0;
                        status.last_error_detail = None;
                    }
                }
                Ok(false) => {
                    // `pump` can stay false while the runtime is STOPPING and
                    // never deliver another frame. Detach instead of sleeping forever.
                    let since = xr_not_ready.get_or_insert_with(Instant::now);
                    if since.elapsed() >= Duration::from_secs(2) {
                        xr_not_ready = None;
                        if state.xr_session_ready {
                            tracing::warn!("OpenXR session did not become ready; reconnecting");
                            detach_for_reconnect(
                                &mut state,
                                &mut status,
                                "vr.openxr_session_stalled".into(),
                                true,
                            );
                        } else {
                            state.xr_stall_count = state.xr_stall_count.saturating_add(1);
                            if state.xr_stall_count == 1 {
                                tracing::info!(
                                    "OpenXR session stayed idle before becoming ready; waiting for VR runtime"
                                );
                            }
                            let retry_delay = reconnect_backoff(state.xr_stall_count);
                            detach_to_waiting_runtime(&mut state, &mut status, None, retry_delay);
                        }
                        update_status(&app, &shared_status, &status);
                    } else {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                    continue;
                }
                Err(error) => {
                    xr_not_ready = None;
                    if is_openxr_session_end(&error) {
                        tracing::info!(%error, "OpenXR session ended; waiting for VR runtime");
                        detach_to_waiting_runtime(
                            &mut state,
                            &mut status,
                            None,
                            RECONNECT_INTERVAL,
                        );
                    } else {
                        tracing::warn!(%error, "OpenXR frame wait failed");
                        detach_for_reconnect(&mut state, &mut status, error, true);
                    }
                    update_status(&app, &shared_status, &status);
                    continue;
                }
            }
        }

        if !xr_driving && !woken && started < next_tick {
            #[cfg(not(any(windows, linux_vr)))]
            let wake = next_tick;
            #[cfg(any(windows, linux_vr))]
            let mut wake = next_tick;
            #[cfg(any(windows, linux_vr))]
            if (state.ui.pointer_fast_tick() || state.player_audio.active())
                && let Some(backend) = state.backend.as_mut()
            {
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
            if let Some(delay) = wake.checked_duration_since(Instant::now()) {
                std::thread::park_timeout(delay);
            }
            continue;
        }

        let business = woken || business_clock.due(started);
        if woken {
            business_clock.0 = started + UPDATE_INTERVAL;
        }
        #[cfg(windows)]
        let mut open_ocr_bindings = false;
        #[cfg(any(windows, linux_vr))]
        let mut placement_mode = None;
        if business {
            let pending_config = lock_unpoisoned(&latest_config).take();
            if let Some((config, ocr_config)) = pending_config {
                #[cfg(any(windows, linux_vr))]
                if state.config != config {
                    state.config_revision = state.config_revision.wrapping_add(1);
                }
                #[cfg(any(windows, linux_vr))]
                if state.config.wrist != config.wrist
                    || state.config.translation_display != config.translation_display
                {
                    state.dialogue_policy += 1;
                }
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
                let backend_changed = state.config.backend != config.backend;
                state.config = config;
                if backend_changed {
                    if let Some(mut backend) = state.backend.take() {
                        backend.hide_all();
                    }
                    #[cfg(any(windows, linux_vr))]
                    state.reset_ui();
                    state.reset_headset();
                    state.reset_dashboard();
                    state.next_reconnect = Instant::now();
                    state.next_probe = Instant::now();
                    state.probe_ready = false;
                    state.probe_incompatibility = None;
                    state.incompatible_openvr = None;
                    state.xr_session_ready = false;
                    state.xr_stall_count = 0;
                    state.last_connect_error = None;
                    status.reconnect_attempt = 0;
                    status.last_error_detail = None;
                    state.selected_backend = None;
                }
                state.ocr_config = ocr_config;
            }
            let pending_dashboard = lock_unpoisoned(&latest_dashboard).take();
            if let Some(view) = pending_dashboard
                && state.dashboard_view.as_ref() != Some(&view)
            {
                #[cfg(any(windows, linux_vr))]
                if state.dashboard_view.as_ref().is_none_or(|previous| {
                    previous.caption_visibility != view.caption_visibility
                        || previous.max_visible_players != view.max_visible_players
                }) {
                    state.dialogue_policy += 1;
                }
                #[cfg(any(windows, linux_vr))]
                if state.dashboard_view.as_ref().is_none_or(|previous| {
                    previous.caption_visibility != view.caption_visibility
                        || previous.max_visible_players != view.max_visible_players
                        || previous.labels.captions != view.labels.captions
                }) {
                    state.dashboard_caption_revision =
                        state.dashboard_caption_revision.wrapping_add(1);
                }
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
            let control = std::mem::take(&mut *lock_unpoisoned(&pending_control));
            {
                #[cfg(any(windows, linux_vr))]
                {
                    placement_mode = control.placement_mode;
                }
                #[cfg(windows)]
                {
                    open_ocr_bindings = control.ocr_bindings;
                }
                if control.retry {
                    if let Some(mut backend) = state.backend.take() {
                        backend.hide_all();
                    }
                    state.selected_backend = None;
                    #[cfg(windows)]
                    {
                        state.ocr.clear();
                        state.reset_ui();
                    }
                    #[cfg(linux_vr)]
                    state.reset_ui();
                    state.reset_headset();
                    state.reset_dashboard();
                    #[cfg(any(windows, linux_vr))]
                    app.state::<super::surface::SurfaceHub>().retry(&app);
                    state.next_reconnect = Instant::now();
                    state.next_probe = Instant::now();
                    state.probe_ready = false;
                    state.probe_incompatibility = None;
                    state.incompatible_openvr = None;
                    state.xr_session_ready = false;
                    state.xr_stall_count = 0;
                    state.last_connect_error = None;
                    status.reconnect_attempt = 0;
                    status.last_error_detail = None;
                }
                if let Some(visible) = control.headset_sample {
                    state.headset_sample = visible;
                }
                if let Some(visible) = control.wrist_sample {
                    #[cfg(any(windows, linux_vr))]
                    if state.wrist_sample != visible {
                        state.dialogue_policy += 1;
                    }
                    state.wrist_sample = visible;
                }
            }
            for event in event_receiver.try_iter().take(EVENT_QUEUE_CAPACITY) {
                let now = Instant::now();
                state
                    .headset
                    .apply(event.clone(), now, &state.config.headset);
                state.wrist.apply(event, now, &state.config.wrist);
            }
        }

        let available = features.borrow().vr_overlay;
        #[cfg(any(windows, linux_vr))]
        if let Some(mode) = placement_mode
            && let Some(backend) = state.backend.as_ref()
        {
            state.ui.place(&app, backend, &state.controls, mode);
        }
        // The panic hook records details; keep the worker alive and reconnect.
        let ticked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            tick(&app, &mut state, &mut status, available, business)
        }));
        if ticked.is_err() {
            recover_from_panic(&mut state, &mut status);
        }
        status.requested_backend = state.config.backend.clone();
        status.backend = state.backend.as_ref().map(Backend::kind);
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
        #[cfg(any(windows, linux_vr))]
        if state.backend.is_some() || !state.surfaces_dormant {
            state
                .headset_surface
                .finish_tick(&app, state.backend.as_mut(), OverlayKind::Headset);
            state.ui.finish_surfaces(&app, state.backend.as_mut());
            state.dashboard_surface.finish_tick(
                &app,
                state.backend.as_mut(),
                OverlayKind::Dashboard,
            );
            state
                .player_audio
                .finish_surfaces(&app, state.backend.as_mut());
            #[cfg(windows)]
            state.ocr.finish_surfaces(&app, state.backend.as_mut());
            if state.backend.is_some() {
                state.surfaces_dormant = false;
            } else if !app
                .state::<super::surface::SurfaceHub>()
                .needs_surface_maintenance()
            {
                state.surfaces_dormant = true;
            }
        }
        if business {
            #[cfg(windows)]
            {
                status.ocr = state.ocr.status.clone();
            }
            update_status(&app, &shared_status, &status);
            let ready = |resource: &ResourceState| {
                matches!(
                    resource,
                    ResourceState::ReadyHidden | ResourceState::Visible | ResourceState::Fading
                )
            };
            lock_unpoisoned(&state.controls).set_overlay_effective(
                status.state == RuntimeState::Ready && ready(&status.wrist.state),
                status.state == RuntimeState::Ready && ready(&status.headset.state),
            );
        }
        #[cfg(any(windows, linux_vr))]
        let ui_active = state.backend.is_some()
            && (state.ui.fast_tick()
                || state.player_audio.active()
                || app.state::<super::surface::SurfaceHub>().needs_fast_tick());
        #[cfg(windows)]
        let ui_active = ui_active || (state.backend.is_some() && state.ocr.needs_fast_tick());
        #[cfg(not(any(windows, linux_vr)))]
        let ui_active = false;
        let interval = if state.backend.is_none() {
            if status.state == RuntimeState::Disabled {
                IDLE_DISABLED_INTERVAL
            } else {
                IDLE_WAITING_INTERVAL
            }
        } else if status.dashboard.visible || ui_active {
            SURFACE_FRAME_INTERVAL
        } else {
            UPDATE_INTERVAL
        };
        if state.backend.is_none() {
            next_tick = started + interval;
            business_clock.0 = next_tick;
        } else {
            next_tick = (started + interval).min(business_clock.0);
        }
        #[cfg(any(windows, linux_vr))]
        if let Some(backend) = state.backend.as_mut() {
            if state.player_audio.active() {
                state.player_audio.sync_cursor(backend);
            } else {
                state.ui.sync_cursor(&app, backend);
            }
            next_pointer = Instant::now() + backend.cursor_interval();
        }
        if xr_driving {
            let accepted = match state.backend.as_mut().map(Backend::end_frame) {
                Some(Ok(submitted)) => submitted,
                Some(Err(error)) => {
                    if is_openxr_session_end(&error) {
                        tracing::info!(%error, "OpenXR session ended; waiting for VR runtime");
                        detach_to_waiting_runtime(
                            &mut state,
                            &mut status,
                            None,
                            RECONNECT_INTERVAL,
                        );
                    } else {
                        tracing::warn!(%error, "OpenXR frame submission failed");
                        detach_for_reconnect(&mut state, &mut status, error, true);
                    }
                    update_status(&app, &shared_status, &status);
                    false
                }
                None => false,
            };
            #[cfg(not(any(windows, linux_vr)))]
            let _ = accepted;
            #[cfg(any(windows, linux_vr))]
            {
                state.headset_surface.confirm_frame(&app, accepted);
                state.dashboard_surface.confirm_frame(&app, accepted);
                state.ui.confirm_frames(&app, accepted);
                state.player_audio.confirm_frames(&app, accepted);
            }
            continue;
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
    #[cfg(linux_vr)]
    state.reset_ui();
    state.backend = None;
    if state.config.backend == "auto" {
        state.selected_backend = None;
    }
    state.reset_headset();
    state.reset_dashboard();
    state.probe_ready = false;
    state.xr_session_ready = false;
    state.next_probe = Instant::now();
    status.state = RuntimeState::Reconnecting;
    status.backend = None;
    status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
    state.next_reconnect = Instant::now() + reconnect_backoff(status.reconnect_attempt);
    status.last_error_detail = Some("VR Overlay worker panicked".into());
}

fn tick(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    available: bool,
    business: bool,
) {
    if state.backend.as_mut().is_some_and(Backend::quit_requested) {
        tracing::info!("VR runtime requested quit; detaching backend and keeping VRCS running");
        state.session.quit();
        state.last_connect_error = None;
        state.xr_stall_count = 0;
        detach_to_waiting_runtime(state, status, None, RECONNECT_INTERVAL);
        return;
    }

    if !available {
        #[cfg(windows)]
        {
            state.ocr.unavailable(false);
            state.reset_ui();
        }
        #[cfg(linux_vr)]
        state.reset_ui();
        if let Some(mut backend) = state.backend.take() {
            backend.hide_all();
        }
        state.selected_backend = None;
        state.probe_ready = false;
        state.probe_incompatibility = None;
        state.incompatible_openvr = None;
        state.xr_session_ready = false;
        state.xr_stall_count = 0;
        state.last_connect_error = None;
        state.next_probe = Instant::now();
        state.next_reconnect = Instant::now();
        state.headset_sample = false;
        state.wrist_sample = false;
        state.headset = HeadsetPresentation::default();
        state.headset.set_player_limit(Some(0));
        state.wrist = WristPresentation::default();
        state.wrist.set_player_limit(Some(0));
        state.reset_headset();
        state.reset_dashboard();
        status.state = RuntimeState::Disabled;
        status.backend = None;
        status.runtime_installed = false;
        status.hmd_present = false;
        status.reconnect_attempt = 0;
        status.last_error_detail = None;
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
    if state.backend.is_some() {
        status.runtime_installed = true;
        status.hmd_present = true;
    } else if Instant::now() >= state.next_probe {
        let runtime_path = Backend::openvr_runtime_path();
        let snapshot = evaluate_probe(
            state.config.backend.as_str(),
            state
                .incompatible_openvr
                .as_ref()
                .map(|(pid, reason)| (*pid, reason.as_str())),
            Backend::openxr_probe,
            Backend::runtime_installed,
            |explicit| openvr_server_pid(runtime_path.as_deref(), explicit),
            |pid| state.session.can_connect(pid),
            Backend::hmd_present,
        );
        state.selected_backend = snapshot.selected_backend;
        status.runtime_installed = snapshot.runtime_installed;
        status.hmd_present = snapshot.hmd_present;
        state.probe_ready = snapshot.ready_to_connect;
        state.probe_incompatibility = snapshot.incompatibility_detail;
        if snapshot.clear_incompatible_openvr {
            state.incompatible_openvr = None;
        }
        state.next_probe = Instant::now() + RECONNECT_INTERVAL;
    }
    let openxr = state.backend.as_ref().map_or(
        state.selected_backend == Some(BackendKind::OpenXr),
        |backend| backend.kind() == BackendKind::OpenXr,
    );
    status.headset.sample_visible = state.headset_sample;
    status.wrist.sample_visible = state.wrist_sample;

    if !cfg!(any(windows, linux_vr)) {
        status.state = RuntimeState::Unsupported;
        return;
    }
    let any_enabled =
        state.config.enabled && (state.config.headset.enabled || state.config.wrist.enabled);
    #[cfg(windows)]
    let ocr_enabled = state.ocr_config.enabled;
    #[cfg(not(windows))]
    let ocr_enabled = false;
    #[cfg(windows)]
    let vision_enabled = state.ocr.vision.enabled();
    #[cfg(not(windows))]
    let vision_enabled = false;
    let waiting_runtime = state.backend.is_none()
        && (!state.probe_ready || !status.runtime_installed || !status.hmd_present);
    if waiting_runtime {
        state.xr_stall_count = 0;
        reset_detached_state(state);
        apply_waiting_runtime(state, status, state.probe_incompatibility.clone());
        return;
    }

    if state.backend.is_none() && Instant::now() >= state.next_reconnect {
        #[cfg(windows)]
        state.ocr.unavailable(ocr_enabled);
        let pid = if openxr {
            None
        } else {
            openvr_server_pid(
                Backend::openvr_runtime_path().as_deref(),
                state.config.backend == "openvr",
            )
        };
        if !openxr
            && (!state.session.can_connect(pid)
                || pid.is_some_and(|pid| {
                    state
                        .incompatible_openvr
                        .as_ref()
                        .is_some_and(|(bad_pid, _)| *bad_pid == pid)
                }))
        {
            detach_to_waiting_runtime(
                state,
                status,
                state.probe_incompatibility.clone(),
                RECONNECT_INTERVAL,
            );
            return;
        }
        status.state = if status.reconnect_attempt == 0 {
            RuntimeState::Initializing
        } else {
            RuntimeState::Reconnecting
        };
        // Honor the one backend selected above. Re-running `auto` here can
        // choose a different backend than the frame/capability path expects.
        let preference = match state.selected_backend {
            Some(BackendKind::OpenXr) => "openxr",
            Some(BackendKind::OpenVr) => "openvr",
            None => &state.config.backend,
        };
        match Backend::connect(preference) {
            Ok(backend) => {
                if !openxr && let Some(pid) = pid {
                    state.session.connected(pid);
                }
                let kind = backend.kind();
                state.selected_backend = Some(kind);
                #[cfg(windows)]
                if let Ok(idx) = backend.surface_adapter_index() {
                    super::surface::warm_gpu(idx);
                }
                state.backend = Some(backend);
                state.last_connect_error = None;
                state.probe_incompatibility = None;
                if kind == BackendKind::OpenXr {
                    status.runtime_installed = true;
                    status.hmd_present = true;
                    state.xr_session_ready = false;
                    status.state = RuntimeState::Initializing;
                } else {
                    state.xr_session_ready = true;
                    state.xr_stall_count = 0;
                    status.state = RuntimeState::Ready;
                    status.last_connected_at = Some(unix_timestamp());
                    status.reconnect_attempt = 0;
                    status.last_error_detail = None;
                }
                #[cfg(linux_vr)]
                {
                    state.last_input_error = None;
                }
                #[cfg(any(windows, linux_vr))]
                state.reset_ui();
                state.reset_headset();
                state.reset_dashboard();
                #[cfg(windows)]
                state.ocr.reset_surfaces();
            }
            Err(error) => match classify_connect_error(&error) {
                ConnectFailureKind::MissingRuntime => {
                    if state.last_connect_error.as_deref() != Some(&error) {
                        tracing::info!(
                            error = %error,
                            "VR runtime unavailable during connect; waiting for VR runtime"
                        );
                        state.last_connect_error = Some(error);
                    }
                    state.xr_stall_count = 0;
                    detach_to_waiting_runtime(state, status, None, RECONNECT_INTERVAL);
                    return;
                }
                ConnectFailureKind::Incompatible => {
                    if state.last_connect_error.as_deref() != Some(&error) {
                        tracing::warn!(
                            error = %error,
                            "VR Overlay backend is incompatible with the active runtime"
                        );
                        state.last_connect_error = Some(error.clone());
                    }
                    if !openxr && let Some(pid) = pid {
                        state.incompatible_openvr = Some((pid, error.clone()));
                    }
                    state.xr_stall_count = 0;
                    state.probe_incompatibility = Some(error.clone());
                    detach_to_waiting_runtime(state, status, Some(error), RECONNECT_INTERVAL);
                    return;
                }
                ConnectFailureKind::Transient => {
                    status.reconnect_attempt = status.reconnect_attempt.saturating_add(1);
                    let delay = reconnect_backoff(status.reconnect_attempt);
                    if state.last_connect_error.as_deref() != Some(&error)
                        || status.reconnect_attempt <= 3
                    {
                        tracing::warn!(
                            error = %error,
                            reconnect_attempt = status.reconnect_attempt,
                            retry_in_secs = delay.as_secs(),
                            "VR Overlay connection failed"
                        );
                        state.last_connect_error = Some(error.clone());
                    }
                    state.xr_session_ready = false;
                    status.last_error_detail = Some(error);
                    status.state = RuntimeState::Reconnecting;
                    state.next_probe = Instant::now() + RECONNECT_INTERVAL;
                    state.next_reconnect = Instant::now() + delay;
                    return;
                }
            },
        }
    }

    let Some(mut backend) = state.backend.take() else {
        #[cfg(windows)]
        state.ocr.unavailable(ocr_enabled);
        #[cfg(linux_vr)]
        state.reset_ui();
        return;
    };
    let openxr = backend.kind() == BackendKind::OpenXr;
    if !openxr || state.xr_session_ready {
        status.state = RuntimeState::Ready;
        status.last_error_detail = None;
    }
    #[cfg(linux_vr)]
    if !openxr {
        if state.ocr_manifest.is_none()
            && let Some(directory) = app
                .path()
                .app_config_dir()
                .ok()
                .map(|path| path.join("steamvr-ocr"))
        {
            match super::ocr_input::install_manifest(&directory) {
                Ok(path) => state.ocr_manifest = Some(path),
                Err(error) => {
                    tracing::warn!(%error, "VR controller input manifest unavailable")
                }
            }
        }
        if let Some(path) = &state.ocr_manifest {
            match backend.update_ocr_input(path) {
                Ok(()) => state.last_input_error = None,
                Err(error) => {
                    if state.last_input_error.as_deref() != Some(&error) {
                        tracing::warn!(%error, "VR controller input unavailable");
                        state.last_input_error = Some(error);
                    }
                }
            }
        }
    }
    #[cfg(any(windows, linux_vr))]
    let modal = state
        .player_audio
        .tick(app, &mut backend, &state.controls, business);
    #[cfg(windows)]
    {
        if let Err(error) = state.ocr.prepare_input(&mut backend) {
            state.reset_ui();
            if state.ocr.status.last_error.as_ref() != Some(&error) {
                tracing::warn!(%error, "VR controller input unavailable");
            }
            state.ocr.status.last_error = Some(error);
        }
        let tick = super::ui_runtime::UiTick {
            reader_revision: state.ocr.input_revision(),
            content_owns_input: state.ocr.owns_input() || modal,
            business,
            dialogue_sample: state.wrist_sample,
            dialogue_content_active: state.wrist_sample
                || state.wrist.has_candidates()
                    && !state
                        .wrist
                        .idle_expired(Instant::now(), &state.config.wrist)
                || active_caption_notice(state, Instant::now()).0.is_some(),
            dialogue_retention: dialogue_retention(state),
        };
        let (events, focused) =
            state
                .ui
                .tick_with_cadence(app, &mut backend, &state.config, &state.controls, tick);
        state.ocr.pointer_input(events, focused || modal);
        if business {
            state.player_progress = lock_unpoisoned(&state.controls)
                .player_audio()
                .progress
                .clone();
        }
        state.ocr.tick_with_cadence(
            app,
            &mut backend,
            &state.ocr_config,
            &state.config.wrist,
            state.player_progress.as_ref(),
            business,
        );
    }
    #[cfg(linux_vr)]
    {
        let tick = super::ui_runtime::UiTick {
            reader_revision: 0,
            content_owns_input: modal,
            business,
            dialogue_sample: state.wrist_sample,
            dialogue_content_active: state.wrist_sample
                || state.wrist.has_candidates()
                    && !state
                        .wrist
                        .idle_expired(Instant::now(), &state.config.wrist)
                || active_caption_notice(state, Instant::now()).0.is_some(),
            dialogue_retention: dialogue_retention(state),
        };
        let _ = state
            .ui
            .tick_with_cadence(app, &mut backend, &state.config, &state.controls, tick);
    }
    if business {
        update_headset(app, state, status, &mut backend);
        update_wrist(app, state, status, &mut backend);
    } else {
        #[cfg(any(windows, linux_vr))]
        {
            match state
                .headset_surface
                .refresh(app, &mut backend, OverlayKind::Headset, None)
                .and_then(|visible| {
                    if visible {
                        backend.show(OverlayKind::Headset)?;
                    }
                    Ok(visible)
                }) {
                Err(error) => {
                    backend.reset(OverlayKind::Headset);
                    state.reset_headset();
                    resource_error(
                        &mut status.headset,
                        "headset_render",
                        error,
                        &mut status.last_error_detail,
                    );
                }
                Ok(true) => {
                    if status.headset.state != ResourceState::Fading {
                        status.headset.state = ResourceState::Visible;
                    }
                    status.headset.last_error_code = None;
                }
                Ok(false)
                    if matches!(
                        status.headset.state,
                        ResourceState::Visible | ResourceState::Fading
                    ) =>
                {
                    status.headset.state = ResourceState::Creating;
                }
                _ => {}
            }
            refresh_wrist(app, state, status, &mut backend);
        }
    }
    update_dashboard(app, state, status, &mut backend, business);
    let headset_failed =
        state.config.headset.enabled && status.headset.state == ResourceState::Error;
    let wrist_failed = state.config.wrist.enabled && status.wrist.state == ResourceState::Error;
    #[cfg(any(windows, linux_vr))]
    let (headset_failed, wrist_failed) = {
        let hub = app.state::<super::surface::SurfaceHub>();
        // Wrist captions render through Dialogue; the wrist menu is a separate surface.
        (
            headset_failed && !hub.recovery_failed(super::surface_protocol::SurfaceId::Headset),
            wrist_failed && !hub.recovery_failed(super::surface_protocol::SurfaceId::Dialogue),
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
        state.backend = Some(backend);
        detach_for_reconnect(state, status, error, true);
    } else {
        state.backend = Some(backend);
    }
}

#[cfg(any(windows, linux_vr))]
fn update_dashboard(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut Backend,
    business: bool,
) {
    use super::dashboard::{DASHBOARD_HEIGHT, DASHBOARD_WIDTH};
    use super::surface::SurfaceHub;
    use super::surface_protocol::{SurfaceId, SurfaceView};
    let Some(view) = state.dashboard_view.as_mut() else {
        status.dashboard.state = ResourceState::Disabled;
        status.dashboard.visible = false;
        return;
    };
    if business {
        let notice = lock_unpoisoned(&state.controls)
            .player_audio()
            .notice
            .clone();
        if view.player_audio_notice != notice {
            Arc::make_mut(view).player_audio_notice = notice;
        }
    }
    let view = view.clone();
    // Error backoff is not a user hide and must not reset the Surface recovery budget.
    if backend.dashboard_visible()
        && let Err(error) = state.dashboard_surface.request(app)
    {
        tracing::warn!(%error, "Failed to request VR dashboard surface");
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
        // The OpenXR settings quad can open on this tick (menu button or wrist
        // menu); request its surface before the refresh path skips a dormant tab.
        if backend.dashboard_visible() {
            state.dashboard_surface.request(app)?;
        }
        let hub = app.state::<SurfaceHub>();
        // Keep the tab registered, but never wake its WebView before it is opened.
        if !backend.dashboard_visible() {
            let _ = backend.poll_dashboard_events()?;
            state.dashboard.clear_pointer();
            state.dashboard_pressed_key = None;
            state.dashboard_repeat = None;
            return Ok(false);
        }
        let dark = lock_unpoisoned(&state.controls).dark();
        // Revoke stale input before polling; only the renderer owns recovery decisions.
        if business {
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
        }
        let previous_ui = state.dashboard.ui.clone();
        let mut layout = hub.layout(SurfaceId::Dashboard);
        for event in backend.poll_dashboard_events()? {
            let (x, y) = match event {
                DashboardPointerEvent::Move { x, y }
                | DashboardPointerEvent::Down { x, y }
                | DashboardPointerEvent::Up { x, y } => (x, y),
                DashboardPointerEvent::Scroll { delta } => {
                    if let Err(error) = hub.scroll(app, SurfaceId::Dashboard, delta) {
                        tracing::debug!(%error, "VR dashboard scroll unavailable");
                    }
                    continue;
                }
                DashboardPointerEvent::Shown | DashboardPointerEvent::Hidden => {
                    state.dashboard.clear_pointer();
                    state.dashboard_pressed_key = None;
                    state.dashboard_repeat = None;
                    continue;
                }
            };
            let point = [x / DASHBOARD_WIDTH as f32, y / DASHBOARD_HEIGHT as f32];
            let hit = layout.as_ref().and_then(|layout| layout.hit(point));
            state.dashboard.hovered = hit;
            match event {
                DashboardPointerEvent::Move { .. } => {
                    if layout.is_some()
                        && state
                            .dashboard_repeat
                            .as_ref()
                            .is_some_and(|repeat| hit != Some(repeat.region))
                    {
                        state.dashboard_repeat = None;
                    }
                }
                DashboardPointerEvent::Down { .. } => {
                    state.dashboard.pressed = hit;
                    state.dashboard_pressed_key = layout.as_ref().map(|layout| layout.key);
                    state.dashboard_repeat = if view.player_audio_notice.is_none() {
                        hit.and_then(|id| {
                            layout.as_ref().and_then(|layout| {
                                layout
                                    .regions
                                    .iter()
                                    .find(|region| region.id == id)
                                    .and_then(|region| region.dashboard.as_ref())
                                    .filter(|command| command.is_repeatable())
                                    .map(|command| DashboardRepeat {
                                        region: id,
                                        command: command.clone(),
                                        next_at: Instant::now() + Duration::from_millis(420),
                                        repeated: false,
                                    })
                            })
                        })
                    } else {
                        None
                    };
                }
                DashboardPointerEvent::Scroll { .. } => {}
                DashboardPointerEvent::Shown | DashboardPointerEvent::Hidden => {}
                DashboardPointerEvent::Up { .. } => {
                    let pressed = state.dashboard.pressed.take();
                    let key = state.dashboard_pressed_key.take();
                    let repeated = state
                        .dashboard_repeat
                        .take()
                        .is_some_and(|repeat| repeat.repeated);
                    let command = (!repeated)
                        .then(|| {
                            layout
                                .as_ref()
                                .filter(|layout| Some(layout.key) == key)
                                .filter(|_| hit.is_some() && hit == pressed)
                                .and_then(|layout| {
                                    layout.regions.iter().find(|region| Some(region.id) == hit)
                                })
                                .and_then(|region| region.dashboard.clone())
                        })
                        .flatten();
                    if let Some(command) = command {
                        let dismiss = matches!(command, super::dashboard::DashboardCommand::Close)
                            && state.dashboard.ui.picker.is_none()
                            && state.dashboard.ui.display_editor.is_none();
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
                        } else if view.player_audio_notice.is_none()
                            && let Some(action) = state.dashboard.activate(command, &view)
                        {
                            dispatch_dashboard_action(app, state, backend, action);
                        }
                        if dismiss {
                            backend.close_dashboard();
                            state.dashboard.clear_pointer();
                            state.dashboard_pressed_key = None;
                            state.dashboard_repeat = None;
                        }
                        // Any subsequent input must wait for the newly rendered layout.
                        layout = None;
                    }
                }
            }
        }
        if let Some(mut repeat) = state.dashboard_repeat.take() {
            let keep = if view.player_audio_notice.is_some() {
                false
            } else {
                match layout.or_else(|| hub.layout(SurfaceId::Dashboard)) {
                    None => true,
                    Some(current_layout) => {
                        if state.dashboard.hovered != Some(repeat.region) {
                            false
                        } else if Instant::now() >= repeat.next_at {
                            let matches_region = current_layout.regions.iter().any(|region| {
                                region.id == repeat.region
                                    && region.dashboard.as_ref() == Some(&repeat.command)
                            });
                            if matches_region {
                                if let Some(action) = state
                                    .dashboard
                                    .activate_repeat(repeat.command.clone(), &view)
                                {
                                    dispatch_dashboard_action(app, state, backend, action);
                                    repeat.repeated = true;
                                    repeat.next_at = Instant::now() + Duration::from_millis(110);
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        } else {
                            true
                        }
                    }
                }
            };
            if keep {
                state.dashboard_repeat = Some(repeat);
            }
        }
        let ready = if business || previous_ui != state.dashboard.ui {
            state.dashboard_surface.present(
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
            )?
        } else {
            state.dashboard_surface.refresh(
                app,
                backend,
                OverlayKind::Dashboard,
                Some((state.dashboard.hovered, state.dashboard.pressed, None)),
            )?
        };
        if !ready {
            backend.show_dashboard_placeholder(None)?;
        }
        Ok(ready)
    })();
    match result {
        Ok(ready) => {
            state.last_dashboard_error = None;
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
            if cfg!(linux_vr)
                && backend.kind() == BackendKind::OpenVr
                && !backend.dashboard_visible()
            {
                if state.last_dashboard_error.as_deref() != Some(&error) {
                    tracing::warn!(%error, "VR dashboard overlay unavailable; keeping overlays active");
                }
                state.last_dashboard_error = Some(error);
                state.dashboard_retry_at = Some(Instant::now() + Duration::from_secs(10));
                status.dashboard.visible = false;
                status.dashboard.state = ResourceState::DeviceUnavailable;
                status.dashboard.last_error_code = None;
                return;
            }
            if let Err(placeholder_error) = backend.show_dashboard_placeholder(Some(&error)) {
                tracing::warn!(%placeholder_error, "VR dashboard error display failed");
            }
            // `reset_dashboard` cleared the renderer request flag. Keep the
            // OpenXR modal alive so its error placeholder is not hidden by
            // the same tick's `finish_tick`.
            if backend.dashboard_visible()
                && let Err(request_error) = state.dashboard_surface.request(app)
            {
                tracing::warn!(
                    error = %request_error,
                    "Failed to request VR dashboard surface"
                );
            }
            state.dashboard_retry_at = Some(Instant::now() + Duration::from_secs(1));
            status.dashboard.visible = false;
            status.dashboard.state = ResourceState::Error;
            status.dashboard.last_error_code = Some("dashboard_surface".into());
            if state.last_dashboard_error.as_deref() != Some(&error)
                && status.last_error_detail.as_deref() != Some(&error)
            {
                tracing::warn!(%error, "VR dashboard surface unavailable");
            }
            state.last_dashboard_error = Some(error.clone());
            status.last_error_detail = Some(error);
        }
    }
}

#[cfg(any(windows, linux_vr))]
fn dispatch_dashboard_action(
    app: &AppHandle,
    state: &mut WorkerState,
    backend: &Backend,
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
    let control = {
        let hub = lock_unpoisoned(&state.controls);
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
            DashboardAction::ToggleNetworkCapture => Some(ControlAction::SetNetworkCaptureRunning(
                !view.items[5].requested,
            )),
            _ => None,
        }
    };
    if let Some(control) = control {
        dispatch_control(app, &state.controls, ControlSource::Dashboard, control);
    } else if let Err(error) = app.emit_to("main", "vr-dashboard-action", action) {
        tracing::warn!(%error, "SteamVR dashboard action emit failed");
    }
}

#[cfg(all(not(windows), not(linux_vr)))]
fn update_dashboard(
    _app: &AppHandle,
    _state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    _backend: &mut Backend,
    _business: bool,
) {
    status.dashboard.state = ResourceState::Disabled;
    status.dashboard.visible = false;
}

#[cfg(any(windows, linux_vr, test))]
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
    backend: &mut Backend,
) {
    #[cfg(all(not(windows), not(linux_vr)))]
    let _ = app;
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

    #[cfg(any(windows, linux_vr))]
    let presented = (|| {
        use super::surface_protocol::SurfaceView;
        let now = Instant::now();
        let draft = question_draft.as_deref();
        let (notice, notice_error) = active_caption_notice(state, now);
        let dark = lock_unpoisoned(&state.controls).dark();
        let headset_revision = state.headset.revision();
        if let Some(cache) = state.headset_cache.as_ref().filter(|cache| {
            cache.headset_revision == headset_revision
                && cache.config_revision == state.config_revision
                && cache.dashboard_revision == state.dashboard_caption_revision
                && cache.sample == state.headset_sample
                && cache.dark == dark
                && cache.draft.as_deref() == draft
                && cache.notice.as_deref() == notice.as_deref()
                && cache.notice_error == notice_error
                && cache.next_change.is_none_or(|at| now < at)
                && (cache.frame_opacity.is_none() || state.headset_surface.has_published_key())
        }) {
            let Some(frame_opacity) = cache.frame_opacity else {
                backend.hide(OverlayKind::Headset);
                status.headset.state = ResourceState::ReadyHidden;
                status.headset.last_error_code = None;
                return Ok(None);
            };
            let visible =
                state
                    .headset_surface
                    .refresh(app, backend, OverlayKind::Headset, None)?;
            let shown = backend
                .set_opacity(
                    OverlayKind::Headset,
                    state.config.headset.opacity
                        * if notice.is_some() { 1.0 } else { frame_opacity },
                )
                .and_then(|()| {
                    if visible {
                        backend.show(OverlayKind::Headset)
                    } else {
                        backend.hide(OverlayKind::Headset);
                        Ok(())
                    }
                });
            if shown.is_err() {
                state.reset_headset();
            }
            shown?;
            return Ok(Some((visible, frame_opacity)));
        }

        let frame = if let Some(ref draft) = question_draft {
            Some(question_draft_frame(draft))
        } else if state.headset_sample {
            Some(PresentationFrame::headset(
                "VRCS Headset Overlay Sample / 视野字幕预览",
                1.0,
            ))
        } else {
            state.headset.frame_with_translation_display(
                now,
                &state.config.headset,
                &state.config.translation_display,
            )
        };
        let frame = frame.or_else(|| {
            notice.as_ref().map(|_| PresentationFrame {
                content: PresentationContent::Headset(Vec::new()),
                opacity: 1.0,
            })
        });
        let next_change = if draft.is_some() || state.headset_sample {
            None
        } else {
            state.headset.next_change(now, &state.config.headset)
        };
        let Some(frame) = frame else {
            state.headset_cache = Some(HeadsetPublishCache {
                headset_revision,
                config_revision: state.config_revision,
                dashboard_revision: state.dashboard_caption_revision,
                sample: state.headset_sample,
                dark,
                draft: None,
                notice,
                notice_error,
                frame_opacity: None,
                next_change,
            });
            backend.hide(OverlayKind::Headset);
            status.headset.state = ResourceState::ReadyHidden;
            status.headset.last_error_code = None;
            return Ok(None);
        };

        let PresentationContent::Headset(messages) = frame.content else {
            return Err("Invalid headset content".into());
        };
        let frame_opacity = frame.opacity;
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
                notice_error,
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
        let shown = backend
            .set_opacity(
                OverlayKind::Headset,
                state.config.headset.opacity * if notice.is_some() { 1.0 } else { frame_opacity },
            )
            .and_then(|()| {
                if visible {
                    backend.show(OverlayKind::Headset)
                } else {
                    backend.hide(OverlayKind::Headset);
                    Ok(())
                }
            });
        if shown.is_err() {
            state.reset_headset();
        }
        shown?;
        state.headset_cache =
            state
                .headset_surface
                .has_published_key()
                .then_some(HeadsetPublishCache {
                    headset_revision,
                    config_revision: state.config_revision,
                    dashboard_revision: state.dashboard_caption_revision,
                    sample: state.headset_sample,
                    dark,
                    draft: question_draft,
                    notice,
                    notice_error,
                    frame_opacity: Some(frame_opacity),
                    next_change,
                });
        Ok(Some((visible, frame_opacity)))
    })();
    #[cfg(all(not(windows), not(linux_vr)))]
    let presented: Result<Option<(bool, f32)>, String> =
        Err("VR Overlay is only supported on Windows".into());
    match presented {
        Ok(None) => {}
        Ok(Some((visible, frame_opacity))) => {
            status.headset.state = if !visible {
                ResourceState::Creating
            } else if frame_opacity < 1.0 {
                ResourceState::Fading
            } else {
                ResourceState::Visible
            };
            status.headset.last_error_code = None;
        }
        Err(error) => {
            backend.reset(OverlayKind::Headset);
            #[cfg(any(windows, linux_vr))]
            {
                state.headset_cache = None;
            }
            resource_error(
                &mut status.headset,
                "headset_render",
                error,
                &mut status.last_error_detail,
            );
        }
    }
}

#[cfg(any(windows, linux_vr))]
fn dialogue_retention(state: &WorkerState) -> super::ui_runtime::DialogueRetentionKey {
    super::ui_runtime::DialogueRetentionKey {
        content: state.wrist.retention_revision(),
        policy: state.dialogue_policy,
    }
}

#[cfg(any(windows, linux_vr))]
fn refresh_wrist(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut Backend,
) {
    render_wrist(app, state, status, backend, false);
}

#[cfg(any(windows, linux_vr))]
fn update_wrist(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut Backend,
) {
    render_wrist(app, state, status, backend, true);
}

#[cfg(any(windows, linux_vr))]
fn render_wrist(
    app: &AppHandle,
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut Backend,
    business: bool,
) {
    use super::surface_protocol::SurfaceView;
    let now = Instant::now();
    if !state.config.enabled || !state.config.wrist.enabled {
        backend.reset(OverlayKind::Wrist);
        state.dialogue_cache = None;
        state.ui.reset_dialogue_visibility();
        status.wrist.state = ResourceState::Disabled;
        status.wrist.bound_role = None;
        status.wrist.tracked_device_available = false;
        status.wrist.last_error_code = None;
        return;
    }
    let retention = dialogue_retention(state);
    if state
        .ui
        .dialogue_retention
        .is_some_and(|previous| previous.policy != retention.policy)
    {
        state.ui.reset_dialogue_visibility();
    }
    let (mode, pose, relative, follow_view, busy, error, revision) = {
        let placement = lock_unpoisoned(&state.ui.placement);
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
        match (pose, state.ui.current_origin()) {
            (Some(pose), Some(origin)) => {
                backend.ensure_world_wrist(&state.config.wrist, pose, origin, relative)
            }
            _ => {
                backend.hide(OverlayKind::Wrist);
                state.dialogue_cache = None;
                state.ui.reset_dialogue_visibility();
                status.wrist.state = ResourceState::DeviceUnavailable;
                status.wrist.tracked_device_available = false;
                return;
            }
        }
    } else if business {
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
    } else {
        Ok(())
    };
    if let Err(error) = setup {
        backend.hide(OverlayKind::Wrist);
        state.dialogue_cache = None;
        state.ui.reset_dialogue_visibility();
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
    if !state.ui.dialogue_available || state.ui.dialogue_occluded {
        backend.hide(OverlayKind::Wrist);
        state.dialogue_cache = None;
        state.ui.reset_dialogue_visibility();
        status.wrist.tracked_device_available = state.ui.dialogue_available;
        status.wrist.state = if state.ui.dialogue_available {
            ResourceState::ReadyHidden
        } else {
            ResourceState::DeviceUnavailable
        };
        return;
    }
    let target = state.ui.dialogue_visible;
    if !business && target && state.ui.dialogue_pending.is_none() {
        backend.hide(OverlayKind::Wrist);
        return;
    }
    let result = (|| -> Result<(), String> {
        let ready =
            if !target {
                state.dialogue_cache = None;
                state.ui.dialogue_retention == Some(retention)
                    && state.ui.dialogue_fade.active()
                    && state
                        .ui
                        .dialogue_surface
                        .hold_for_fade(app, backend, OverlayKind::Wrist)
            } else if !business {
                state.ui.dialogue_surface.refresh(
                    app,
                    backend,
                    OverlayKind::Wrist,
                    Some((state.ui.dialogue_hover, None, None)),
                )?
            } else {
                let (notice, notice_error) = active_caption_notice(state, now);
                let wrist_revision = state.wrist.revision();
                let controls_version = lock_unpoisoned(&state.controls).ui_version();
                if let Some(cache) = state.dialogue_cache.as_ref().filter(|cache| {
                    cache.wrist_revision == wrist_revision
                        && cache.config_revision == state.config_revision
                        && cache.dashboard_revision == state.dashboard_caption_revision
                        && cache.controls_version == controls_version
                        && cache.sample == state.wrist_sample
                        && cache.notice.as_deref() == notice.as_deref()
                        && cache.notice_error == notice_error
                        && cache.mode == mode
                        && cache.follow_view == follow_view
                        && cache.busy == busy
                        && cache.error.as_deref() == error.as_deref()
                        && cache.idle_expires_at.is_none_or(|at| now < at)
                        && (!cache.has_frame
                            || (state.ui.dialogue_pending == Some(retention)
                                && state.ui.dialogue_surface.has_published_key()))
                }) {
                    if !cache.has_frame {
                        state.ui.reset_dialogue_visibility();
                        backend.hide(OverlayKind::Wrist);
                        status.wrist.state = ResourceState::ReadyHidden;
                        status.wrist.last_error_code = None;
                        return Ok(());
                    }
                    state.ui.dialogue_surface.refresh(
                        app,
                        backend,
                        OverlayKind::Wrist,
                        Some((state.ui.dialogue_hover, None, None)),
                    )?
                } else {
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
                            now,
                            &state.config.wrist,
                            &state.config.translation_display,
                        )
                    };
                    let frame = frame.or_else(|| {
                        notice
                            .as_ref()
                            .map(|_| PresentationFrame::wrist(Vec::new()))
                    });
                    let idle_expires_at = if state.wrist_sample {
                        None
                    } else {
                        state.wrist.idle_expires_at(&state.config.wrist)
                    };
                    let Some(frame) = frame else {
                        state.dialogue_cache = Some(DialoguePublishCache {
                            wrist_revision,
                            config_revision: state.config_revision,
                            dashboard_revision: state.dashboard_caption_revision,
                            controls_version,
                            sample: state.wrist_sample,
                            notice,
                            notice_error,
                            mode,
                            follow_view,
                            busy,
                            error,
                            idle_expires_at,
                            has_frame: false,
                        });
                        state.ui.reset_dialogue_visibility();
                        backend.hide(OverlayKind::Wrist);
                        status.wrist.state = ResourceState::ReadyHidden;
                        status.wrist.last_error_code = None;
                        return Ok(());
                    };
                    let controls = lock_unpoisoned(&state.controls).snapshot();
                    let messages = match frame.content {
                        PresentationContent::Wrist(messages) => messages,
                        _ => Vec::new(),
                    };
                    let ready = state.ui.dialogue_surface.present(
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
                            notice: notice.clone(),
                            notice_error,
                            labels: state
                                .dashboard_view
                                .as_ref()
                                .map(|view| view.labels.captions.clone())
                                .unwrap_or_default(),
                            mode,
                            follow_view,
                            busy,
                            error: error.clone(),
                            font_size: state.config.wrist.font_size_px,
                            opacity: state.config.wrist.background_opacity,
                        },
                        state.ui.dialogue_hover,
                        None,
                    )?;
                    state.ui.dialogue_pending = Some(retention);
                    state.dialogue_cache = state.ui.dialogue_surface.has_published_key().then_some(
                        DialoguePublishCache {
                            wrist_revision,
                            config_revision: state.config_revision,
                            dashboard_revision: state.dashboard_caption_revision,
                            controls_version,
                            sample: state.wrist_sample,
                            notice,
                            notice_error,
                            mode,
                            follow_view,
                            busy,
                            error,
                            idle_expires_at,
                            has_frame: true,
                        },
                    );
                    ready
                }
            };
        if state.ui.dialogue_surface.current_frame(app) {
            state.ui.dialogue_retention = state.ui.dialogue_pending;
        }
        // Ordinary content updates keep their current alpha while the renderer replaces pixels.
        let ready = ready
            && (target && state.ui.dialogue_fade.active()
                || state.ui.dialogue_retention == Some(retention));
        let fade = state.ui.dialogue_fade.advance(target, ready, now);
        state.ui.dialogue_active = target || fade.animating;
        state.ui.dialogue_revision = ready.then_some(revision);
        if ready && (fade.factor > 0.0 || fade.animating) {
            backend.set_opacity(OverlayKind::Wrist, state.config.wrist.opacity * fade.factor)?;
            backend.show(OverlayKind::Wrist)?;
        } else {
            backend.hide(OverlayKind::Wrist);
        }
        status.wrist.state = if fade.animating {
            ResourceState::Fading
        } else if target && ready {
            ResourceState::Visible
        } else if target {
            ResourceState::Creating
        } else {
            ResourceState::ReadyHidden
        };
        if !target && !fade.animating {
            state
                .ui
                .dialogue_surface
                .stop(app, Some(backend), OverlayKind::Wrist);
        }
        status.wrist.last_error_code = None;
        Ok(())
    })();
    if let Err(error) = result {
        state.dialogue_cache = None;
        state.ui.reset_dialogue_visibility();
        backend.reset(OverlayKind::Wrist);
        wrist_error(status, "wrist_render", error);
    }
}

#[cfg(all(not(windows), not(linux_vr)))]
fn update_wrist(
    _app: &AppHandle,
    _state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    _backend: &mut Backend,
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

#[cfg(any(windows, linux_vr))]
fn wrist_error(status: &mut VrOverlayStatus, code: &str, detail: String) {
    status.wrist.state = ResourceState::Error;
    status.wrist.last_error_code = Some(code.into());
    status.last_error_detail = Some(detail);
}

fn update_status(app: &AppHandle, shared: &Arc<Mutex<VrOverlayStatus>>, next: &VrOverlayStatus) {
    let changed = {
        let mut current = lock_unpoisoned(shared);
        if *current != *next {
            *current = next.clone();
            true
        } else {
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
    fn business_clock_stays_at_twenty_hz_during_fast_interaction() {
        let now = Instant::now();
        let mut clock = BusinessClock(now);
        let updates = (0..60)
            .filter(|tick| clock.due(now + SURFACE_FRAME_INTERVAL * *tick))
            .count();
        assert_eq!(updates, 20);
        let resumed = now + Duration::from_secs(5);
        assert!(clock.due(resumed));
        assert!(!clock.due(resumed));
        assert!(!clock.due(resumed + Duration::from_millis(49)));
        assert!(clock.due(resumed + Duration::from_millis(50)));
    }

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

    #[test]
    fn explicit_openxr_waits_without_reconnecting_when_no_vr_runtime_or_hmd_exists() {
        let missing_loader = evaluate_probe(
            "openxr",
            None,
            || OpenXrProbe {
                runtime_installed: false,
                hmd_present: false,
                detail: Some("vr.openxr_loader_unavailable".into()),
            },
            || unreachable!("openxr must not probe openvr"),
            |_| unreachable!(),
            |_| unreachable!(),
            || unreachable!(),
        );
        assert!(!missing_loader.ready_to_connect);
        assert_eq!(missing_loader.selected_backend, Some(BackendKind::OpenXr));
        assert_eq!(missing_loader.incompatibility_detail, None);

        let missing_hmd = evaluate_probe(
            "openxr",
            None,
            || OpenXrProbe {
                runtime_installed: true,
                hmd_present: false,
                detail: None,
            },
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            || unreachable!(),
        );
        assert!(!missing_hmd.ready_to_connect);
        assert!(missing_hmd.runtime_installed);
        assert!(!missing_hmd.hmd_present);
        assert_eq!(missing_hmd.incompatibility_detail, None);

        let unsupported_platform = evaluate_probe(
            "openxr",
            None,
            || OpenXrProbe {
                runtime_installed: false,
                hmd_present: false,
                detail: Some("vr.openxr_backend_unavailable".into()),
            },
            || unreachable!(),
            |_| unreachable!(),
            |_| unreachable!(),
            || unreachable!(),
        );
        assert!(!unsupported_platform.ready_to_connect);
        assert_eq!(
            unsupported_platform.incompatibility_detail.as_deref(),
            Some("vr.openxr_backend_unavailable")
        );
    }

    #[test]
    fn openvr_and_auto_wait_without_probing_hmd_when_no_server_is_running() {
        let mut hmd_probed = false;
        let openvr_idle = evaluate_probe(
            "openvr",
            None,
            || unreachable!(),
            || true,
            |explicit| {
                assert!(explicit);
                None
            },
            |pid| pid.is_some(),
            || {
                hmd_probed = true;
                true
            },
        );
        assert!(!hmd_probed);
        assert!(!openvr_idle.ready_to_connect);
        assert!(openvr_idle.runtime_installed);
        assert!(!openvr_idle.hmd_present);

        let auto_idle = evaluate_probe(
            "auto",
            None,
            || OpenXrProbe {
                runtime_installed: false,
                hmd_present: false,
                detail: Some("vr.openxr_loader_unavailable".into()),
            },
            || true,
            |explicit| {
                assert!(!explicit);
                None
            },
            |pid| pid.is_some(),
            || {
                hmd_probed = true;
                true
            },
        );
        assert!(!hmd_probed);
        assert!(!auto_idle.ready_to_connect);
        assert!(!auto_idle.hmd_present);
        assert_eq!(auto_idle.selected_backend, Some(BackendKind::OpenVr));
    }

    #[test]
    fn auto_prefers_openxr_when_ready_and_uses_non_explicit_openvr_gate_otherwise() {
        let xr_ready = evaluate_probe(
            "auto",
            None,
            || OpenXrProbe {
                runtime_installed: true,
                hmd_present: true,
                detail: None,
            },
            || unreachable!("auto must not touch openvr when openxr is ready"),
            |_| unreachable!(),
            |_| unreachable!(),
            || unreachable!(),
        );
        assert!(xr_ready.ready_to_connect);
        assert_eq!(xr_ready.selected_backend, Some(BackendKind::OpenXr));

        let ovr_ready = evaluate_probe(
            "auto",
            None,
            || OpenXrProbe {
                runtime_installed: true,
                hmd_present: false,
                detail: None,
            },
            || true,
            |explicit| {
                assert!(!explicit);
                Some(1234)
            },
            |pid| pid == Some(1234),
            || true,
        );
        assert!(ovr_ready.ready_to_connect);
        assert_eq!(ovr_ready.selected_backend, Some(BackendKind::OpenVr));
        assert_eq!(ovr_ready.openvr_pid, Some(1234));
    }

    #[test]
    fn incompatible_openvr_server_is_suppressed_until_pid_changes() {
        let mut hmd_probed = false;
        let same_pid = evaluate_probe(
            "openvr",
            Some((
                42,
                "OpenVR initialization failed: Init_InvalidApplicationType",
            )),
            || unreachable!(),
            || true,
            |_| Some(42),
            |_| unreachable!(),
            || {
                hmd_probed = true;
                true
            },
        );
        assert!(!hmd_probed);
        assert!(!same_pid.ready_to_connect);
        assert!(!same_pid.clear_incompatible_openvr);
        assert_eq!(
            same_pid.incompatibility_detail.as_deref(),
            Some("OpenVR initialization failed: Init_InvalidApplicationType")
        );

        let restarted_pid = evaluate_probe(
            "openvr",
            Some((
                42,
                "OpenVR initialization failed: Init_InvalidApplicationType",
            )),
            || unreachable!(),
            || true,
            |_| Some(43),
            |pid| pid == Some(43),
            || true,
        );
        assert!(restarted_pid.ready_to_connect);
        assert!(restarted_pid.clear_incompatible_openvr);
        assert_eq!(restarted_pid.incompatibility_detail, None);
    }

    #[test]
    fn connect_error_classification_and_backoff_are_bounded() {
        assert_eq!(
            classify_connect_error("vr.openxr_headset_unavailable"),
            ConnectFailureKind::MissingRuntime
        );
        assert_eq!(
            classify_connect_error("OpenXR runtime unavailable: ERROR_RUNTIME_FAILURE"),
            ConnectFailureKind::MissingRuntime
        );
        assert_eq!(
            classify_connect_error("OpenVR initialization failed: Init_HmdNotFoundPresenceFailed"),
            ConnectFailureKind::MissingRuntime
        );
        assert_eq!(
            classify_connect_error("OpenVR initialization failed: Init_NoServerForBackgroundApp"),
            ConnectFailureKind::MissingRuntime
        );
        assert_eq!(
            classify_connect_error("OpenVR initialization failed: Init_InvalidApplicationType"),
            ConnectFailureKind::Incompatible
        );
        assert_eq!(
            classify_connect_error("vr.openxr_overlay_unsupported"),
            ConnectFailureKind::Incompatible
        );
        assert_eq!(
            classify_connect_error("OpenVR overlay interface failed: unknown"),
            ConnectFailureKind::Transient
        );
        assert!(is_openxr_session_end("vr.openxr_session_ended"));
        assert!(is_openxr_session_end(
            "Frame wait failed: ERROR_SESSION_LOST"
        ));
        assert!(!is_openxr_session_end(
            "Frame wait failed: ERROR_RUNTIME_FAILURE"
        ));

        assert_eq!(reconnect_backoff(1), Duration::from_secs(2));
        assert_eq!(reconnect_backoff(2), Duration::from_secs(4));
        assert_eq!(reconnect_backoff(3), Duration::from_secs(8));
        assert_eq!(reconnect_backoff(4), Duration::from_secs(16));
        assert_eq!(reconnect_backoff(5), Duration::from_secs(30));
        assert_eq!(reconnect_backoff(99), Duration::from_secs(30));
    }
}
