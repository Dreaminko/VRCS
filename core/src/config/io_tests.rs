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
