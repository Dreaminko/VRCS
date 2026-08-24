//! VRCS Rust Core，可由独立二进制或 Tauri 主进程启动。

mod anki;
mod asr;
mod audio;
mod chatbox;
mod config;
mod credentials;
mod db;
mod domain_events;
mod error;
mod external_api;
mod glossary;
mod language_session;
mod learning;
mod llm;
mod microphone_monitor;
mod models;
mod osc;
mod pipeline;
mod providers;
mod server;
mod smart_turn;
mod startup;
mod subtitle_output;
mod translation;
mod vad;
mod vrchat_mute_sync;
mod vrcx;
mod yomitan;

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::server::AppState;
use crate::startup::{RuntimeAssembly, RuntimeTasks, StartupPlan};

pub use crate::config::{VrOverlayConfig, VrOverlayHeadsetConfig, VrOverlayWristConfig};
pub use crate::models::{Subtitle, SubtitleTranslation};
pub use crate::subtitle_output::PresentationEvent;
pub use crate::translation::same_translation_language;

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
    runtime_tasks: RuntimeTasks,
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

    pub fn external_api_address(&self) -> Option<SocketAddr> {
        self.state
            .integrations
            .external_api_status
            .read()
            .expect("External API status lock")
            .address
            .as_deref()
            .and_then(|address| address.parse().ok())
    }

    pub fn subscribe_presentation_events(&self) -> broadcast::Receiver<PresentationEvent> {
        self.state
            .content
            .subtitle_output
            .subscribe_presentation_events()
    }

    pub fn subscribe_vr_overlay_config(&self) -> watch::Receiver<VrOverlayConfig> {
        self.state.integrations.vr_overlay_config_tx.subscribe()
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
        let result = self.runtime_tasks.wait_server().await;
        let _ = self.shutdown.send(true);
        self.runtime_tasks.stop_auxiliaries().await;
        if let Some(server) = self
            .state
            .integrations
            .external_api_server
            .lock()
            .await
            .take()
        {
            server.stop().await;
        }
        let _control = self.state.capture.capture_control.lock().await;
        self.state
            .capture
            .speaker_pipeline
            .lock()
            .await
            .stop()
            .await;
        self.state
            .capture
            .microphone_pipeline
            .lock()
            .await
            .stop()
            .await;
        self.state
            .capture
            .microphone_monitor
            .lock()
            .await
            .stop()
            .await;
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
        .unwrap_or_else(|_| "vrcs_core=info,vrcs_desktop_lib=info,tower_http=info".into());
    if let Some(log_dir) = log_dir {
        std::fs::create_dir_all(log_dir).map_err(|error| {
            format!(
                "Failed to create log directory {}: {error}",
                log_dir.display()
            )
        })?;
        let appender = tracing_appender::rolling::RollingFileAppender::builder()
            .rotation(tracing_appender::rolling::Rotation::DAILY)
            .filename_prefix("errorlog")
            .filename_suffix("log")
            .max_log_files(7)
            .build(log_dir)
            .map_err(|error| format!("Failed to create log file: {error}"))?;
        let (file_writer, guard) = tracing_appender::non_blocking(appender);
        tracing_subscriber::fmt()
            .with_ansi(false)
            .with_env_filter(filter)
            .with_writer(file_writer.and(std::io::stderr))
            .try_init()
            .map_err(|error| format!("Failed to initialize logging: {error}"))?;
        Ok(LoggingGuard {
            _guard: Some(guard),
        })
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .try_init()
            .map_err(|error| format!("Failed to initialize logging: {error}"))?;
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
    let plan = StartupPlan::resolve(options, defer_managed_vad)?;
    let RuntimeAssembly {
        requested_address,
        session_token,
        shutdown_tx,
        shutdown_rx,
        state,
        model_manager,
        vad_runtime,
        mut vad_prepare_task,
    } = RuntimeAssembly::build(plan).await?;

    let runtime_tasks =
        match RuntimeTasks::spawn(requested_address, Arc::clone(&state), shutdown_rx).await {
            Ok(tasks) => tasks,
            Err(error) => {
                let _ = shutdown_tx.send(true);
                if let Some(task) = vad_prepare_task.take() {
                    task.abort();
                    let _ = task.await;
                }
                if let Some(server) = state.integrations.external_api_server.lock().await.take() {
                    server.stop().await;
                }
                model_manager.cancel_all_and_wait().await;
                return Err(error);
            }
        };
    let address = runtime_tasks.address();
    tracing::info!(
        elapsed_ms = startup_started.elapsed().as_millis(),
        deferred_vad = defer_managed_vad,
        "vrcs-core startup ready"
    );

    Ok(CoreHandle {
        address,
        session_token,
        shutdown: shutdown_tx,
        runtime_tasks,
        state,
        model_manager,
        vad_runtime,
        vad_prepare_task,
    })
}

#[cfg(test)]
mod tests;
