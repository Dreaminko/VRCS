mod backend;
#[cfg(windows)]
mod d3d11_texture;
mod dashboard;
mod dashboard_renderer;
#[cfg(any(windows, test))]
mod headset_layout;
#[cfg(windows)]
pub(crate) mod ocr_capture;
#[cfg(windows)]
mod ocr_geometry;
#[cfg(windows)]
mod ocr_gesture;
#[cfg(windows)]
mod ocr_input;
#[cfg(windows)]
mod ocr_input_state;
#[cfg(windows)]
mod ocr_plane;
mod ocr_progress;
#[cfg(windows)]
mod ocr_progress_renderer;
#[cfg(windows)]
mod ocr_renderer;
#[cfg(windows)]
mod ocr_runtime;
#[cfg(windows)]
mod ocr_selection;
mod ocr_status;
#[cfg(windows)]
mod ocr_wrist;
#[cfg(windows)]
mod ocr_wrist_layout;
#[cfg(windows)]
mod ocr_wrist_renderer;
#[cfg(windows)]
mod text_raster;
// Keep the image verifier covered without enabling scan cancellation.
#[cfg(all(windows, test))]
mod ocr_tracking;
mod presentation;
mod process;
pub(crate) mod renderer;
mod runtime;
mod transform;
#[cfg(any(windows, test))]
mod wrist_layout;
#[cfg(windows)]
mod wrist_renderer;

use tauri::State;

use dashboard::DashboardViewModel;
pub use runtime::Manager;
use runtime::{SampleKind, VrOverlayStatus};

#[tauri::command]
pub fn vr_overlay_status(manager: State<'_, Manager>) -> Result<VrOverlayStatus, String> {
    manager.status()
}

#[tauri::command]
pub fn vr_overlay_retry(
    manager: State<'_, Manager>,
    runtime: State<'_, crate::CoreRuntime>,
) -> Result<(), String> {
    runtime.require_feature(vrcs_core::FeatureKey::VrOverlay)?;
    manager.retry()
}

#[tauri::command]
pub fn vr_dashboard_update_view(
    view: DashboardViewModel,
    manager: State<'_, Manager>,
) -> Result<(), String> {
    manager.update_dashboard(view)
}

#[tauri::command]
pub fn vr_ocr_open_bindings(
    manager: State<'_, Manager>,
    runtime: State<'_, crate::CoreRuntime>,
) -> Result<(), String> {
    runtime.require_feature(vrcs_core::FeatureKey::VrOverlay)?;
    runtime.require_feature(vrcs_core::FeatureKey::Ocr)?;
    manager.open_ocr_bindings()
}

#[tauri::command]
pub fn vr_overlay_show_sample(
    kind: String,
    manager: State<'_, Manager>,
    runtime: State<'_, crate::CoreRuntime>,
) -> Result<(), String> {
    runtime.require_feature(vrcs_core::FeatureKey::VrOverlay)?;
    manager.set_sample(SampleKind::parse(&kind)?, true)
}

#[tauri::command]
pub fn vr_overlay_hide_sample(kind: String, manager: State<'_, Manager>) -> Result<(), String> {
    manager.set_sample(SampleKind::parse(&kind)?, false)
}
