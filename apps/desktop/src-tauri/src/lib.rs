#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running VRCS");
}

#[cfg(test)]
mod tests {
    const CAPABILITIES: &str = include_str!("../capabilities/default.json");

    #[test]
    fn custom_window_actions_are_allowed() {
        for permission in [
            "core:window:allow-start-dragging",
            "core:window:allow-minimize",
            "core:window:allow-toggle-maximize",
            "core:window:allow-close",
        ] {
            assert!(CAPABILITIES.contains(permission), "missing {permission}");
        }
    }
}
