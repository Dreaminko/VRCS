//! Global shortcuts are opt-in and independent from the capture button.
use ashpd::desktop::{
    global_shortcuts::{GlobalShortcuts, NewShortcut, Shortcut},
    Session,
};
use futures_util::StreamExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Emitter, Manager as _, WebviewWindow};
use tokio::sync::Notify;

const SHORTCUTS_CHANGED_EVENT: &str = "desktop-shortcuts-changed";

#[derive(Default)]
pub struct Manager {
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    task: Option<Registration>,
    restore_attempted: Option<(Option<String>, Option<String>)>,
    restoring: bool,
    x11: Option<super::x11_shortcuts::X11Session>,
    x11_bindings: Vec<String>,
}

impl Manager {
    pub fn bindings(&self, app: &AppHandle) -> Option<Vec<String>> {
        let mut state = self.state.lock().unwrap();
        if crate::platform_capabilities::is_native_x11() {
            return (!state.x11_bindings.is_empty()).then(|| state.x11_bindings.clone());
        }
        let registration = state.task.as_ref()?;
        if registration.stop.load(Ordering::Acquire)
            || registration.config != configured_bindings(app)
        {
            state.task.take();
            return None;
        }
        let bindings = registration.bindings.lock().unwrap().clone();
        (!bindings.is_empty()).then_some(bindings)
    }
}

struct Registration {
    stop: Arc<AtomicBool>,
    stop_notify: Arc<Notify>,
    task: tauri::async_runtime::JoinHandle<()>,
    session: SessionGuard,
    config: (Option<String>, Option<String>),
    bindings: Arc<Mutex<Vec<String>>>,
}

impl Registration {
    async fn close(mut self) {
        self.stop.store(true, Ordering::Release);
        self.stop_notify.notify_waiters();
        self.task.abort();
        if let Some(session) = self.session.0.take() {
            let _ = session.close().await;
        }
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        self.stop_notify.notify_waiters();
        self.task.abort();
    }
}

struct SessionGuard(Option<Session<'static, GlobalShortcuts<'static>>>);
impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() {
            tauri::async_runtime::spawn(async move {
                let _ = session.close().await;
            });
        }
    }
}

pub async fn available() -> bool {
    if crate::platform_capabilities::is_native_x11() {
        return tauri::async_runtime::spawn_blocking(super::x11_shortcuts::available)
            .await
            .unwrap_or(false);
    }
    let check = async {
        let proxy = GlobalShortcuts::new().await.ok()?;
        proxy.get_property::<u32>("version").await.ok()
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), check)
        .await
        .ok()
        .flatten()
        .is_some_and(|version| version >= 1)
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    let manager = app.state::<Manager>();
    let state = manager.state.lock().unwrap();
    if let Some(session) = state.x11.as_ref() {
        session.set_paused(paused);
    }
}

fn sync_x11(
    app: &AppHandle,
    ocr: Option<String>,
    vision: Option<String>,
) -> Result<Vec<String>, String> {
    let manager = app.state::<Manager>();
    let mut state = manager.state.lock().unwrap();
    if ocr.is_none() && vision.is_none() {
        state.x11.take();
        state.x11_bindings.clear();
        drop(state);
        let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
        return Ok(Vec::new());
    }
    if state.x11.is_none() {
        match super::x11_shortcuts::X11Session::start(app.clone()) {
            Ok(session) => state.x11 = Some(session),
            Err(error) => {
                state.x11_bindings.clear();
                drop(state);
                let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
                return Err(error);
            }
        }
    }
    let result = state.x11.as_ref().unwrap().update(ocr, vision);
    match result {
        Ok(bindings) => {
            state.x11_bindings = bindings.clone();
            drop(state);
            let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
            Ok(bindings)
        }
        Err(error) => {
            if error == "capture.shortcuts_unavailable" {
                state.x11.take();
            }
            state.x11_bindings.clear();
            drop(state);
            let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
            Err(error)
        }
    }
}

fn trigger(shortcut: &str) -> Result<String, String> {
    Ok(super::shortcut::Shortcut::parse(shortcut)?.portal_trigger())
}

fn configured_bindings(app: &AppHandle) -> (Option<String>, Option<String>) {
    (
        app.state::<crate::desktop_ocr::Manager>()
            .shortcut_binding(),
        app.state::<crate::desktop_vision::Manager>()
            .shortcut_binding(),
    )
}

