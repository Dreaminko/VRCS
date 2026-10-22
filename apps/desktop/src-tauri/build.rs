// Declaring the app commands makes Tauri check them against capabilities, so
// each window only reaches the commands granted in `capabilities/*.json`.
const COMMANDS: &[&str] = &[
    "core_connection",
    "core_startup",
    "retry_core",
    "write_glossary_file",
    "open_vrcx_repository",
    "diagnostic_status",
    "report_frontend_error",
    "open_log_directory",
    "export_error_report",
    "app_build_info",
    "check_for_update",
    "download_and_install_update",
    "update_native_labels",
    "set_compact_window_topmost",
    "vr_overlay_status",
    "vr_overlay_retry",
    "vr_ocr_open_bindings",
    "vr_overlay_show_sample",
    "vr_overlay_hide_sample",
    "vr_dashboard_update_view",
    "vr_overlay_control_ready",
    "vr_overlay_control_update",
    "vr_overlay_control_result",
    "vr_overlay_control_release",
    "vr_overlay_placement_result",
    "vr_overlay_place_dialogue",
    #[cfg(windows)]
    "vr_surface_ready",
    #[cfg(windows)]
    "vr_surface_layout",
    "set_shortcut_recording",
    "desktop_ocr_status",
    "desktop_ocr_scan",
    "desktop_ocr_close",
    "desktop_vision_status",
    "desktop_vision_open",
    "desktop_vision_capture",
    "desktop_vision_image",
    "desktop_vision_start_recording",
    "desktop_vision_finish_recording",
    "desktop_vision_ask",
    "desktop_vision_cancel",
    "desktop_vision_close",
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    "vr_preview_open",
    #[cfg(all(windows, debug_assertions, feature = "vr-preview"))]
    "vr_preview_update",
];

fn main() {
    println!("cargo:rerun-if-env-changed=TAURI_UPDATER_PUBLIC_KEY");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri build script");
}
