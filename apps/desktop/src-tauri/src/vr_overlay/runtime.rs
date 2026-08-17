use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, watch};
use vrcs_core::{PresentationEvent, VrOverlayConfig};

use super::backend::{OpenVrBackend, OverlayKind};
use super::presentation::{
    HeadsetPresentation, MessageSide, PresentationFrame, WristMessage, WristPresentation,
};
use super::renderer::{self, Layout};

pub const STATUS_EVENT: &str = "vr-overlay-status-changed";
const UPDATE_INTERVAL: Duration = Duration::from_millis(50);
const RECONNECT_INTERVAL: Duration = Duration::from_secs(2);
const EVENT_QUEUE_CAPACITY: usize = 256;
const DROP_WARNING_INTERVAL: Duration = Duration::from_secs(5);

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
pub struct VrOverlayStatus {
    pub state: RuntimeState,
    pub runtime_installed: bool,
    pub hmd_present: bool,
    pub last_connected_at: Option<String>,
    pub reconnect_attempt: u32,
    pub headset: ResourceStatus,
    pub wrist: WristStatus,
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
            last_error_detail: None,
        }
    }
}

#[derive(Default)]
struct PendingControl {
    retry: bool,
    headset_sample: Option<bool>,
    wrist_sample: Option<bool>,
}

pub struct Manager {
    app: AppHandle,
    status: Arc<Mutex<VrOverlayStatus>>,
    event_sender: Mutex<Option<SyncSender<PresentationEvent>>>,
    pending_control: Arc<Mutex<PendingControl>>,
    latest_config: Arc<Mutex<Option<VrOverlayConfig>>>,
    stopping: Arc<AtomicBool>,
    worker: Mutex<Option<JoinHandle<()>>>,
    bridges: Mutex<Vec<tauri::async_runtime::JoinHandle<()>>>,
}

