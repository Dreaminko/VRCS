//! Desktop screenshot capture via `org.freedesktop.portal.Screenshot`.
use super::image::{decode_png, Image};
use ashpd::desktop::screenshot::Screenshot;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tokio::sync::Notify;

fn portal_error(error: ashpd::Error) -> String {
    use ashpd::{Error, PortalError};
    match error {
        Error::Response(ashpd::desktop::ResponseError::Cancelled)
        | Error::Portal(PortalError::Cancelled(_)) => "capture.cancelled".into(),
        Error::Response(_) | Error::Portal(PortalError::NotAllowed(_)) => "capture.rejected".into(),
        Error::PortalNotFound(_) | Error::RequiresVersion(_, _) => "capture.no_portal".into(),
        other => {
            tracing::warn!(%other, "Screenshot Portal failed");
            "capture.portal_failed".into()
        }
    }
}

pub async fn available() -> bool {
    let check = async {
        let conn = ashpd::zbus::Connection::session().await.ok()?;
        let proxy = ashpd::zbus::Proxy::new(
            &conn,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Screenshot",
        )
        .await
        .ok()?;
        proxy.get_property::<u32>("version").await.ok()
    };
    tokio::time::timeout(Duration::from_secs(2), check)
        .await
        .ok()
        .flatten()
        .is_some_and(|version| version >= 1)
}

pub async fn capture_region(
    cancelled: Arc<AtomicBool>,
    cancel_notify: Arc<Notify>,
    on_selecting: impl FnOnce(),
) -> Result<Option<Image>, String> {
    let _permit = super::reserve_picker().map_err(|_| "capture.selection_busy")?;
    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    on_selecting();

    let wait_cancel = async {
        loop {
            let notified = cancel_notify.notified();
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            notified.await;
        }
    };

    let request = tokio::select! {
        result = Screenshot::request().interactive(true).modal(true).send() => {
            match result {
                Ok(request) => request,
                Err(error) => {
                    let mapped = portal_error(error);
                    return if mapped == "capture.cancelled" {
                        Ok(None)
                    } else {
                        Err(mapped)
                    };
                }
            }
        }
        _ = wait_cancel => return Ok(None),
    };

    let screenshot = match request.response() {
        Ok(screenshot) => screenshot,
        Err(error) => {
            let mapped = portal_error(error);
            return if mapped == "capture.cancelled" {
                Ok(None)
            } else {
                Err(mapped)
            };
        }
    };

    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }

    let path = screenshot.uri().to_file_path().map_err(|_| {
        tracing::warn!(uri = %screenshot.uri(), "Screenshot portal returned non-file URI");
        "capture.portal_failed".to_string()
    })?;

    let image = tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(&path).map_err(|error| {
            tracing::warn!(%error, path = %path.display(), "Failed to read screenshot portal file");
            "capture.portal_failed".to_string()
        })?;
        decode_png(&bytes)
    })
    .await
    .map_err(|_| "capture.portal_failed".to_string())??;

    if cancelled.load(Ordering::Acquire) {
        return Ok(None);
    }
    Ok(Some(image))
}
