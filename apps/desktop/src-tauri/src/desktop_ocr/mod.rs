#[cfg(windows)]
mod hotkey;
mod runtime;
#[cfg(windows)]
mod selection;
#[cfg(windows)]
mod selection_window;
mod shortcut;
mod status;

pub use runtime::Manager;
use status::DesktopOcrStatus;
use tauri::{State, WebviewWindow};

fn check_window(window: &WebviewWindow) -> Result<(), String> {
    if matches!(window.label(), "main" | "ocr") {
        Ok(())
    } else {
        Err("Unsupported OCR window".into())
    }
}

#[tauri::command]
pub fn desktop_ocr_status(
    window: WebviewWindow,
    manager: State<'_, Manager>,
) -> Result<DesktopOcrStatus, String> {
    check_window(&window)?;
    Ok(manager.status())
}

#[tauri::command]
pub fn desktop_ocr_scan(window: WebviewWindow, manager: State<'_, Manager>) -> Result<(), String> {
    check_window(&window)?;
    manager.scan()
}

#[tauri::command]
pub fn desktop_ocr_close(window: WebviewWindow, manager: State<'_, Manager>) -> Result<(), String> {
    check_window(&window)?;
    manager.close();
    Ok(())
}
