use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use crate::config::{load_config, AppConfig};
use crate::{asr, resolve_config_path, CoreOptions};

pub(crate) struct StartupPlan {
    pub(crate) config_path: PathBuf,
    pub(crate) config: AppConfig,
    pub(crate) requested_address: SocketAddr,
    pub(crate) session_token: String,
    pub(crate) database_path: PathBuf,
    pub(crate) glossary_cache_path: PathBuf,
    pub(crate) vad_model_path: PathBuf,
    pub(crate) managed_vad_model: bool,
    pub(crate) defer_managed_vad: bool,
    pub(crate) asr_model_dir: PathBuf,
    pub(crate) asr_model_dir_override: Option<PathBuf>,
}

impl StartupPlan {
    pub(crate) fn resolve(options: CoreOptions, defer_managed_vad: bool) -> Result<Self, String> {
        let mut config = load_config(&options.config_path)?;
        config
            .validate_settings()
            .map_err(|error| format!("Invalid startup configuration: {error}"))?;
        asr::validate_config(&mut config.asr)
            .map_err(|error| format!("Invalid startup configuration: {error}"))?;

        let host = options.host.unwrap_or_else(|| config.server.host.clone());
        let port = options.port.unwrap_or(config.server.port);
        config.server.host = host.clone();
        config.server.port = port;
        let supplied_session_token = options
            .session_token
            .filter(|token| !token.trim().is_empty());
        let requested_address = SocketAddr::new(
            host.parse()
                .map_err(|_| format!("Invalid listen address: {host}"))?,
            port,
        );
        if !requested_address.ip().is_loopback() && supplied_session_token.is_none() {
            return Err("VRCS_SESSION_TOKEN is required for non-loopback listen addresses".into());
        }
        let session_token =
            supplied_session_token.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

        let config_dir = options
            .config_path
            .parent()
            .unwrap_or_else(|| Path::new("."));
        let database_path =
            resolve_config_path(&options.config_path, &config.storage.database_path);
        let managed_vad_model = options.vad_model_path.is_none();
        let vad_model_path = options
            .vad_model_path
            .unwrap_or_else(|| config_dir.join("models").join("silero_vad.onnx"));
        let asr_model_dir_override = options.asr_model_dir.clone();
        let asr_model_dir = options.asr_model_dir.unwrap_or_else(|| {
            resolve_config_path(&options.config_path, &config.storage.model_directory)
        });
        let glossary_cache_path = options
            .config_path
            .with_file_name("glossary-subscription-cache.json");

        Ok(Self {
            config_path: options.config_path,
            config,
            requested_address,
            session_token,
            database_path,
            glossary_cache_path,
            vad_model_path,
            managed_vad_model,
            defer_managed_vad,
            asr_model_dir,
            asr_model_dir_override,
        })
    }
}
