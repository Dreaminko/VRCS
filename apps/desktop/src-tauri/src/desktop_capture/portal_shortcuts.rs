//! Global shortcuts are opt-in and independent from the capture button.
use ashpd::desktop::{
    global_shortcuts::{GlobalShortcuts, NewShortcut},
    Session,
};
use futures_util::StreamExt;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tauri::{AppHandle, Manager as _, WebviewWindow};

#[derive(Default)]
pub struct Manager {
    task: Mutex<Option<Registration>>,
}
impl Manager {
    pub fn bindings(&self, app: &AppHandle) -> Option<Vec<String>> {
        let mut task = self.task.lock().unwrap();
        let registration = task.as_ref()?;
        if registration.stop.load(Ordering::Acquire)
            || registration.config != configured_bindings(app)
        {
            task.take();
            return None;
        }
        Some(registration.bindings.clone())
    }
}
struct Registration {
    stop: Arc<AtomicBool>,
    task: tauri::async_runtime::JoinHandle<()>,
    session: SessionGuard,
    config: (Option<String>, Option<String>),
    bindings: Vec<String>,
}
impl Registration {
    async fn close(mut self) {
        self.stop.store(true, Ordering::Release);
        self.task.abort();
        if let Some(session) = self.session.0.take() {
            let _ = session.close().await;
        }
    }
}
impl Drop for Registration {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
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
    let Ok(proxy) = GlobalShortcuts::new().await else {
        return false;
    };
    proxy
        .get_property::<u32>("version")
        .await
        .is_ok_and(|version| version >= 1)
}

fn trigger(shortcut: &str) -> Result<String, String> {
    Ok(super::shortcut::Shortcut::parse(shortcut)?.portal_trigger())
}

fn configured_bindings(app: &AppHandle) -> (Option<String>, Option<String>) {
    (
        app.state::<crate::desktop_ocr::Manager>().shortcut_binding(),
        app.state::<crate::desktop_vision::Manager>().shortcut_binding(),
    )
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
    let previous = app.state::<Manager>().task.lock().unwrap().take();
    if let Some(previous) = previous {
        previous.close().await;
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
    let session = guard.0.as_ref().unwrap();
    let mut activated = proxy.receive_activated().await.map_err(|e| e.to_string())?;
    let session_path = serde_json::to_value(session)
        .map_err(|e| e.to_string())?
        .as_str()
        .ok_or("capture.invalid_session")?
        .to_owned();
    let mut closed = session.receive_closed().await.map_err(|e| e.to_string())?;
    let response = proxy
        .bind_shortcuts(session, &shortcuts, None)
        .await
        .map_err(|e| e.to_string())?
        .response()
        .map_err(|e| e.to_string())?;
    let bindings = response
        .shortcuts()
        .iter()
        .map(|shortcut| shortcut.trigger_description().to_owned())
        .collect::<Vec<_>>();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let event_config = config.clone();
    let event_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let _proxy = proxy;
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
                _ = closed.next() => break,
                _ = async { while !stopping.load(Ordering::Acquire) { tokio::time::sleep(std::time::Duration::from_millis(100)).await; } } => break,
            }
        }
        stopping.store(true, Ordering::Release);
    });
    if configured_bindings(&app) != config {
        task.abort();
        return Err("capture.shortcuts_changed".into());
    }
    *app.state::<Manager>().task.lock().unwrap() = Some(Registration {
        stop, task, session: guard, config, bindings,
    });
    Ok(())
}
