use serde::Serialize;
use tauri::State;
use vrcs_core::platform_capabilities::Capability;

#[derive(Serialize)]
pub(crate) struct PlatformCapabilities {
    os: &'static str,
    arch: &'static str,
    session_type: String,
    screenshot: Capability,
    always_on_top: Capability,
    global_shortcuts: Capability,
    shortcut_bindings: Option<Vec<String>>,
    vr: VrCapabilities,
}

#[derive(Serialize)]
struct VrCapabilities {
    subtitles: Capability,
    controller_input: Capability,
    scene_capture: Capability,
}

fn non_empty_env(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

fn detect_linux_session_type(
    gdk_backend: Option<&str>,
    wayland_display: Option<&str>,
    xdg_session_type: Option<&str>,
    display: Option<&str>,
) -> &'static str {
    let has_wayland = non_empty_env(wayland_display).is_some()
        || non_empty_env(xdg_session_type).is_some_and(|session| session == "wayland");
    let has_x11 = non_empty_env(display).is_some();
    if let Some(backend) = non_empty_env(gdk_backend)
        .and_then(|list| list.split(',').map(str::trim).find(|item| !item.is_empty()))
    {
        if backend.eq_ignore_ascii_case("x11") && has_x11 {
            return "x11";
        }
        if backend.eq_ignore_ascii_case("wayland") && has_wayland {
            return "wayland";
        }
    }
    if has_wayland {
        "wayland"
    } else if has_x11 || non_empty_env(xdg_session_type).is_some_and(|session| session == "x11") {
        "x11"
    } else {
        "unknown"
    }
}

pub fn session_type() -> String {
    if cfg!(target_os = "linux") {
        let gdk_backend = std::env::var("GDK_BACKEND").ok();
        let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
        let xdg_session_type = std::env::var("XDG_SESSION_TYPE").ok();
        let display = std::env::var("DISPLAY").ok();
        detect_linux_session_type(
            gdk_backend.as_deref(),
            wayland_display.as_deref(),
            xdg_session_type.as_deref(),
            display.as_deref(),
        )
        .into()
    } else {
        std::env::consts::OS.into()
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn is_native_x11() -> bool {
    let wayland_display = std::env::var("WAYLAND_DISPLAY").ok();
    let xdg_session_type = std::env::var("XDG_SESSION_TYPE").ok();
    let display = std::env::var("DISPLAY").ok();
    non_empty_env(wayland_display.as_deref()).is_none()
        && !non_empty_env(xdg_session_type.as_deref()).is_some_and(|session| session == "wayland")
        && non_empty_env(display.as_deref()).is_some()
}

#[tauri::command]
pub(crate) async fn platform_capabilities(
    _app: tauri::AppHandle,
    vr: State<'_, crate::vr_overlay::Manager>,
) -> Result<PlatformCapabilities, String> {
    let windows = cfg!(windows);
    let session_type = session_type();
    let vr_status = vr.status()?;
    let vr_supported = windows || cfg!(linux_vr);
    let vr_unsupported_reason = if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "vr.runtime_arch_unverified"
    } else {
        "platform.vr_unavailable"
    };
    let vr_available = vr_supported && vr_status.runtime_installed && vr_status.hmd_present;
    #[cfg(target_os = "linux")]
    let (screenshot, shortcuts) = tokio::join!(
        crate::desktop_capture::portal::available(),
        crate::desktop_capture::portal_shortcuts::available()
    );
    #[cfg(not(target_os = "linux"))]
    let (screenshot, shortcuts) = (windows, windows);
    // Without a native Wayland toplevel protocol we cannot promise global stacking.
    let topmost = windows || (cfg!(target_os = "linux") && session_type == "x11");
    Ok(PlatformCapabilities {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        session_type,
        screenshot: Capability::runtime(
            cfg!(any(windows, target_os = "linux")),
            screenshot,
            "platform.screenshot_unavailable",
        ),
        always_on_top: Capability::runtime(
            topmost,
            topmost,
            "platform.window_stacking_unavailable",
        ),
        global_shortcuts: Capability::runtime(
            cfg!(any(windows, target_os = "linux")),
            shortcuts,
            "platform.shortcuts_unavailable",
        ),
        shortcut_bindings: {
            #[cfg(target_os = "linux")]
            {
                use tauri::Manager as _;
                _app.state::<crate::desktop_capture::portal_shortcuts::Manager>()
                    .bindings(&_app)
            }
            #[cfg(not(target_os = "linux"))]
            {
                None
            }
        },
        vr: VrCapabilities {
            subtitles: Capability {
                supported: vr_supported,
                available: vr_available,
                reason_code: (!vr_supported).then_some(vr_unsupported_reason),
            },
            controller_input: Capability {
                supported: vr_supported,
                available: vr_available,
                reason_code: (!vr_supported).then_some(vr_unsupported_reason),
            },
            scene_capture: Capability {
                supported: windows,
                available: windows && vr_available,
                reason_code: (!windows).then_some("vr.scene_capture_unavailable"),
            },
        },
    })
}

#[cfg(test)]
mod tests {
    use super::detect_linux_session_type;

    #[test]
    fn linux_session_type_honors_gdk_backend_and_empty_vars() {
        assert_eq!(
            detect_linux_session_type(Some("x11"), Some("wayland-0"), Some("wayland"), Some(":0")),
            "x11"
        );
        assert_eq!(
            detect_linux_session_type(
                Some("wayland,x11"),
                Some("wayland-0"),
                Some("wayland"),
                Some(":0")
            ),
            "wayland"
        );
        assert_eq!(
            detect_linux_session_type(None, Some(""), Some("x11"), Some(":0")),
            "x11"
        );
        assert_eq!(
            detect_linux_session_type(None, Some("wayland-0"), None, Some(":0")),
            "wayland"
        );
        assert_eq!(detect_linux_session_type(None, None, None, None), "unknown");
    }
}
