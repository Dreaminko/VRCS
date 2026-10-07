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

#[test]
fn legacy_ocr_wrist_position_is_copied_and_saved_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["ocr"].as_object_mut().unwrap().remove("wrist");
    raw["vr_overlay"]["wrist"]["hand"] = serde_json::json!("right");
    raw["vr_overlay"]["wrist"]["width_m"] = serde_json::json!(0.46);
    raw["vr_overlay"]["wrist"]["offset_x_m"] = serde_json::json!(0.12);
    raw["vr_overlay"]["wrist"]["font_size_px"] = serde_json::json!(60);
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();

    let config = load_config(&path).unwrap();
    let mut saved: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(saved["ocr"]["wrist"]["hand"], "right");
    assert!((saved["ocr"]["wrist"]["width_m"].as_f64().unwrap() - 0.46).abs() < 1e-6);
    assert!((saved["ocr"]["wrist"]["offset_x_m"].as_f64().unwrap() - 0.12).abs() < 1e-6);
    assert_eq!(saved["ocr"]["wrist"]["font_size_px"], 32);

    saved["vr_overlay"]["wrist"]["width_m"] = serde_json::json!(0.7);
    fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
    let reloaded = load_config(&path).unwrap();
    let saved_wrist: super::VrOcrWristConfig =
        serde_json::from_value(saved["ocr"]["wrist"].clone()).unwrap();
    assert_eq!(reloaded.ocr.wrist.as_ref(), Some(&saved_wrist));
    assert_eq!(reloaded.ocr.wrist, config.ocr.wrist);
}

#[test]
fn null_ocr_wrist_config_is_initialized_on_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    let mut raw = serde_json::to_value(AppConfig::default()).unwrap();
    raw["ocr"]["wrist"] = serde_json::Value::Null;
    fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    load_config(&path).unwrap();
    let saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(saved["ocr"]["wrist"].is_object());
}
