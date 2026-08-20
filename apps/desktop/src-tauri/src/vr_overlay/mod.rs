mod backend;
#[cfg(windows)]
mod d3d11_texture;
mod presentation;
mod process;
mod renderer;
mod runtime;
mod transform;
#[cfg(windows)]
mod wrist_renderer;

use tauri::State;

pub use runtime::Manager;
use runtime::{SampleKind, VrOverlayStatus};

#[tauri::command]
pub fn vr_overlay_status(manager: State<'_, Manager>) -> Result<VrOverlayStatus, String> {
    manager.status()
}

#[tauri::command]
pub fn vr_overlay_retry(manager: State<'_, Manager>) -> Result<(), String> {
    manager.retry()
}

#[tauri::command]
pub fn vr_overlay_show_sample(kind: String, manager: State<'_, Manager>) -> Result<(), String> {
    manager.set_sample(SampleKind::parse(&kind)?, true)
}

#[tauri::command]
pub fn vr_overlay_hide_sample(kind: String, manager: State<'_, Manager>) -> Result<(), String> {
    manager.set_sample(SampleKind::parse(&kind)?, false)
}
