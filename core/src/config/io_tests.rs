use std::fs;

use super::{load_config, AppConfig};

#[test]
fn default_config_round_trips() {
    let dir = std::env::temp_dir().join(format!("vrcs-config-{}", std::process::id()));
    let path = dir.join("config.json");
    let config = load_config(&path).unwrap();
    assert_eq!(config, AppConfig::default());
    let reloaded = load_config(&path).unwrap();
    assert_eq!(reloaded, config);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn legacy_vr_overlay_config_loads_with_backup_and_preserves_settings() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["schema_version"] = serde_json::json!(26);
    raw["vr_overlay"]["headset"]["content_mode"] = serde_json::json!("bilingual");
    raw["vr_overlay"]["wrist"]["content_mode"] = serde_json::json!("bilingual");
    raw["vr_overlay"]["headset"]["width_m"] = serde_json::json!(1.7);
    let original = serde_json::to_string_pretty(&raw).unwrap();
    fs::write(&path, &original).unwrap();

    let config = load_config(&path).unwrap();
    assert_eq!(config.vr_overlay.headset.width_m, 1.7);
    assert_eq!(config.vr_overlay.translation_display, "all_languages");
    assert_eq!(
        fs::read_to_string(path.with_extension("v26.backup.json")).unwrap(),
        original
    );
    let saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert!(saved["vr_overlay"]["headset"].get("content_mode").is_none());
    assert!(saved["vr_overlay"]["wrist"].get("content_mode").is_none());
    assert_eq!(load_config(&path).unwrap(), config);
}
