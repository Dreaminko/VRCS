//! VRCS Rust Core，可由独立二进制或 Tauri 主进程启动。

mod anki;
mod asr;
mod audio;
mod config;
mod db;
mod error;
mod models;
mod pipeline;
mod server;
mod vad;
mod yomitan;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::config::load_config;
use crate::db::Database;
use crate::pipeline::TranscriptionPipeline;
use crate::server::{AppState, CORE_VERSION};

pub struct CoreOptions {
    pub config_path: PathBuf,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub session_token: Option<String>,
    pub vad_model_path: Option<PathBuf>,
    pub asr_model_dir: Option<PathBuf>,
}

impl CoreOptions {
    pub fn from_env() -> Self {
        Self {
            config_path: std::env::var("VRCS_CONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("config.json")),
            host: std::env::var("VRCS_HOST").ok(),
            port: std::env::var("VRCS_PORT")
                .ok()
                .and_then(|value| value.parse().ok()),
            session_token: std::env::var("VRCS_SESSION_TOKEN").ok(),
            vad_model_path: std::env::var("VRCS_SILERO_MODEL").ok().map(PathBuf::from),
            asr_model_dir: std::env::var("VRCS_ASR_MODEL_DIR").ok().map(PathBuf::from),
        }
    }
}

pub struct CoreHandle {
    address: SocketAddr,
    session_token: String,
    shutdown: watch::Sender<bool>,
    task: JoinHandle<Result<(), String>>,
    state: Arc<AppState>,
    model_manager: Arc<asr::ModelManager>,
    vad_runtime: vad::VadRuntimeState,
    vad_prepare_task: Option<JoinHandle<()>>,
}

impl CoreHandle {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn session_token(&self) -> &str {
        &self.session_token
    }

    pub fn vad_backend(&self) -> &'static str {
        self.vad_runtime.backend()
    }

    pub fn vad_model_version(&self) -> Option<&'static str> {
        self.vad_runtime.model_version()
    }

    pub fn stop(&mut self) {
        if let Some(task) = self.vad_prepare_task.take() {
            task.abort();
        }
        self.model_manager.cancel_all();
        let _ = self.shutdown.send(true);
    }

    pub async fn wait(mut self) -> Result<(), String> {
        if let Some(task) = self.vad_prepare_task.take() {
            task.abort();
            let _ = task.await;
        }
        let result = self
            .task
            .await
            .map_err(|error| format!("Core task failed: {error}"))?;
        let _ = self.shutdown.send(true);
        let _control = self.state.capture_control.lock().await;
        self.state.speaker_pipeline.lock().await.stop().await;
        self.state.microphone_pipeline.lock().await.stop().await;
        self.model_manager.cancel_all_and_wait().await;
        result
    }

    pub async fn shutdown(mut self) -> Result<(), String> {
        self.stop();
        self.wait().await
    }
}

pub struct LoggingGuard {
    _guard: Option<tracing_appender::non_blocking::WorkerGuard>,
}

pub fn init_tracing(log_dir: Option<&Path>) -> Result<LoggingGuard, String> {
    use tracing_subscriber::fmt::writer::MakeWriterExt;

    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "vrcs_core=info,tower_http=info".into());
    if let Some(log_dir) = log_dir {
        std::fs::create_dir_all(log_dir)
            .map_err(|error| format!("无法创建日志目录 {}：{error}", log_dir.display()))?;
        let appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("vrcs-core")
            .filename_suffix("log")
            .max_log_files(4)
            .build(log_dir)
            .map_err(|error| format!("无法创建日志文件：{error}"))?;
        let (file_writer, guard) = tracing_appender::non_blocking(appender);
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(file_writer.and(std::io::stderr))
            .try_init()
            .map_err(|error| format!("无法初始化日志：{error}"))?;
        Ok(LoggingGuard {
            _guard: Some(guard),
        })
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(|error| format!("无法初始化日志：{error}"))?;
        Ok(LoggingGuard { _guard: None })
    }
}

