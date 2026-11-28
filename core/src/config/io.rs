use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::migration::{LEGACY_WRIST_MENU_FIELDS, config_from_value, config_version};
use super::{AppConfig, SCHEMA_VERSION};

#[cfg(target_os = "linux")]
fn protect_config_files(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let prefix = format!(
        "{}.v",
        path.file_stem().unwrap_or_default().to_string_lossy()
    );
    for entry in fs::read_dir(parent)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name
            .strip_prefix(&prefix)
            .and_then(|name| name.strip_suffix(".backup.json"))
            .is_some_and(|version| version.parse::<u64>().is_ok())
            && entry.file_type()?.is_file()
        {
            fs::set_permissions(entry.path(), fs::Permissions::from_mode(0o600))?;
        }
    }
    Ok(())
}

pub fn load_config(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        let config = AppConfig::default();
        save_config(path, &config)?;
        return Ok(config);
    }
    #[cfg(target_os = "linux")]
    protect_config_files(path).map_err(|error| error.to_string())?;
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let raw: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("Configuration JSON is invalid: {error}"))?;
    let version = config_version(&raw)?;
    let config = config_from_value(&raw)?;
    let migrate_ocr_backend = raw.get("ocr").and_then(|ocr| ocr.get("backend")).is_some();
    let migrate_ocr_wrist = raw
        .get("ocr")
        .and_then(|ocr| ocr.get("wrist"))
        .is_none_or(serde_json::Value::is_null);
    let migrate_vision_wrist = raw
        .get("vision")
        .and_then(|vision| vision.get("wrist"))
        .is_none_or(serde_json::Value::is_null);
    let migrate_wrist_menu_legacy = raw
        .get("vr_overlay")
        .and_then(|overlay| overlay.get("wrist_menu"))
        .and_then(serde_json::Value::as_object)
        .is_some_and(|wrist_menu| {
            LEGACY_WRIST_MENU_FIELDS
                .iter()
                .any(|field| wrist_menu.contains_key(*field))
        });
    if version != SCHEMA_VERSION as u64 {
        let backup = path.with_extension(format!("v{version}.backup.json"));
        if backup.exists() && !backup.is_file() {
            return Err(format!(
                "Configuration backup path is not a file: {}",
                backup.display()
            ));
        }
        if !backup.exists() {
            fs::copy(path, &backup).map_err(|error| {
                format!(
                    "Failed to back up the previous configuration to {}: {error}",
                    backup.display()
                )
            })?;
        }
        save_config(path, &config)?;
    } else if migrate_ocr_backend
        || migrate_ocr_wrist
        || migrate_vision_wrist
        || migrate_wrist_menu_legacy
    {
        save_config(path, &config)?;
    }
    Ok(config)
}

/// Writes atomically via a temporary file and rename to avoid leaving a partial config on crash.
pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    let mut config = config.clone();
    config.resolve_model_assignments();
    if let Some(parent) = path.parent() {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent).map_err(|error| error.to_string())?;
    }
    let payload = serde_json::to_string_pretty(&config).map_err(|error| error.to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary: PathBuf =
        path.with_extension(format!("json.{}.{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(target_os = "linux")]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&temporary)
            .map_err(|error| error.to_string())?;
        file.write_all(payload.as_bytes())
            .map_err(|error| error.to_string())?;
        file.sync_all().map_err(|error| error.to_string())?;
        drop(file);
        fs::rename(&temporary, path).map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
