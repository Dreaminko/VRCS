#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(target_os = "linux")]
fn configure_webkit_env() {
    let wayland = vrcs_desktop_lib::platform_capabilities::session_type() == "wayland";
    if wayland
        && std::path::Path::new("/sys/module/nvidia").exists()
        && std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none()
    {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }
}

#[cfg(target_os = "linux")]
fn configure_openvr_env() {
    if std::env::var_os("VR_OVERRIDE").is_none_or(|value| value.is_empty()) {
        return;
    }
    let config_root = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|value| !value.is_empty())
                .map(|home| std::path::PathBuf::from(home).join(".config"))
        });
    let registry_path = std::env::var_os("VR_PATHREG_OVERRIDE")
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            config_root
                .as_ref()
                .map(|root| root.join("openvr/openvrpaths.vrpath"))
        });
    if registry_path.as_ref().is_some_and(|path| path.is_file()) {
        return;
    }
    if std::env::var_os("VR_CONFIG_PATH")
        .filter(|value| !value.is_empty())
        .is_none()
    {
        if let Some(config_dir) = config_root.as_ref().map(|root| root.join("openvr")) {
            std::env::set_var("VR_CONFIG_PATH", config_dir);
        }
    }
    if std::env::var_os("VR_LOG_PATH")
        .filter(|value| !value.is_empty())
        .is_none()
    {
        let log_dir = std::env::var_os("XDG_CACHE_HOME")
            .filter(|value| !value.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| {
                std::env::var_os("HOME")
                    .filter(|value| !value.is_empty())
                    .map(|home| std::path::PathBuf::from(home).join(".cache"))
            })
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("openvr");
        std::env::set_var("VR_LOG_PATH", log_dir);
    }
}

fn main() {
    #[cfg(target_os = "linux")]
    if std::env::args().nth(1).as_deref() == Some("--network-helper") {
        if let Err(error) = vrcs_core::run_network_helper(std::env::args().skip(2)) {
            eprintln!("vrcs-network-helper: {error}");
            std::process::exit(1);
        }
        return;
    }
    #[cfg(target_os = "linux")]
    if std::env::var_os("PKEXEC_UID").is_some() {
        eprintln!(
            "Refusing to run the desktop UI under pkexec; only --network-helper is permitted."
        );
        std::process::exit(1);
    }
    #[cfg(target_os = "linux")]
    {
        configure_webkit_env();
        configure_openvr_env();
    }
    if std::env::args().any(|argument| argument == "--release-self-test") {
        if let Err(error) = vrcs_desktop_lib::release_self_test() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    vrcs_desktop_lib::run();
}
