use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::migration::{config_from_value, config_version};
use super::{AppConfig, SCHEMA_VERSION};

pub fn load_config(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        let config = AppConfig::default();
        save_config(path, &config)?;
        return Ok(config);
    }
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let raw: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("Configuration JSON is invalid: {error}"))?;
    let version = config_version(&raw)?;
    let config = config_from_value(&raw)?;
    if version != SCHEMA_VERSION as u64 {
        let backup = path.with_extension(format!("v{version}.backup.json"));
        if !backup.exists() {
            fs::copy(path, &backup)
                .map_err(|error| format!("无法备份旧配置到 {}：{error}", backup.display()))?;
        }
        save_config(path, &config)?;
    }
    Ok(config)
}

/// 原子写入：先写临时文件再替换，避免崩溃时留下半截配置。
pub fn save_config(path: &Path, config: &AppConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let payload = serde_json::to_string_pretty(config).map_err(|error| error.to_string())?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let temporary: PathBuf =
        path.with_extension(format!("json.{}.{nonce}.tmp", std::process::id()));
    let result = (|| {
        let mut file = fs::File::create(&temporary).map_err(|error| error.to_string())?;
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
