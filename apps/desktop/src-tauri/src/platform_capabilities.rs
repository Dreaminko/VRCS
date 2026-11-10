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
    vr: VrCapabilities,
    update_method: &'static str,
}

#[derive(Serialize)]
struct VrCapabilities {
    subtitles: Capability,
    controller_input: Capability,
    scene_capture: Capability,
}

pub(crate) fn session_type() -> String {
    if cfg!(target_os = "linux") {
        if std::env::var_os("WAYLAND_DISPLAY").is_some()
            || std::env::var("XDG_SESSION_TYPE").is_ok_and(|session| session == "wayland")
        {
            "wayland".into()
        } else if std::env::var_os("DISPLAY").is_some() {
            "x11".into()
        } else {
            "unknown".into()
        }
    } else {
        std::env::consts::OS.into()
    }
}

#[tauri::command]
pub(crate) async fn platform_capabilities(
    vr: State<'_, crate::vr_overlay::Manager>,
) -> Result<PlatformCapabilities, String> {
    let windows = cfg!(windows);
    let session_type = session_type();
    let vr_status = vr.status()?;
    let vr_reason = if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "vr.runtime_arch_unverified"
    } else if windows {
        "platform.vr_runtime_unavailable"
    } else {
        "platform.vr_unavailable"
    };
    let vr_available = windows && vr_status.runtime_installed && vr_status.hmd_present;
    #[cfg(target_os = "linux")]
    let (screenshot, shortcuts) = tokio::join!(crate::desktop_capture::portal::available(), crate::desktop_capture::portal_shortcuts::available());
    #[cfg(not(target_os = "linux"))]
    let (screenshot, shortcuts) = (windows, windows);
    // Without a native Wayland toplevel protocol we cannot promise global stacking.
    let topmost = windows || (cfg!(target_os = "linux") && session_type == "x11");
    Ok(PlatformCapabilities {
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        session_type,
        screenshot: Capability::runtime(cfg!(any(windows, target_os = "linux")), screenshot, "platform.screenshot_unavailable"),
        always_on_top: Capability::runtime(
            topmost,
            topmost,
            "platform.window_stacking_unavailable",
        ),
        global_shortcuts: Capability::runtime(cfg!(any(windows, target_os = "linux")), shortcuts, "platform.shortcuts_unavailable"),
        vr: VrCapabilities {
            subtitles: Capability::runtime(windows, vr_available, vr_reason),
            controller_input: Capability::runtime(windows, vr_available, vr_reason),
            scene_capture: Capability::runtime(windows, vr_available, vr_reason),
        },
        update_method: if crate::app_updates::updater_available() {
            "in_app"
        } else {
            "manual"
        },
    })
}