pub(crate) fn resolve_config_path(config_path: &Path, value: &str) -> PathBuf {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        path
    } else {
        config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(path)
    }
}

pub async fn start(options: CoreOptions) -> Result<CoreHandle, String> {
    start_inner(options, false).await
}

/// Starts the Core without putting managed VAD preparation on the critical path.
///
/// A missing or invalid managed model is downloaded and validated in the
/// background. Capture can start immediately with the existing energy fallback;
/// a later capture start will use Silero after the model becomes available.
pub async fn start_with_deferred_vad(options: CoreOptions) -> Result<CoreHandle, String> {
    start_inner(options, true).await
}

async fn start_inner(options: CoreOptions, defer_managed_vad: bool) -> Result<CoreHandle, String> {
    let startup_started = Instant::now();
    let mut config = load_config(&options.config_path)?;
    config
        .validate_settings()
        .map_err(|error| format!("启动配置无效：{error}"))?;
    asr::validate_config(&config.asr).map_err(|error| format!("启动配置无效：{error}"))?;
    let host = options.host.unwrap_or_else(|| config.server.host.clone());
    let port = options.port.unwrap_or(config.server.port);
    config.server.host = host.clone();
    config.server.port = port;
    let supplied_session_token = options
        .session_token
        .filter(|token| !token.trim().is_empty());
    let address = SocketAddr::new(
        host.parse()
            .map_err(|_| format!("无效的监听地址：{host}"))?,
        port,
    );
    if !address.ip().is_loopback() && supplied_session_token.is_none() {
        return Err("非回环监听地址必须配置 VRCS_SESSION_TOKEN".into());
    }
    let session_token =
        supplied_session_token.unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());

    let config_dir = options
        .config_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let database_path = resolve_config_path(&options.config_path, &config.storage.database_path);
    let database = Database::open(&database_path)
        .map_err(|error| format!("无法打开数据库 {}: {error}", database_path.display()))?;
    let managed_vad_model = options.vad_model_path.is_none();
    let vad_model_path = options
        .vad_model_path
        .unwrap_or_else(|| config_dir.join("models").join("silero_vad.onnx"));
    let vad_prepare_task = if managed_vad_model && defer_managed_vad {
        let path = vad_model_path.clone();
        Some(tokio::spawn(async move {
            let started = Instant::now();
            match vad::ensure_model(&path).await {
                Ok(()) => tracing::info!(
                    elapsed_ms = started.elapsed().as_millis(),
                    "Silero VAD model prepared in background"
                ),
                Err(error) => tracing::warn!(
                    %error,
                    elapsed_ms = started.elapsed().as_millis(),
                    "Silero VAD model unavailable; energy fallback remains active"
                ),
            }
        }))
    } else {
        None
    };
    if managed_vad_model && !defer_managed_vad {
        if let Err(error) = vad::ensure_model(&vad_model_path).await {
            tracing::warn!(%error, "Silero VAD model unavailable; using energy fallback");
        }
    }
    let vad_runtime = vad::VadRuntimeState::default();
    if !defer_managed_vad {
        let _ = vad::VoiceDetector::load_with_runtime(&vad_model_path, vad_runtime.clone());
    }
    let asr_model_dir_override = options.asr_model_dir.clone();
    let asr_model_dir = options.asr_model_dir.unwrap_or_else(|| {
        resolve_config_path(&options.config_path, &config.storage.model_directory)
    });
    let model_manager = Arc::new(asr::ModelManager::new(asr_model_dir.clone())?);
    let asr_config = config.asr.clone();
    let (subtitles_tx, _) = broadcast::channel(50);
    let (live_tx, _) = broadcast::channel(100);
    let db = Arc::new(Mutex::new(database));
    let asr_service = asr::AsrService::new(asr_config, asr_model_dir);
    let asr_runtime = asr_service.runtime_state();
    let asr = Arc::new(Mutex::new(asr_service));
    let handle_vad_runtime = vad_runtime.clone();
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let state = Arc::new(AppState {
        config_path: options.config_path,
        asr_model_dir_override,
        config: RwLock::new(config),
        db,
        subtitles_tx,
        live_tx,
        http: anki::client(),
        session_token: session_token.clone(),
        shutdown: shutdown_rx.clone(),
        vad_runtime: vad_runtime.clone(),
        asr,
        asr_runtime,
        model_manager: Arc::clone(&model_manager),
        capture_control: tokio::sync::Mutex::new(()),
        speaker_pipeline: tokio::sync::Mutex::new(TranscriptionPipeline::new(
            audio::CaptureSource::Speaker,
            "speaker",
            vad_model_path.clone(),
            vad_runtime.clone(),
            shutdown_rx.clone(),
        )),
        microphone_pipeline: tokio::sync::Mutex::new(TranscriptionPipeline::new(
            audio::CaptureSource::Microphone,
            "microphone",
            vad_model_path,
            vad_runtime,
            shutdown_rx.clone(),
        )),
    });

    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|error| format!("无法监听 {address}: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("无法读取监听地址：{error}"))?;
    tracing::info!(version = CORE_VERSION, %address, "vrcs-core listening");
    tracing::info!(
        elapsed_ms = startup_started.elapsed().as_millis(),
        deferred_vad = defer_managed_vad,
        "vrcs-core startup ready"
    );
    let server_state = Arc::clone(&state);
    let mut server_shutdown = shutdown_rx;
    let task = tokio::spawn(async move {
        axum::serve(listener, server::router(server_state))
            .with_graceful_shutdown(async move {
                if !*server_shutdown.borrow() {
                    let _ = server_shutdown.changed().await;
                }
            })
            .await
            .map_err(|error| format!("服务运行失败: {error}"))
    });
    Ok(CoreHandle {
        address,
        session_token,
        shutdown: shutdown_tx,
        task,
        state,
        model_manager,
        vad_runtime: handle_vad_runtime,
        vad_prepare_task,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn embedded_core_starts_and_stops() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        assert!(handle.address().port() > 0);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn deferred_vad_does_not_block_core_startup() {
        let directory = tempfile::tempdir().unwrap();
        let started = Instant::now();
        let handle = start_with_deferred_vad(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: None,
            asr_model_dir: None,
        })
        .await
        .unwrap();

        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "deferred startup should not wait for the managed VAD download"
        );
        assert_eq!(handle.vad_backend(), "energy");
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn settings_update_switches_the_live_model_directory() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let port = std::net::TcpListener::bind(("127.0.0.1", 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let handle = start(CoreOptions {
            config_path,
            host: Some("127.0.0.1".into()),
            port: Some(port),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let token = handle.session_token().to_owned();
        let tiny = handle
            .model_manager
            .list("small", "not_loaded")
            .into_iter()
            .find(|model| model.id == "tiny")
            .unwrap();
        let old_model_path = handle.model_manager.model_dir().join("ggml-tiny.bin");
        let model_file = std::fs::File::create(&old_model_path).unwrap();
        model_file.set_len(tiny.total_bytes).unwrap();
        asr::cache_model_verification_for_test(&old_model_path, "tiny");
        let client = reqwest::Client::new();
        let settings_url = format!("http://{}/api/settings", handle.address());
        let mut settings: serde_json::Value = client
            .get(&settings_url)
            .bearer_auth(&token)
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        settings["storage"]["model_directory"] = serde_json::json!("custom/models");

        let response = client
            .put(settings_url)
            .bearer_auth(&token)
            .json(&settings)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);
        assert_eq!(
            handle.model_manager.model_dir(),
            directory.path().join("custom").join("models")
        );
        assert!(handle.model_manager.model_dir().is_dir());
        assert!(!old_model_path.exists());
        assert!(handle
            .model_manager
            .model_dir()
            .join("ggml-tiny.bin")
            .is_file());
        assert!(handle.model_manager.is_downloaded("tiny").unwrap());

        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn websocket_accepts_query_token_without_authorization_header() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("test-token".into()),
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let response = reqwest::Client::new()
            .get(format!("http://{}/ws?token=test-token", handle.address()))
            .header(reqwest::header::CONNECTION, "Upgrade")
            .header(reqwest::header::UPGRADE, "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::SWITCHING_PROTOCOLS);
        drop(response);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn websocket_query_token_does_not_weaken_rest_authentication() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("test-token".into()),
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let base_url = format!("http://{}", handle.address());
        let health = client
            .get(format!("{base_url}/health"))
            .send()
            .await
            .unwrap();
        let websocket = client
            .get(format!("{base_url}/ws?token=wrong-token"))
            .header(reqwest::header::CONNECTION, "Upgrade")
            .header(reqwest::header::UPGRADE, "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();

        assert_eq!(health.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(websocket.status(), reqwest::StatusCode::UNAUTHORIZED);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn websocket_rejects_untrusted_browser_origin() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("test-token".into()),
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let response = reqwest::Client::new()
            .get(format!("http://{}/ws?token=test-token", handle.address()))
            .header(reqwest::header::ORIGIN, "https://example.com")
            .header(reqwest::header::CONNECTION, "Upgrade")
            .header(reqwest::header::UPGRADE, "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::FORBIDDEN);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dictionary_import_accepts_bodies_larger_than_axum_default() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let token = handle.session_token().to_owned();
        let response = reqwest::Client::new()
            .post(format!(
                "http://{}/api/dictionaries/import",
                handle.address()
            ))
            .bearer_auth(token)
            .body(vec![0; 3 * 1024 * 1024])
            .send()
            .await
            .unwrap();

        assert_eq!(response.status(), reqwest::StatusCode::UNPROCESSABLE_ENTITY);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn loopback_without_supplied_token_is_still_authenticated() {
        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let client = reqwest::Client::new();
        let url = format!("http://{}/health", handle.address());
        let unauthorized = client.get(&url).send().await.unwrap();
        let authorized = client
            .get(url)
            .bearer_auth(handle.session_token())
            .send()
            .await
            .unwrap();

        assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
        assert_eq!(authorized.status(), reqwest::StatusCode::OK);
        handle.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_closes_an_active_websocket() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let directory = tempfile::tempdir().unwrap();
        let handle = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            session_token: Some("test-token".into()),
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .unwrap();
        let mut stream = tokio::net::TcpStream::connect(handle.address())
            .await
            .unwrap();
        let request = format!(
            "GET /ws?token=test-token HTTP/1.1\r\nHost: {}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\r\n",
            handle.address()
        );
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut response = [0u8; 512];
        let read = stream.read(&mut response).await.unwrap();
        assert!(String::from_utf8_lossy(&response[..read]).contains("101 Switching Protocols"));

        tokio::time::timeout(std::time::Duration::from_secs(2), handle.shutdown())
            .await
            .expect("shutdown timed out")
            .unwrap();
    }

    #[tokio::test]
    async fn startup_rejects_invalid_persisted_configuration() {
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.json");
        let mut config = crate::config::AppConfig::default();
        config.storage.subtitle_history_limit = 0;
        std::fs::write(&config_path, serde_json::to_vec(&config).unwrap()).unwrap();

        let error = start(CoreOptions {
            config_path,
            host: None,
            port: Some(0),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .err()
        .expect("invalid config must fail");

        assert!(error.contains("subtitle_history_limit"));
    }

    #[tokio::test]
    async fn non_loopback_binding_requires_session_token() {
        let directory = tempfile::tempdir().unwrap();
        let error = start(CoreOptions {
            config_path: directory.path().join("config.json"),
            host: Some("0.0.0.0".into()),
            port: Some(0),
            session_token: None,
            vad_model_path: Some(directory.path().join("missing-silero.onnx")),
            asr_model_dir: None,
        })
        .await
        .err()
        .expect("unauthenticated external binding must fail");

        assert!(error.contains("VRCS_SESSION_TOKEN"));
    }
}
