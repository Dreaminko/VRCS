use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Clone, Debug)]
pub(crate) struct QwenConnection {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) model: String,
    pub(crate) generation: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct Selection {
    executable: PathBuf,
    model: PathBuf,
    mmproj: PathBuf,
    alias: String,
    device: String,
}

#[derive(Default)]
struct RuntimeState {
    child: Option<Child>,
    selection: Option<Selection>,
    connection: Option<QwenConnection>,
    generation: u64,
}

pub(crate) struct QwenRuntime {
    state: Mutex<RuntimeState>,
    http: reqwest::Client,
}

impl QwenRuntime {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(RuntimeState::default()),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .expect("static local HTTP client options are valid"),
        }
    }

    pub(crate) async fn ensure_started(
        &self,
        executable: &Path,
        model: &Path,
        mmproj: &Path,
        alias: &str,
        device: &str,
        startup_timeout: Duration,
    ) -> Result<QwenConnection, String> {
        let selection = Selection {
            executable: checked_file(executable)?,
            model: checked_file(model)?,
            mmproj: checked_file(mmproj)?,
            alias: alias.to_owned(),
            device: device.to_owned(),
        };
        if selection.alias.trim().is_empty() {
            return Err("A Qwen ASR model alias is required".into());
        }
        if !["auto", "cpu"].contains(&selection.device.as_str()) {
            return Err("Unsupported Qwen ASR device".into());
        }
        let mut state = self.state.lock().await;
        if state.selection.as_ref() == Some(&selection) {
            let connection = state.connection.clone();
            if let (Some(child), Some(connection)) = (&mut state.child, connection) {
                if child
                    .try_wait()
                    .map_err(|error| error.to_string())?
                    .is_none()
                {
                    return Ok(connection);
                }
            }
        }
        stop_child(&mut state).await?;

        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("Could not reserve a Qwen ASR port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut command = Command::new(&selection.executable);
        command
            .args(launch_args(
                &selection.model,
                &selection.mmproj,
                port,
                &selection.alias,
                &token,
                &selection.device,
            ))
            .current_dir(
                selection
                    .executable
                    .parent()
                    .expect("canonical file has parent"),
            )
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.as_std_mut().creation_flags(0x0800_0000);
        }
        drop(listener);
        let child = command
            .spawn()
            .map_err(|error| format!("Could not start Qwen ASR runtime: {error}"))?;
        state.generation = state.generation.wrapping_add(1);
        state.child = Some(child);
        state.selection = Some(selection.clone());
        let origin = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + startup_timeout;
        loop {
            if let Some(exit) = state
                .child
                .as_mut()
                .expect("started child exists")
                .try_wait()
                .map_err(|error| error.to_string())?
            {
                stop_child(&mut state).await?;
                return Err(format!("Qwen ASR runtime exited during startup: {exit}"));
            }
            match probe_ready(&self.http, &origin, &token, &selection.alias).await {
                Ok(true) => {
                    let connection = QwenConnection {
                        base_url: format!("{origin}/v1"),
                        token,
                        model: selection.alias,
                        generation: state.generation,
                    };
                    state.connection = Some(connection.clone());
                    return Ok(connection);
                }
                Ok(false) => {}
                Err(error) => {
                    stop_child(&mut state).await?;
                    return Err(error);
                }
            }
            if Instant::now() >= deadline {
                stop_child(&mut state).await?;
                return Err("Qwen ASR runtime did not become ready in time".into());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub(crate) async fn connection(&self) -> Result<Option<QwenConnection>, String> {
        let mut state = self.state.lock().await;
        if let Some(child) = state.child.as_mut() {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                stop_child(&mut state).await?;
            }
        }
        Ok(state.connection.clone())
    }

    pub(crate) async fn uses_package(&self, package: &str) -> Result<bool, String> {
        let mut state = self.state.lock().await;
        if let Some(child) = state.child.as_mut() {
            if child
                .try_wait()
                .map_err(|error| error.to_string())?
                .is_some()
            {
                stop_child(&mut state).await?;
            }
        }
        Ok(state
            .selection
            .as_ref()
            .is_some_and(|selection| selection.alias == package))
    }

    pub(crate) async fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        stop_child(&mut state).await
    }
}

pub(crate) fn executable_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VRCS_LLAMA_SERVER") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Core executable: {error}"))?;
    let name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    Ok(current
        .parent()
        .ok_or_else(|| "Core executable has no parent directory".to_string())?
        .join(name))
}

fn checked_file(path: &Path) -> Result<PathBuf, String> {
    let absolute = std::fs::canonicalize(path).map_err(|error| {
        format!(
            "Qwen ASR runtime file {} is unavailable: {error}",
            path.display()
        )
    })?;
    if !absolute.is_file() {
        return Err(format!(
            "Qwen ASR runtime path is not a file: {}",
            path.display()
        ));
    }
    Ok(absolute)
}

async fn stop_child(state: &mut RuntimeState) -> Result<(), String> {
    state.connection = None;
    state.selection = None;
    if let Some(mut child) = state.child.take() {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            child
                .kill()
                .await
                .map_err(|error| format!("Could not stop Qwen ASR runtime: {error}"))?;
        }
    }
    Ok(())
}

pub(super) fn launch_args(
    model: &Path,
    mmproj: &Path,
    port: u16,
    alias: &str,
    token: &str,
    device: &str,
) -> Vec<OsString> {
    let mut args = vec![
        "--model".into(),
        model.as_os_str().into(),
        "--mmproj".into(),
        mmproj.as_os_str().into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string().into(),
        "--alias".into(),
        alias.into(),
        "--api-key".into(),
        token.into(),
        "--no-webui".into(),
        "--cors-origins".into(),
        "localhost".into(),
    ];
    if device == "cpu" {
        args.extend(["--gpu-layers".into(), "0".into()]);
    }
    args
}

#[derive(Deserialize)]
struct Health {
    status: String,
}

#[derive(Deserialize)]
struct Models {
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

pub(super) async fn probe_ready(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    alias: &str,
) -> Result<bool, String> {
    let health = match http.get(format!("{origin}/health")).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(false),
        Err(error) => return Err(format!("Qwen ASR health check failed: {error}")),
    };
    if health.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return Ok(false);
    }
    if !health.status().is_success() {
        return Err(format!(
            "Qwen ASR health check returned {}",
            health.status()
        ));
    }
    let health: Health = health
        .json()
        .await
        .map_err(|error| format!("Invalid Qwen ASR health response: {error}"))?;
    if health.status != "ok" {
        return Ok(false);
    }
    let models = http
        .get(format!("{origin}/v1/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("Qwen ASR model check failed: {error}"))?;
    if !models.status().is_success() {
        return Err(format!("Qwen ASR model check returned {}", models.status()));
    }
    let models: Models = models
        .json()
        .await
        .map_err(|error| format!("Invalid Qwen ASR model response: {error}"))?;
    Ok(models.data.iter().any(|model| model.id == alias))
}
