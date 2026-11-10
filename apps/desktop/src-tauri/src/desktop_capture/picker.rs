use super::{image::{Image, encode_png}, selection::Region};
use std::sync::{Mutex, Arc, atomic::{AtomicU64, AtomicBool, Ordering}};
use tauri::{AppHandle, Manager as _, WebviewWindow, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

struct Pending { id: u64, image: Image, sender: oneshot::Sender<Option<Image>> }
#[derive(Default)]
pub struct Manager { pending: Mutex<Option<Pending>>, sequence: AtomicU64 }

impl Manager {
    fn cancel(&self, id: u64) {
        let mut pending = self.pending.lock().unwrap();
        if pending.as_ref().is_some_and(|pending| pending.id == id) { pending.take(); }
    }
}

struct PickerGuard { app: AppHandle, id: u64 }
impl Drop for PickerGuard {
    fn drop(&mut self) {
        self.app.state::<Manager>().cancel(self.id);
        // Never close a newer picker from an aborted older task.
        if self.app.state::<Manager>().sequence.load(Ordering::Acquire) == self.id {
            if let Some(window) = self.app.get_webview_window("capture-image") { let _ = window.close(); }
        }
    }
}

pub async fn capture_region(app: AppHandle, cancelled: Arc<AtomicBool>, on_frozen: impl FnOnce()) -> Result<Option<Image>, String> {
    let _permit = super::reserve_picker().map_err(|_| "capture.selection_busy")?;
    let Some(image) = super::portal::freeze(cancelled.clone()).await? else { return Ok(None); };
    if cancelled.load(Ordering::Acquire) { return Ok(None); }
    on_frozen();
    let manager = app.state::<Manager>();
    let id = manager.sequence.fetch_add(1, Ordering::AcqRel) + 1;
    let (sender, mut receiver) = oneshot::channel();
    *manager.pending.lock().unwrap() = Some(Pending { id, image, sender });
    let _guard = PickerGuard { app: app.clone(), id };
    let create_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if !create_app.state::<Manager>().pending.lock().unwrap().as_ref().is_some_and(|pending| pending.id == id) { return Ok(()); }
        let window = WebviewWindowBuilder::new(&create_app, "capture-image", WebviewUrl::App(format!("index.html?window=capture-image&id={id}").into()))
            .title("VRCS — Select image region").inner_size(1000.0, 740.0).min_inner_size(400.0, 320.0).build()
            .map_err(|e| e.to_string())?;
        let app = create_app.clone();
        window.on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::CloseRequested { .. } | tauri::WindowEvent::Destroyed) { app.state::<Manager>().cancel(id); }
        });
        if !create_app.state::<Manager>().pending.lock().unwrap().as_ref().is_some_and(|pending| pending.id == id) { let _ = window.close(); return Ok(()); }
        Ok::<_, String>(())
    }).await.map_err(|e| e.to_string())??;
    tokio::select! {
        result = &mut receiver => Ok(result.unwrap_or(None)),
        _ = async { while !cancelled.load(Ordering::Acquire) { tokio::time::sleep(std::time::Duration::from_millis(40)).await; } } => Ok(None),
    }
}

fn check_window(window: &WebviewWindow) -> Result<(), String> {
    if window.label() == "capture-image" { Ok(()) } else { Err("Unsupported capture window".into()) }
}

#[tauri::command]
pub fn desktop_capture_frame(window: WebviewWindow, manager: tauri::State<'_, Manager>, id: u64) -> Result<tauri::ipc::Response, String> {
    check_window(&window)?;
    let pending = manager.pending.lock().unwrap();
    let pending = pending.as_ref().filter(|p| p.id == id).ok_or("capture.expired")?;
    Ok(tauri::ipc::Response::new(encode_png(&pending.image)?))
}

#[tauri::command]
pub fn desktop_capture_select(window: WebviewWindow, manager: tauri::State<'_, Manager>, id: u64, left: u32, top: u32, width: u32, height: u32) -> Result<(), String> {
    check_window(&window)?;
    let mut pending = manager.pending.lock().unwrap();
    let selected = pending.as_ref().filter(|p| p.id == id).ok_or("capture.expired")?
        .image.crop(Region { left, top, width, height })?;
    let request = pending.take().ok_or("capture.expired")?;
    // The full frame drops here; only the crop crosses into OCR/vision.
    let _ = request.sender.send(Some(selected));
    Ok(())
}

#[tauri::command]
pub fn desktop_capture_cancel(window: WebviewWindow, manager: tauri::State<'_, Manager>, id: u64) -> Result<(), String> {
    check_window(&window)?;
    manager.cancel(id);
    Ok(())
}