fn extract_bindings(shortcuts: &[Shortcut]) -> Vec<String> {
    shortcuts
        .iter()
        .map(|shortcut| shortcut.trigger_description().to_owned())
        .filter(|trigger| !trigger.is_empty())
        .collect()
}

async fn start_registration(
    app: &AppHandle,
    proxy: GlobalShortcuts<'static>,
    guard: SessionGuard,
    config: (Option<String>, Option<String>),
    initial_shortcuts: &[Shortcut],
) -> Result<Registration, String> {
    let session = guard.0.as_ref().ok_or("capture.invalid_session")?;
    let mut activated = proxy.receive_activated().await.map_err(|e| e.to_string())?;
    let mut shortcuts_changed = proxy
        .receive_shortcuts_changed()
        .await
        .map_err(|e| e.to_string())?;
    let session_path = serde_json::to_value(session)
        .map_err(|e| e.to_string())?
        .as_str()
        .ok_or("capture.invalid_session")?
        .to_owned();
    let mut closed = session.receive_closed().await.map_err(|e| e.to_string())?;
    let bindings = Arc::new(Mutex::new(extract_bindings(initial_shortcuts)));
    let task_bindings = Arc::clone(&bindings);
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = Arc::clone(&stop);
    let stop_notify = Arc::new(Notify::new());
    let stop_wait = Arc::clone(&stop_notify);
    let event_config = config.clone();
    let event_app = app.clone();

    let task = tauri::async_runtime::spawn(async move {
        let _proxy = proxy;
        let wait_stop = async {
            loop {
                let notified = stop_wait.notified();
                if stopping.load(Ordering::Acquire) {
                    break;
                }
                notified.await;
            }
        };
        tokio::pin!(wait_stop);
        loop {
            tokio::select! {
                event = activated.next() => {
                    let Some(event) = event else { break; };
                    if event.session_handle().as_str() != session_path { continue; }
                    if event_app.state::<super::ShortcutRecording>().is_active() { continue; }
                    if configured_bindings(&event_app) != event_config { break; }
                    match event.shortcut_id() {
                        "ocr" => { let _ = event_app.state::<crate::desktop_ocr::Manager>().scan(); }
                        "vision" => { let _ = event_app.state::<crate::desktop_vision::Manager>().capture(); }
                        _ => {}
                    }
                }
                changed = shortcuts_changed.next() => {
                    let Some(changed) = changed else { break; };
                    if changed.session_handle().as_str() != session_path { continue; }
                    *task_bindings.lock().unwrap() = extract_bindings(changed.shortcuts());
                    let _ = event_app.emit(SHORTCUTS_CHANGED_EVENT, ());
                }
                _ = closed.next() => break,
                _ = &mut wait_stop => break,
            }
        }
        stopping.store(true, Ordering::Release);
        let _ = event_app.emit(SHORTCUTS_CHANGED_EVENT, ());
    });

    Ok(Registration {
        stop,
        stop_notify,
        task,
        session: guard,
        config,
        bindings,
    })
}

async fn install_registration(app: &AppHandle, registration: Registration) {
    let config = registration.config.clone();
    let previous = {
        let manager = app.state::<Manager>();
        let mut state = manager.state.lock().unwrap();
        state.restore_attempted = Some(config);
        state.task.replace(registration)
    };
    if let Some(previous) = previous {
        previous.close().await;
    }
    let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
}

