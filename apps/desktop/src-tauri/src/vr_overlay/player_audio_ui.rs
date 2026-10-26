use serde::{Deserialize, Serialize};

use super::ocr_progress::ProgressView;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prompt {
    pub request_id: u32,
    pub title: String,
    pub intro: String,
    pub sections: Vec<Section>,
    pub remaining_seconds: u32,
    pub reading_status: String,
    pub confirm: String,
    pub cancel: String,
    pub previous: String,
    pub next: String,
    pub scroll_hint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub title: String,
    pub paragraphs: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    pub notice: Option<Prompt>,
    pub progress: Option<ProgressView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Decision {
    pub owner_epoch: u32,
    pub request_id: u32,
    pub accepted: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Cancel,
    Confirm,
    Previous,
    Next,
}

#[cfg(windows)]
pub fn activate(
    app: &tauri::AppHandle,
    controls: &std::sync::Mutex<super::control_protocol::ControlHub>,
    surface: super::surface::SurfaceId,
    request_id: u32,
    action: Action,
) {
    use tauri::{Emitter, Manager};
    let decision = {
        let Ok(hub) = controls.lock() else { return };
        if !hub.active()
            || hub.player_audio().notice.as_ref().map(|p| p.request_id) != Some(request_id)
        {
            return;
        }
        match action {
            Action::Previous | Action::Next => {
                let delta = if action == Action::Previous {
                    -0.85
                } else {
                    0.85
                };
                if let Err(error) = app
                    .state::<super::surface::SurfaceHub>()
                    .scroll(app, surface, delta)
                {
                    tracing::debug!(%error, "VR consent scroll unavailable");
                }
                return;
            }
            Action::Cancel | Action::Confirm => {
                hub.player_audio_decision(request_id, action == Action::Confirm)
            }
        }
    };
    if let Some(decision) = decision {
        if let Err(error) = app.emit_to("main", "vr-overlay-player-audio-decision", decision) {
            tracing::warn!(%error, "VR consent decision unavailable");
        }
    }
}

#[cfg(windows)]
pub use native::Runtime;

#[cfg(windows)]
mod native;
