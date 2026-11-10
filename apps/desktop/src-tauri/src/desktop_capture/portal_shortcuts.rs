//! Global shortcuts are opt-in and independent from the capture button.
use ashpd::desktop::{global_shortcuts::{GlobalShortcuts, NewShortcut}, Session};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tauri::{AppHandle, Manager as _, WebviewWindow};

#[derive(Default)]
pub struct Manager { task: Mutex<Option<Registration>> }
struct Registration { stop: Arc<AtomicBool>, task: tauri::async_runtime::JoinHandle<()> }
impl Drop for Registration {
    fn drop(&mut self) { self.stop.store(true, Ordering::Release); self.task.abort(); }
}
struct SessionGuard(Option<Session<'static, GlobalShortcuts<'static>>>);
impl Drop for SessionGuard {
    fn drop(&mut self) {
        if let Some(session) = self.0.take() { tauri::async_runtime::spawn(async move { let _ = session.close().await; }); }
    }
}

pub async fn available() -> bool {
    let Ok(proxy) = GlobalShortcuts::new().await else { return false; };
    proxy.get_property::<u32>("version").await.is_ok_and(|version| version >= 1)
}

fn trigger(shortcut: &str) -> Result<String, String> {
    let (flags, key) = super::shortcut::parse_shortcut(shortcut)?;
    let mut trigger = String::new();
    if flags & 2 != 0 { trigger.push_str("CTRL+"); }
    if flags & 1 != 0 { trigger.push_str("ALT+"); }
    if flags & 4 != 0 { trigger.push_str("SHIFT+"); }
    if key >= 112 { trigger.push_str(&format!("F{}", key - 111)); }
    else { trigger.push(char::from_u32(key).ok_or("Invalid shortcut")?); }
    Ok(trigger)
}

#[derive(serde::Serialize)]
pub struct Binding { id: String, trigger: String }

#[tauri::command]
pub async fn desktop_capture_register_shortcuts(window: WebviewWindow, app: AppHandle) -> Result<Vec<Binding>, String> {
    if window.label() != "main" { return Err("Unsupported shortcuts window".into()); }
    let ocr = app.state::<crate::desktop_ocr::Manager>().shortcut_binding();
    let vision = app.state::<crate::desktop_vision::Manager>().shortcut_binding();
    let mut shortcuts = Vec::new();
    if let Some(shortcut) = ocr.as_deref() {
        shortcuts.push(NewShortcut::new("ocr", "VRCS desktop OCR").preferred_trigger(Some(trigger(shortcut)?.as_str())));
    }
    if let Some(shortcut) = vision.as_deref() {
        if ocr.as_ref().is_some_and(|ocr| super::shortcut::parse_shortcut(ocr) == super::shortcut::parse_shortcut(shortcut)) { return Err("desktop_vision.shortcut_conflict".into()); }
        shortcuts.push(NewShortcut::new("vision", "VRCS visual question answering").preferred_trigger(Some(trigger(shortcut)?.as_str())));
    }
    if shortcuts.is_empty() { return Err("capture.shortcuts_disabled".into()); }
    let proxy = GlobalShortcuts::new().await.map_err(|_| "capture.shortcuts_unavailable")?;
    if proxy.get_property::<u32>("version").await.map_err(|_| "capture.shortcuts_unavailable")? < 1 { return Err("capture.shortcuts_unavailable".into()); }
    let session = proxy.create_session().await.map_err(|_| "capture.shortcuts_unavailable")?;
    let guard = SessionGuard(Some(session));
    let session = guard.0.as_ref().unwrap();
    let mut activated = proxy.receive_activated().await.map_err(|e| e.to_string())?;
    let session_path = serde_json::to_value(session).map_err(|e| e.to_string())?.as_str().ok_or("capture.invalid_session")?.to_owned();
    let mut closed = session.receive_closed().await.map_err(|e| e.to_string())?;
    let response = proxy.bind_shortcuts(session, &shortcuts, None).await.map_err(|e| e.to_string())?.response().map_err(|e| e.to_string())?;
    let bindings = response.shortcuts().iter().map(|shortcut| Binding { id: shortcut.id().into(), trigger: shortcut.trigger_description().into() }).collect();
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let event_app = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        let _guard = guard;
        let _proxy = proxy;
        loop {
            tokio::select! {
                event = activated.next() => {
                    let Some(event) = event else { break; };
                    if event.session_handle().as_str() != session_path { continue; }
                    if event_app.state::<super::ShortcutRecording>().is_active() { continue; }
                    match event.shortcut_id() {
                        "ocr" if event_app.state::<crate::desktop_ocr::Manager>().shortcut_binding() == ocr => { let _ = event_app.state::<crate::desktop_ocr::Manager>().scan(); }
                        "vision" if event_app.state::<crate::desktop_vision::Manager>().shortcut_binding() == vision => { let _ = event_app.state::<crate::desktop_vision::Manager>().capture(); }
                        _ => {}
                    }
                }
                _ = closed.next() => break,
                _ = async { while !stopping.load(Ordering::Acquire) { tokio::time::sleep(std::time::Duration::from_millis(100)).await; } } => break,
            }
        }
    });
    *app.state::<Manager>().task.lock().unwrap() = Some(Registration { stop, task });
    Ok(bindings)
}