pub fn sync(app: AppHandle) {
    let config = configured_bindings(&app);
    if crate::platform_capabilities::is_native_x11() {
        let (ocr, mut vision) = config;
        if let (Some(ocr_sc), Some(vision_sc)) = (ocr.as_deref(), vision.as_deref()) {
            if super::shortcut::Shortcut::parse(ocr_sc)
                == super::shortcut::Shortcut::parse(vision_sc)
            {
                vision = None;
            }
        }
        let _ = sync_x11(&app, ocr, vision);
        return;
    }
    if config.0.is_none() && config.1.is_none() {
        let previous = {
            let manager = app.state::<Manager>();
            let mut state = manager.state.lock().unwrap();
            state.restore_attempted = None;
            state.task.take()
        };
        if let Some(previous) = previous {
            tauri::async_runtime::spawn(async move {
                previous.close().await;
                let _ = app.emit(SHORTCUTS_CHANGED_EVENT, ());
            });
        }
        return;
    }
    {
        let manager = app.state::<Manager>();
        let mut state = manager.state.lock().unwrap();
        if let Some(existing) = state.task.as_ref() {
            if !existing.stop.load(Ordering::Acquire) && existing.config == config {
                return;
            }
        }
        if state.restoring || state.restore_attempted.as_ref() == Some(&config) {
            return;
        }
        state.restoring = true;
        state.restore_attempted = Some(config.clone());
    }

    tauri::async_runtime::spawn(async move {
        let _guard = std::mem::DropGuard::new(&app, |app: &AppHandle| {
            app.state::<Manager>().state.lock().unwrap().restoring = false;
        });

        let Ok(proxy) = GlobalShortcuts::new().await else {
            return;
        };
        if !proxy
            .get_property::<u32>("version")
            .await
            .is_ok_and(|version| version >= 1)
        {
            return;
        }
        let Ok(session) = proxy.create_session().await else {
            return;
        };
        let guard = SessionGuard(Some(session));
        let Ok(request) = proxy.list_shortcuts(guard.0.as_ref().unwrap()).await else {
            return;
        };
        let Ok(response) = request.response() else {
            return;
        };
        let shortcuts = response.shortcuts();
        let has_id = |id: &str| {
            shortcuts
                .iter()
                .any(|s| s.id() == id && !s.trigger_description().is_empty())
        };
        if config.0.is_some() && !has_id("ocr") {
            return;
        }
        if config.1.is_some() && !has_id("vision") {
            return;
        }
        if configured_bindings(&app) != config {
            return;
        }
        if let Ok(registration) = start_registration(&app, proxy, guard, config, shortcuts).await {
            install_registration(&app, registration).await;
        }
    });
}

#[tauri::command]
pub async fn desktop_capture_register_shortcuts(
    window: WebviewWindow,
    app: AppHandle,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Unsupported shortcuts window".into());
    }
    let config = configured_bindings(&app);
    if crate::platform_capabilities::is_native_x11() {
        let (ocr, vision) = config;
        if ocr.is_none() && vision.is_none() {
            let _ = sync_x11(&app, None, None);
            return Err("capture.shortcuts_disabled".into());
        }
        if let (Some(ocr_sc), Some(vision_sc)) = (ocr.as_deref(), vision.as_deref()) {
            if super::shortcut::Shortcut::parse(ocr_sc)
                == super::shortcut::Shortcut::parse(vision_sc)
            {
                return Err("desktop_vision.shortcut_conflict".into());
            }
        }
        sync_x11(&app, ocr, vision)?;
        return Ok(());
    }
    let (ocr, vision) = &config;
    let mut shortcuts = Vec::new();
    if let Some(shortcut) = ocr.as_deref() {
        shortcuts.push(
            NewShortcut::new("ocr", "VRCS desktop OCR")
                .preferred_trigger(Some(trigger(shortcut)?.as_str())),
        );
    }
    if let Some(shortcut) = vision.as_deref() {
        if ocr.as_ref().is_some_and(|ocr| {
            super::shortcut::Shortcut::parse(ocr) == super::shortcut::Shortcut::parse(shortcut)
        }) {
            return Err("desktop_vision.shortcut_conflict".into());
        }
        shortcuts.push(
            NewShortcut::new("vision", "VRCS visual question answering")
                .preferred_trigger(Some(trigger(shortcut)?.as_str())),
        );
    }
    if shortcuts.is_empty() {
        return Err("capture.shortcuts_disabled".into());
    }
    let proxy = GlobalShortcuts::new()
        .await
        .map_err(|_| "capture.shortcuts_unavailable")?;
    if proxy
        .get_property::<u32>("version")
        .await
        .map_err(|_| "capture.shortcuts_unavailable")?
        < 1
    {
        return Err("capture.shortcuts_unavailable".into());
    }
    let session = proxy
        .create_session()
        .await
        .map_err(|_| "capture.shortcuts_unavailable")?;
    let guard = SessionGuard(Some(session));
    let response = proxy
        .bind_shortcuts(guard.0.as_ref().unwrap(), &shortcuts, None)
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map_err(|e| e.to_string())?;
    let registration =
        start_registration(&app, proxy, guard, config.clone(), response.shortcuts()).await?;
    if configured_bindings(&app) != config {
        registration.close().await;
        return Err("capture.shortcuts_changed".into());
    }
    install_registration(&app, registration).await;
    Ok(())
}
