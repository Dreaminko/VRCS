use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_database_path")]
    pub database_path: String,
    #[serde(default = "default_model_directory")]
    pub model_directory: String,
    #[serde(default = "default_history_limit")]
    pub subtitle_history_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExternalApiConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_external_api_host")]
    pub host: String,
    #[serde(default = "default_external_api_port")]
    pub port: u16,
    #[serde(default)]
    pub require_token: bool,
}

fn default_host() -> String {
    "127.0.0.1".into()
}

fn default_port() -> u16 {
    8766
}

fn default_external_api_host() -> String {
    "127.0.0.1".into()
}

fn default_external_api_port() -> u16 {
    8767
}

fn default_database_path() -> String {
    "data/vrcs.db".into()
}

fn default_model_directory() -> String {
    "models/whisper".into()
}

fn default_history_limit() -> u32 {
    500
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            database_path: default_database_path(),
            model_directory: default_model_directory(),
            subtitle_history_limit: default_history_limit(),
        }
    }
}

impl Default for ExternalApiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            host: default_external_api_host(),
            port: default_external_api_port(),
            require_token: false,
        }
    }
}
