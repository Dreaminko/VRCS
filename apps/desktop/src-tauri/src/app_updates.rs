use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use semver::Version;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};
use tauri_plugin_store::StoreExt;
use tauri_plugin_updater::{Update, UpdaterExt};

const STABLE_ENDPOINT: &str =
    "https://github.com/Dreaminko/VRCS/releases/latest/download/latest.json";
const RELEASE_PAGE_URL: &str = "https://github.com/Dreaminko/VRCS/releases/latest";
const NIGHTLY_ENDPOINT: &str =
    "https://raw.githubusercontent.com/Dreaminko/VRCS/nightly-updates/nightly.json";
const NIGHTLY_PREFERENCE: &str = "receiveNightlyUpdates";
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
    version: String,
    variant: &'static str,
    os: &'static str,
    arch: &'static str,
    package_channel: &'static str,
    updater_available: bool,
    can_install: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateMetadata {
    version: String,
    current_version: String,
    notes: Option<String>,
    channel: UpdateChannel,
    release_url: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum UpdateChannel {
    Stable,
    Nightly,
}

impl UpdateChannel {
    fn endpoint(self) -> &'static str {
        match self {
            Self::Stable => STABLE_ENDPOINT,
            Self::Nightly => NIGHTLY_ENDPOINT,
        }
    }

    fn matches(self, version: &Version) -> bool {
        match self {
            Self::Stable => version.pre.is_empty(),
            Self::Nightly => version
                .pre
                .as_str()
                .strip_prefix("nightly.")
                .is_some_and(|date| {
                    date.len() == 8 && date.bytes().all(|digit| digit.is_ascii_digit())
                }),
        }
    }
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
    CheckFailed(UpdateChannel),
    BothChecksFailed,
    PreferenceSaveFailed,
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
            Self::CheckFailed(UpdateChannel::Stable) => "update.stable_check_failed",
            Self::CheckFailed(UpdateChannel::Nightly) => "update.nightly_check_failed",
            Self::BothChecksFailed => "update.both_checks_failed",
            Self::PreferenceSaveFailed => "update.preference_save_failed",
            Self::Failed => "update.failed",
        };
        serializer.serialize_str(code)
    }
}

fn variant() -> &'static str {
    "standard"
}

fn target() -> String {
    format!(
        "{}-{}-{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        variant()
    )
}

#[cfg(target_os = "linux")]
fn appimage_writable() -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Some(appimage) = std::env::var_os("APPIMAGE").filter(|path| !path.is_empty()) else {
        return false;
    };
    let writable = |path: &std::path::Path| {
        std::ffi::CString::new(path.as_os_str().as_bytes())
            .is_ok_and(|cstr| unsafe { libc::access(cstr.as_ptr(), libc::W_OK) == 0 })
    };
    let path = std::path::Path::new(&appimage);
    path.parent().is_some_and(writable) && writable(path)
}

fn has_public_key() -> bool {
    UPDATER_PUBLIC_KEY.is_some_and(|key| !key.trim().is_empty())
}

pub(crate) fn can_install_updates() -> bool {
    #[cfg(windows)]
    let supported = true;
    #[cfg(target_os = "linux")]
    let supported = package_channel() == "appimage" && appimage_writable();
    #[cfg(not(any(windows, target_os = "linux")))]
    let supported = false;
    supported && has_public_key()
}

pub(crate) fn updater_available() -> bool {
    #[cfg(windows)]
    let supported = true;
    #[cfg(target_os = "linux")]
    let supported = package_channel() != "development";
    #[cfg(not(any(windows, target_os = "linux")))]
    let supported = false;
    supported && has_public_key()
}

fn nightly_supported() -> bool {
    cfg!(windows)
}

fn package_channel() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("APPIMAGE").is_some()
            || matches!(
                tauri::utils::platform::bundle_type(),
                Some(tauri::utils::config::BundleType::AppImage)
            )
        {
            "appimage"
        } else if matches!(
            tauri::utils::platform::bundle_type(),
            Some(tauri::utils::config::BundleType::Rpm)
        ) {
            "rpm"
        } else if matches!(
            tauri::utils::platform::bundle_type(),
            Some(tauri::utils::config::BundleType::Deb)
        ) {
            "deb"
        } else if std::env::current_exe()
            .ok()
            .is_some_and(|path| path == std::path::Path::new("/usr/bin/vrcs-desktop"))
        {
            "system"
        } else if cfg!(debug_assertions) {
            "development"
        } else {
            "manual"
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        if cfg!(debug_assertions) {
            "development"
        } else if cfg!(windows) {
            "installer"
        } else {
            "manual"
        }
    }
}

fn acquire_busy(state: &UpdateState) -> Result<BusyGuard<'_>, UpdateError> {
    state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| UpdateError::Busy)?;
    Ok(BusyGuard(&state.busy))
}

