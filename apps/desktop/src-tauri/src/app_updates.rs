use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_updater::{Update, UpdaterExt};

const UPDATE_ENDPOINT: &str =
    "https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json";
const UPDATE_TIMEOUT: Duration = Duration::from_secs(30);
const UPDATER_PUBLIC_KEY: Option<&str> = option_env!("TAURI_UPDATER_PUBLIC_KEY");

pub(crate) struct UpdateState {
    pending: Mutex<Option<Update>>,
    busy: AtomicBool,
}

impl UpdateState {
    pub(crate) fn new() -> Self {
        Self {
            pending: Mutex::new(None),
            busy: AtomicBool::new(false),
        }
    }
}

struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BuildInfo {
    version: &'static str,
    variant: &'static str,
    updater_available: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateMetadata {
    version: String,
    current_version: String,
    notes: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(
    tag = "event",
    content = "data",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum DownloadEvent {
    Started { content_length: Option<u64> },
    Progress { chunk_length: usize },
    Finished,
}

#[derive(Debug)]
pub(crate) enum UpdateError {
    Unavailable,
    Busy,
    NoPendingUpdate,
    Failed,
}

impl Serialize for UpdateError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let code = match self {
            Self::Unavailable => "update.unavailable",
            Self::Busy => "update.busy",
            Self::NoPendingUpdate => "update.no_pending",
            Self::Failed => "update.failed",
        };
        serializer.serialize_str(code)
    }
}

fn variant() -> &'static str {
    if cfg!(feature = "cuda") {
        "cuda"
    } else {
        "standard"
    }
}

fn target() -> String {
    format!("windows-x86_64-{}", variant())
}

fn acquire_busy(state: &UpdateState) -> Result<BusyGuard<'_>, UpdateError> {
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| UpdateError::Busy)?;
    Ok(BusyGuard(&state.busy))
}

pub(crate) fn register_plugin(app: &tauri::App) -> tauri::Result<()> {
    let Some(public_key) = UPDATER_PUBLIC_KEY.filter(|key| !key.trim().is_empty()) else {
        tracing::info!("application updater is disabled because no public key was configured");
        return Ok(());
    };
    app.handle().plugin(
        tauri_plugin_updater::Builder::new()
            .pubkey(public_key)
            .target(target())
            .build(),
    )
}

#[tauri::command]
pub(crate) fn app_build_info() -> BuildInfo {
    BuildInfo {
        version: env!("CARGO_PKG_VERSION"),
        variant: variant(),
        updater_available: UPDATER_PUBLIC_KEY.is_some_and(|key| !key.trim().is_empty()),
    }
}

#[tauri::command]
pub(crate) async fn check_for_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<UpdateMetadata>, UpdateError> {
    let public_key = UPDATER_PUBLIC_KEY
        .filter(|key| !key.trim().is_empty())
        .ok_or(UpdateError::Unavailable)?;
    let _busy = acquire_busy(&state)?;
    let endpoint = UPDATE_ENDPOINT.parse().map_err(|error| {
        tracing::error!(%error, "invalid updater endpoint");
        UpdateError::Failed
    })?;
    let before_exit_app = app.clone();
    let update = app
        .updater_builder()
        .pubkey(public_key)
        .target(target())
        .endpoints(vec![endpoint])
        .map_err(|error| {
            tracing::warn!(%error, "failed to configure application updater");
            UpdateError::Failed
        })?
        .timeout(UPDATE_TIMEOUT)
        .on_before_exit(move || crate::prepare_for_exit(&before_exit_app))
        .build()
        .map_err(|error| {
            tracing::warn!(%error, "failed to initialize application updater");
            UpdateError::Failed
        })?
        .check()
        .await
        .map_err(|error| {
            tracing::warn!(%error, "application update check failed");
            UpdateError::Failed
        })?;

    let metadata = update.as_ref().map(|update| UpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
    });
    *state.pending.lock().map_err(|_| UpdateError::Failed)? = update;
    Ok(metadata)
}

#[tauri::command]
pub(crate) async fn download_and_install_update(
    state: State<'_, UpdateState>,
    on_event: Channel<DownloadEvent>,
) -> Result<(), UpdateError> {
    let _busy = acquire_busy(&state)?;
    let update = state
        .pending
        .lock()
        .map_err(|_| UpdateError::Failed)?
        .take()
        .ok_or(UpdateError::NoPendingUpdate)?;
    let started = AtomicBool::new(false);

    update
        .download_and_install(
            |chunk_length, content_length| {
                if !started.swap(true, Ordering::AcqRel) {
                    let _ = on_event.send(DownloadEvent::Started { content_length });
                }
                let _ = on_event.send(DownloadEvent::Progress { chunk_length });
            },
            || {
                let _ = on_event.send(DownloadEvent::Finished);
            },
        )
        .await
        .map_err(|error| {
            tracing::error!(%error, "application update installation failed");
            UpdateError::Failed
        })
}

#[cfg(test)]
mod tests {
    use super::{target, variant, DownloadEvent};
    use serde_json::json;

    #[test]
    fn download_events_match_the_frontend_contract() {
        assert_eq!(
            serde_json::to_value(DownloadEvent::Started {
                content_length: Some(512),
            })
            .unwrap(),
            json!({ "event": "started", "data": { "contentLength": 512 } })
        );
        assert_eq!(
            serde_json::to_value(DownloadEvent::Progress { chunk_length: 128 }).unwrap(),
            json!({ "event": "progress", "data": { "chunkLength": 128 } })
        );
    }

    #[test]
    fn updater_target_matches_build_variant() {
        let expected_variant = if cfg!(feature = "cuda") {
            "cuda"
        } else {
            "standard"
        };
        assert_eq!(variant(), expected_variant);
        assert_eq!(target(), format!("windows-x86_64-{expected_variant}"));
    }
}