impl Manager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            status: Arc::new(Mutex::new(VrOverlayStatus::initial())),
            event_sender: Mutex::new(None),
            pending_control: Arc::new(Mutex::new(PendingControl::default())),
            latest_config: Arc::new(Mutex::new(None)),
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

    pub fn start(
        &self,
        mut events: broadcast::Receiver<PresentationEvent>,
        mut config: watch::Receiver<VrOverlayConfig>,
    ) -> Result<(), String> {
        tracing::info!("Starting VR Overlay manager");
        self.stop();
        let initial_config = config.borrow().clone();
        let (event_sender, event_receiver) = mpsc::sync_channel(EVENT_QUEUE_CAPACITY);
        self.stopping.store(false, Ordering::Release);

        let app = self.app.clone();
        let status = self.status.clone();
        let pending_control = self.pending_control.clone();
        let latest_config = self.latest_config.clone();
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
                    stopping,
                    initial_config,
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
                            if last_warning.map_or(true, |last: Instant| {
                                last.elapsed() >= DROP_WARNING_INTERVAL
                            }) {
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
    config: VrOverlayConfig,
    headset: HeadsetPresentation,
    wrist: WristPresentation,
    headset_sample: bool,
    wrist_sample: bool,
    headset_hash: Option<u64>,
    wrist_hash: Option<u64>,
    backend: Option<OpenVrBackend>,
    next_reconnect: Instant,
}

fn worker_loop(
    app: AppHandle,
    shared_status: Arc<Mutex<VrOverlayStatus>>,
    event_receiver: Receiver<PresentationEvent>,
    pending_control: Arc<Mutex<PendingControl>>,
    latest_config: Arc<Mutex<Option<VrOverlayConfig>>>,
    stopping: Arc<AtomicBool>,
    config: VrOverlayConfig,
) {
    let mut state = WorkerState {
        config,
        headset: HeadsetPresentation::default(),
        wrist: WristPresentation::default(),
        headset_sample: false,
        wrist_sample: false,
        headset_hash: None,
        wrist_hash: None,
        backend: None,
        next_reconnect: Instant::now(),
    };
    let mut status = VrOverlayStatus::initial();

    loop {
        let started = Instant::now();
        if stopping.load(Ordering::Acquire) {
            status.state = RuntimeState::ShuttingDown;
            update_status(&app, &shared_status, &status);
            if let Some(mut backend) = state.backend.take() {
                backend.hide_all();
            }
            break;
        }

        if let Ok(mut pending) = latest_config.lock() {
            if let Some(config) = pending.take() {
                state.wrist.set_max_entries(config.wrist.max_entries);
                if !config.enabled || !config.headset.enabled {
                    state.headset_sample = false;
                }
                if !config.enabled || !config.wrist.enabled {
                    state.wrist_sample = false;
                }
                state.config = config;
                status.last_error_detail = None;
            }
        }
        if let Ok(mut pending) = pending_control.lock() {
            let control = std::mem::take(&mut *pending);
            if control.retry {
                if let Some(mut backend) = state.backend.take() {
                    backend.hide_all();
                }
                state.headset_hash = None;
                state.wrist_hash = None;
                state.next_reconnect = Instant::now();
                status.reconnect_attempt = 0;
                status.last_error_detail = None;
            }
            if let Some(visible) = control.headset_sample {
                state.headset_sample = visible;
                state.headset_hash = None;
            }
            if let Some(visible) = control.wrist_sample {
                state.wrist_sample = visible;
                state.wrist_hash = None;
            }
        }
        for event in event_receiver.try_iter().take(EVENT_QUEUE_CAPACITY) {
            let now = Instant::now();
            state
                .headset
                .apply(event.clone(), now, &state.config.headset);
            state.wrist.apply(event, now, &state.config.wrist);
        }

        tick(&mut state, &mut status);
        update_status(&app, &shared_status, &status);
        if let Some(delay) = UPDATE_INTERVAL.checked_sub(started.elapsed()) {
            std::thread::sleep(delay);
        }
    }
}

fn tick(state: &mut WorkerState, status: &mut VrOverlayStatus) {
    status.runtime_installed = OpenVrBackend::runtime_installed();
    status.hmd_present = OpenVrBackend::hmd_present();
    status.headset.sample_visible = state.headset_sample;
    status.wrist.sample_visible = state.wrist_sample;

    if !cfg!(windows) {
        status.state = RuntimeState::Unsupported;
        return;
    }
    let any_enabled = state.config.headset.enabled || state.config.wrist.enabled;
    if !state.config.enabled || !any_enabled {
        if let Some(mut backend) = state.backend.take() {
            backend.hide_all();
        }
        state.headset_hash = None;
        state.wrist_hash = None;
        status.state = RuntimeState::Disabled;
        disable_resources(status);
        return;
    }
    if !status.runtime_installed || !status.hmd_present {
        if let Some(mut backend) = state.backend.take() {
            backend.hide_all();
        }
        state.headset_hash = None;
        state.wrist_hash = None;
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
        return;
    }

    if state.backend.is_none() && Instant::now() >= state.next_reconnect {
        status.state = if status.reconnect_attempt == 0 {
            RuntimeState::Initializing
        } else {
            RuntimeState::Reconnecting
        };
        match OpenVrBackend::connect() {
            Ok(backend) => {
                state.backend = Some(backend);
                status.state = RuntimeState::Ready;
                status.last_connected_at = Some(unix_timestamp());
                status.reconnect_attempt = 0;
                status.last_error_detail = None;
                state.headset_hash = None;
                state.wrist_hash = None;
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
        return;
    };
    status.state = RuntimeState::Ready;
    status.last_error_detail = None;
    update_headset(state, status, &mut backend);
    update_wrist(state, status, &mut backend);
    let headset_failed =
        state.config.headset.enabled && status.headset.state == ResourceState::Error;
    let wrist_failed = state.config.wrist.enabled && status.wrist.state == ResourceState::Error;
    if any_enabled
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

fn update_headset(
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut OpenVrBackend,
) {
    if !state.config.headset.enabled {
        backend.reset(OverlayKind::Headset);
        state.headset_hash = None;
        status.headset.state = ResourceState::Disabled;
        status.headset.last_error_code = None;
        return;
    }
    status.headset.state = ResourceState::Creating;
    if let Err(error) = backend.ensure_headset(&state.config.headset) {
        backend.reset(OverlayKind::Headset);
        state.headset_hash = None;
        resource_error(
            &mut status.headset,
            "headset_setup",
            error,
            &mut status.last_error_detail,
        );
        return;
    }

    let frame = if state.headset_sample {
        Some(PresentationFrame::headset(
            "VRCS Headset Overlay Sample / 视野字幕预览",
            1.0,
        ))
    } else {
        state.headset.frame(Instant::now(), &state.config.headset)
    };
    let Some(frame) = frame else {
        backend.hide(OverlayKind::Headset);
        status.headset.state = ResourceState::ReadyHidden;
        status.headset.last_error_code = None;
        return;
    };

    match render_and_show(
        backend,
        OverlayKind::Headset,
        Layout::Headset,
        &frame,
        state.config.headset.font_size_px,
        state.config.headset.background_opacity,
        state.config.headset.opacity,
        &mut state.headset_hash,
    ) {
        Ok(()) => {
            status.headset.state = if frame.opacity < 1.0 {
                ResourceState::Fading
            } else {
                ResourceState::Visible
            };
            status.headset.last_error_code = None;
        }
        Err(error) => {
            backend.reset(OverlayKind::Headset);
            state.headset_hash = None;
            resource_error(
                &mut status.headset,
                "headset_render",
                error,
                &mut status.last_error_detail,
            );
        }
    }
}

fn update_wrist(
    state: &mut WorkerState,
    status: &mut VrOverlayStatus,
    backend: &mut OpenVrBackend,
) {
    if !state.config.wrist.enabled {
        backend.reset(OverlayKind::Wrist);
        state.wrist_hash = None;
        status.wrist.state = ResourceState::Disabled;
        status.wrist.bound_role = None;
        status.wrist.tracked_device_available = false;
        status.wrist.last_error_code = None;
        return;
    }
    status.wrist.state = ResourceState::Creating;
    let binding = match backend.ensure_wrist(&state.config.wrist) {
        Ok(binding) => binding,
        Err(error) => {
            backend.reset(OverlayKind::Wrist);
            state.wrist_hash = None;
            wrist_error(status, "wrist_setup", error);
            return;
        }
    };
    status.wrist.bound_role = binding.role;
    status.wrist.tracked_device_available = binding.available;
    if !binding.available {
        status.wrist.state = ResourceState::DeviceUnavailable;
        status.wrist.last_error_code = None;
        return;
    }

    let frame = if state.wrist_sample {
        PresentationFrame::wrist(vec![
            WristMessage {
                text: "你好，欢迎使用 VRCS。".into(),
                side: MessageSide::Left,
            },
            WristMessage {
                text: "现在对话更容易分辨了。".into(),
                side: MessageSide::Right,
            },
            WristMessage {
                text: "对方在左侧，己方在右侧。".into(),
                side: MessageSide::Left,
            },
        ])
    } else {
        state
            .wrist
            .frame(Instant::now(), &state.config.wrist)
            .unwrap_or_else(|| PresentationFrame::wrist(Vec::new()))
    };

    match render_and_show(
        backend,
        OverlayKind::Wrist,
        Layout::Wrist,
        &frame,
        state.config.wrist.font_size_px,
        state.config.wrist.background_opacity,
        state.config.wrist.opacity,
        &mut state.wrist_hash,
    ) {
        Ok(()) => {
            status.wrist.state = ResourceState::Visible;
            status.wrist.last_error_code = None;
        }
        Err(error) => {
            backend.reset(OverlayKind::Wrist);
            state.wrist_hash = None;
            wrist_error(status, "wrist_render", error);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_and_show(
    backend: &mut OpenVrBackend,
    kind: OverlayKind,
    layout: Layout,
    frame: &PresentationFrame,
    font_size_px: u32,
    background_opacity: f32,
    configured_opacity: f32,
    last_hash: &mut Option<u64>,
) -> Result<(), String> {
    let content_hash =
        renderer::content_hash(layout, &frame.content, font_size_px, background_opacity);
    if *last_hash != Some(content_hash) {
        let texture = renderer::render(layout, &frame.content, font_size_px, background_opacity)?;
        backend.upload(kind, &texture)?;
        *last_hash = Some(content_hash);
    }
    backend.set_opacity(kind, configured_opacity * frame.opacity)?;
    backend.show(kind)
}

fn disable_resources(status: &mut VrOverlayStatus) {
    status.headset.state = ResourceState::Disabled;
    status.headset.last_error_code = None;
    status.wrist.state = ResourceState::Disabled;
    status.wrist.bound_role = None;
    status.wrist.tracked_device_available = false;
    status.wrist.last_error_code = None;
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