pub(crate) fn register_plugin(app: &tauri::App) -> tauri::Result<()> {
    if !updater_available() {
        return Ok(());
    }
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
pub(crate) fn app_build_info(app: AppHandle) -> BuildInfo {
    BuildInfo {
        version: app.package_info().version.to_string(),
        variant: variant(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        package_channel: package_channel(),
        updater_available: updater_available(),
        can_install: can_install_updates(),
    }
}

async fn check_channel(
    app: &AppHandle,
    channel: UpdateChannel,
) -> Result<Option<Update>, UpdateError> {
    let endpoint = channel.endpoint().parse().map_err(|error| {
        tracing::error!(?channel, %error, "invalid updater endpoint");
        UpdateError::CheckFailed(channel)
    })?;
    let before_exit_app = app.clone();
    let result = async {
        app.updater_builder()
            .endpoints(vec![endpoint])?
            .version_comparator(move |current, release| {
                channel.matches(&release.version)
                    && release.version.cmp_precedence(&current).is_gt()
            })
            .timeout(UPDATE_TIMEOUT)
            .on_before_exit(move || crate::prepare_for_exit(&before_exit_app))
            .build()?
            .check()
            .await
    }
    .await;

    result.map_err(|error| {
        tracing::warn!(?channel, %error, "application update check failed");
        UpdateError::CheckFailed(channel)
    })
}

#[tauri::command]
pub(crate) async fn check_for_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
) -> Result<Option<UpdateMetadata>, UpdateError> {
    if !updater_available() {
        return Err(UpdateError::Unavailable);
    }
    let _busy = acquire_busy(&state)?;
    *state.pending.lock().map_err(|_| UpdateError::Failed)? = None;
    let receive_nightly = nightly_supported()
        && app
            .store("preferences.json")
            .map_err(|error| {
                tracing::warn!(%error, "could not read update preferences");
                UpdateError::Failed
            })?
            .get(NIGHTLY_PREFERENCE)
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

    let stable = check_channel(&app, UpdateChannel::Stable).await;
    let nightly = if receive_nightly {
        check_channel(&app, UpdateChannel::Nightly).await
    } else {
        Ok(None)
    };
    let (update, channel) = match (stable, nightly) {
        (Ok(Some(stable)), Ok(Some(nightly))) => {
            let stable_version =
                Version::parse(&stable.version).map_err(|_| UpdateError::Failed)?;
            let nightly_version =
                Version::parse(&nightly.version).map_err(|_| UpdateError::Failed)?;
            if nightly_version.cmp_precedence(&stable_version).is_gt() {
                (nightly, UpdateChannel::Nightly)
            } else {
                (stable, UpdateChannel::Stable)
            }
        }
        (Ok(Some(stable)), _) => (stable, UpdateChannel::Stable),
        (_, Ok(Some(nightly))) => (nightly, UpdateChannel::Nightly),
        (Ok(None), Ok(None)) => return Ok(None),
        (Err(_), Err(_)) => return Err(UpdateError::BothChecksFailed),
        (Err(error), _) | (_, Err(error)) => return Err(error),
    };
    let metadata = UpdateMetadata {
        version: update.version.clone(),
        current_version: update.current_version.clone(),
        notes: update.body.clone(),
        channel,
        release_url: Some(RELEASE_PAGE_URL.to_owned()),
    };
    *state.pending.lock().map_err(|_| UpdateError::Failed)? =
        can_install_updates().then_some(update);
    Ok(Some(metadata))
}

#[tauri::command]
pub(crate) fn set_receive_nightly_updates(
    app: AppHandle,
    state: State<'_, UpdateState>,
    enabled: bool,
) -> Result<(), UpdateError> {
    let _busy = acquire_busy(&state)?;
    let mut pending = state.pending.lock().map_err(|_| UpdateError::Failed)?;
    let store = app.store("preferences.json").map_err(|error| {
        tracing::warn!(%error, "could not load update preferences");
        UpdateError::PreferenceSaveFailed
    })?;
    let previous = store.get(NIGHTLY_PREFERENCE);
    store.set(NIGHTLY_PREFERENCE, enabled);
    if let Err(error) = store.save() {
        if let Some(value) = previous {
            store.set(NIGHTLY_PREFERENCE, value);
        } else {
            store.delete(NIGHTLY_PREFERENCE);
        }
        tracing::warn!(%error, "could not save Nightly update preference");
        return Err(UpdateError::PreferenceSaveFailed);
    }
    *pending = None;
    Ok(())
}

#[tauri::command]
pub(crate) async fn download_and_install_update(
    app: AppHandle,
    state: State<'_, UpdateState>,
    version: String,
    on_event: Channel<DownloadEvent>,
) -> Result<(), UpdateError> {
    if !can_install_updates() {
        return Err(UpdateError::Unavailable);
    }
    let _busy = acquire_busy(&state)?;
    let update = state
        .pending
        .lock()
        .map_err(|_| UpdateError::Failed)?
        .take_if(|update| update.version == version)
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
        })?;
    #[cfg(target_os = "linux")]
    {
        let restart_app = app.clone();
        let _ = tauri::async_runtime::spawn_blocking(move || {
            crate::prepare_for_exit(&restart_app);
            restart_app.restart();
        })
        .await;
    }
    #[cfg(not(target_os = "linux"))]
    let _ = app;
    Ok(())
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
        assert_eq!(variant(), "standard");
        assert_eq!(
            target(),
            format!(
                "{}-{}-standard",
                std::env::consts::OS,
                std::env::consts::ARCH
            )
        );
    }
}
