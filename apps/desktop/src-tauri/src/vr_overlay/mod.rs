mod backend;
mod caption_status;
mod control_protocol;
#[cfg(windows)]
mod cursor;
#[cfg(windows)]
mod d3d11_texture;
mod dashboard;
mod dialogue;
mod interaction;
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
mod ocr_runtime;
#[cfg(windows)]
mod ocr_selection;
mod ocr_status;
#[cfg(windows)]
mod ocr_wrist;
mod placement;
#[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
pub(crate) mod preview;
#[cfg(windows)]
pub(crate) mod surface;
#[cfg(windows)]
mod ui_runtime;
#[cfg(windows)]
mod vision_runtime;
mod wrist_menu;
// Keep the image verifier covered without enabling scan cancellation.
#[cfg(all(windows, test))]
mod ocr_tracking;
mod presentation;
mod process;
pub(crate) mod renderer;
mod runtime;
mod transform;
mod wrist_labels;

use tauri::State;

use control_protocol::{ControlRequest, ControlSnapshot};
use dashboard::DashboardViewModel;
pub use runtime::Manager;
use runtime::{SampleKind, VrOverlayStatus};

fn require_control_owner(window: &tauri::WebviewWindow) -> Result<(), String> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("Control owner must be the main window".into())
    }
}

#[tauri::command]
pub fn vr_overlay_control_ready(
    window: tauri::WebviewWindow,
    view: ControlSnapshot,
    manager: State<'_, Manager>,
) -> Result<ControlSnapshot, String> {
    require_control_owner(&window)?;
    manager
        .placement
        .lock()
        .map_err(|e| e.to_string())?
        .cancel();
    manager
        .controls
        .lock()
        .map_err(|e| e.to_string())?
        .register(view)
}

#[tauri::command]
pub fn vr_overlay_control_update(
    window: tauri::WebviewWindow,
    view: ControlSnapshot,
    manager: State<'_, Manager>,
) -> Result<ControlSnapshot, String> {
    require_control_owner(&window)?;
    manager
        .controls
        .lock()
        .map_err(|e| e.to_string())?
        .update(view.owner_epoch, view)
}

#[tauri::command]
pub fn vr_overlay_control_result(
    window: tauri::WebviewWindow,
    request: ControlRequest,
    error: Option<String>,
    manager: State<'_, Manager>,
) -> Result<bool, String> {
    require_control_owner(&window)?;
    Ok(manager
        .controls
        .lock()
        .map_err(|e| e.to_string())?
        .finish(&request, error))
}

#[tauri::command]
pub fn vr_overlay_control_release(
    window: tauri::WebviewWindow,
    epoch: u32,
    manager: State<'_, Manager>,
) -> Result<(), String> {
    require_control_owner(&window)?;
    let mut hub = manager.controls.lock().map_err(|e| e.to_string())?;
    if hub.snapshot().owner_epoch == epoch {
        hub.revoke();
        manager
            .placement
            .lock()
            .map_err(|e| e.to_string())?
            .cancel();
    }
    Ok(())
}

#[tauri::command]
pub fn vr_overlay_placement_result(
    window: tauri::WebviewWindow,
    epoch: u32,
    id: u64,
    error: Option<String>,
    manager: State<'_, Manager>,
) -> Result<bool, String> {
    require_control_owner(&window)?;
    let hub = manager.controls.lock().map_err(|e| e.to_string())?;
    if !hub.active() || hub.snapshot().owner_epoch != epoch {
        return Ok(false);
    }
    Ok(manager
        .placement
        .lock()
        .map_err(|e| e.to_string())?
        .resolve(id, error.is_none(), error))
}

#[tauri::command]
pub fn vr_overlay_place_dialogue(
    mode: placement::Mode,
    manager: State<'_, Manager>,
    runtime: State<'_, crate::CoreRuntime>,
) -> Result<(), String> {
    runtime.require_feature(vrcs_core::FeatureKey::VrOverlay)?;
    manager.place_dialogue(mode)
}

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
    if runtime.require_feature(vrcs_core::FeatureKey::Ocr).is_err() {
        runtime.require_feature(vrcs_core::FeatureKey::VisualQuestionAnswering)?;
    }
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
