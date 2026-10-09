use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OnceCell};

#[cfg(test)]
#[path = "qwen_runtime_tests.rs"]
mod tests;

#[derive(Clone, Debug)]
pub(crate) struct QwenConnection {
    pub(crate) base_url: String,
    pub(crate) token: String,
    pub(crate) model: String,
    pub(crate) generation: u64,
    pub(crate) session: u64,
}

#[derive(Clone, PartialEq, Eq)]
struct Selection {
    executable: PathBuf,
    model: PathBuf,
    mmproj: PathBuf,
    alias: String,
    device: String,
}

#[derive(Serialize)]
pub(crate) struct RuntimeSnapshot {
    pub(crate) status: &'static str,
    pub(crate) error: Option<String>,
    device: Option<&'static str>,
    fallback: Option<String>,
}

struct RuntimeState {
    child: Option<Child>,
    selection: Option<Selection>,
    connection: Option<QwenConnection>,
    generation: u64,
    session: u64,
    status: &'static str,
    error: Option<String>,
    device: Option<&'static str>,
    fallback: Option<String>,
}
impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            child: None,
            selection: None,
            connection: None,
            generation: 0,
            session: 0,
            status: "not_loaded",
            error: None,
            device: None,
            fallback: None,
        }
    }
}

pub(crate) struct QwenRuntime {
    pub(super) directory: PathBuf,
    state: Mutex<RuntimeState>,
    startup: Mutex<()>,
    devices: OnceCell<Vec<String>>,
    http: reqwest::Client,
}
impl QwenRuntime {
    pub(crate) fn new(directory: PathBuf) -> Self {
        Self {
            directory: directory.join(super::qwen_runtime_download::VERSION),
            state: Mutex::new(RuntimeState::default()),
            startup: Mutex::new(()),
            devices: OnceCell::new(),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(2))
                .build()
                .expect("static local HTTP client options are valid"),
        }
    }

    pub(crate) async fn devices(&self) -> Vec<String> {
        if !cfg!(feature = "vulkan") {
            return Vec::new();
        }
        // A missing runtime must not cache an empty result before its download.
        let Ok(executable) = self.executable_path().and_then(|path| checked_file(&path)) else {
            return Vec::new();
        };
        self.devices
            .get_or_init(|| async {
                let mut command = runtime_command(&executable);
                command.arg("--list-devices").kill_on_drop(true);
                let Ok(Ok(output)) =
                    tokio::time::timeout(Duration::from_secs(5), command.output()).await
                else {
                    return Vec::new();
                };
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .filter(|line| line.trim_start().starts_with("Vulkan"))
                    .map(|line| line.trim().to_owned())
                    .collect()
            })
            .await
            .clone()
    }

    pub(crate) fn executable_path(&self) -> Result<PathBuf, String> {
        if std::env::var_os("VRCS_LLAMA_SERVER").is_none()
            && super::qwen_runtime_download::installed(&self.directory)
        {
            return Ok(self.directory.join("llama-server.exe"));
        }
        executable_path()
    }

    pub(crate) async fn ensure_started(
        &self,
        executable: &Path,
        model: &Path,
        mmproj: &Path,
        alias: &str,
        device: &str,
        timeout: Duration,
    ) -> Result<QwenConnection, String> {
        let selection = Selection {
            executable: checked_file(executable)?,
            model: checked_file(model)?,
            mmproj: checked_file(mmproj)?,
            alias: alias.to_owned(),
            device: device.to_owned(),
        };
        if alias.trim().is_empty() {
            return Err("A Qwen ASR model alias is required".into());
        }
        if !["auto", "cpu", "gpu"].contains(&device) {
            return Err("Unsupported Qwen ASR device".into());
        }
        let _startup = self.startup.lock().await;
        let session = {
            let mut state = self.state.lock().await;
            check_child(&mut state)?;
            let same_selection = state.selection.as_ref() == Some(&selection);
            if same_selection {
                if let Some(connection) = &state.connection {
                    return Ok(connection.clone());
                }
            }
            stop_child(&mut state).await?;
            if !same_selection {
                state.session = state.session.wrapping_add(1);
            }
            state.selection = Some(selection.clone());
            state.status = "loading";
            state.error = None;
            state.fallback = None;
            state.session
        };
        self.start_selection(selection, session, timeout).await
    }

    // Stopped or replaced captures must not be resurrected by old workers.
    pub(crate) async fn reconnect(
        &self,
        previous: &QwenConnection,
    ) -> Result<QwenConnection, String> {
        let _startup = self.startup.lock().await;
        let selection = {
            let mut state = self.state.lock().await;
            check_child(&mut state)?;
            if state.session != previous.session {
                return Err("Managed Qwen ASR session was stopped or replaced".into());
            }
            if let Some(connection) = &state.connection {
                return Ok(connection.clone());
            }
            let selection = state
                .selection
                .clone()
                .ok_or("Managed Qwen ASR session was stopped")?;
            state.status = "loading";
            state.error = None;
            selection
        };
        self.start_selection(selection, previous.session, Duration::from_secs(120))
            .await
    }

    async fn start_selection(
        &self,
        selection: Selection,
        session: u64,
        timeout: Duration,
    ) -> Result<QwenConnection, String> {
        let gpu_available = !self.devices().await.is_empty();
        let device = if selection.device == "cpu" || (selection.device == "auto" && !gpu_available)
        {
            "cpu"
        } else {
            "gpu"
        };
        let deadline = Instant::now() + timeout;
        let first_deadline = if selection.device == "auto" && device == "gpu" {
            Instant::now() + timeout / 2
        } else {
            deadline
        };
        let result = if device == "gpu" && !gpu_available {
            Err(if cfg!(feature = "vulkan") {
                "No Vulkan GPU is available for Qwen ASR".into()
            } else {
                "This build does not include the Vulkan backend for Qwen ASR".into()
            })
        } else {
            self.launch(&selection, session, device, first_deadline)
                .await
        };
        let result = match result {
            Err(error)
                if selection.device == "auto" && device == "gpu" && Instant::now() < deadline =>
            {
                let mut state = self.state.lock().await;
                if state.session != session {
                    return Err(error);
                }
                stop_child(&mut state).await?;
                state.fallback = Some(error);
                drop(state);
                self.launch(&selection, session, "cpu", deadline).await
            }
            result => result,
        };
        if let Err(error) = &result {
            let mut state = self.state.lock().await;
            if state.session == session {
                stop_child(&mut state).await?;
                state.status = "error";
                state.error = Some(error.clone());
            }
        }
        result
    }

    async fn launch(
        &self,
        selection: &Selection,
        session: u64,
        device: &'static str,
        deadline: Instant,
    ) -> Result<QwenConnection, String> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("Could not reserve a Qwen ASR port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| error.to_string())?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let mut command = runtime_command(&selection.executable);
        command
            .args(launch_args(
                &selection.model,
                &selection.mmproj,
                port,
                &selection.alias,
                &token,
                device,
            ))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let diagnostics = Arc::new(StdMutex::new(VecDeque::<String>::new()));
        let generation = {
            let mut state = self.state.lock().await;
            if state.session != session {
                return Err("Qwen ASR startup was cancelled".into());
            }
            drop(listener);
            let mut child = command
                .spawn()
                .map_err(|error| format!("Could not start Qwen ASR runtime: {error}"))?;
            let stderr = child.stderr.take().expect("piped stderr");
            let tail = Arc::clone(&diagnostics);
            let redacted = token.clone();
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let line = line
                        .replace(&redacted, "[redacted]")
                        .chars()
                        .take(512)
                        .collect();
                    let mut tail = tail.lock().expect("runtime diagnostics lock");
                    if tail.len() == 12 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
            state.generation = state.generation.wrapping_add(1);
            state.child = Some(child);
            state.device = Some(device);
            state.generation
        };
        let origin = format!("http://127.0.0.1:{port}");
        loop {
            {
                let mut state = self.state.lock().await;
                if state.session != session {
                    return Err("Qwen ASR startup was cancelled".into());
                }
                check_child(&mut state)?;
                if state.child.is_none() {
                    return Err(with_diagnostics(
                        state
                            .error
                            .clone()
                            .unwrap_or_else(|| "Qwen ASR runtime exited during startup".into()),
                        &diagnostics,
                    ));
                }
            }
            match probe_ready(&self.http, &origin, &token, &selection.alias).await {
                Ok(true) => {
                    let mut state = self.state.lock().await;
                    if state.session != session {
                        return Err("Qwen ASR startup was cancelled".into());
                    }
                    let connection = QwenConnection {
                        base_url: format!("{origin}/v1"),
                        token,
                        model: selection.alias.clone(),
                        generation,
                        session,
                    };
                    state.status = "ready";
                    state.error = None;
                    state.connection = Some(connection.clone());
                    return Ok(connection);
                }
                Ok(false) => {}
                Err(error) => return Err(with_diagnostics(error, &diagnostics)),
            }
            if Instant::now() >= deadline {
                return Err(with_diagnostics(
                    "Qwen ASR runtime did not become ready in time".into(),
                    &diagnostics,
                ));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    pub(crate) async fn connection(&self) -> Result<Option<QwenConnection>, String> {
        let mut state = self.state.lock().await;
        check_child(&mut state)?;
        Ok(state.connection.clone())
    }
    pub(crate) async fn snapshot(&self) -> RuntimeSnapshot {
        let mut state = self.state.lock().await;
        if let Err(error) = check_child(&mut state) {
            state.status = "error";
            state.error = Some(error);
        }
        RuntimeSnapshot {
            status: state.status,
            error: state.error.clone(),
            device: state.device,
            fallback: state.fallback.clone(),
        }
    }
    pub(crate) async fn uses_package(&self, package: &str) -> Result<bool, String> {
        let mut state = self.state.lock().await;
        check_child(&mut state)?;
        Ok(state
            .selection
            .as_ref()
            .is_some_and(|selection| selection.alias == package))
    }
    pub(crate) async fn cancel_loading(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if state.status == "loading" {
            stop_session(&mut state).await?;
        }
        Ok(())
    }
    pub(crate) async fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        stop_session(&mut state).await
    }
}

pub(crate) fn executable_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("VRCS_LLAMA_SERVER") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()
        .map_err(|error| format!("Could not locate the Core executable: {error}"))?;
    let parent = current
        .parent()
        .ok_or("Core executable has no parent directory")?;
    let name = if cfg!(windows) {
        "llama-server.exe"
    } else {
        "llama-server"
    };
    let bundled = parent.join("qwen-runtime").join(name);
    if bundled.is_file() {
        return Ok(bundled);
    }
    let adjacent = parent.join(name);
    if adjacent.is_file() {
        return Ok(adjacent);
    }
    #[cfg(debug_assertions)]
    {
        let staged = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../apps/desktop/src-tauri/resources/qwen-runtime")
            .join(name);
        if staged.is_file() {
            return Ok(staged);
        }
    }
    Ok(bundled)
}
pub(super) fn runtime_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x0800_0000);
        // The installer keeps its Vulkan loader beside the desktop executable.
        if let Ok(current) = std::env::current_exe() {
            if let Some(parent) = current.parent() {
                let mut paths = vec![parent.to_path_buf()];
                if let Some(path) = std::env::var_os("PATH") {
                    paths.extend(std::env::split_paths(&path));
                }
                if let Ok(path) = std::env::join_paths(paths) {
                    command.env("PATH", path);
                }
            }
        }
    }
    command
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
fn check_child(state: &mut RuntimeState) -> Result<(), String> {
    if let Some(child) = &mut state.child {
        if let Some(exit) = child.try_wait().map_err(|error| error.to_string())? {
            state.child = None;
            state.connection = None;
            state.status = "error";
            state.error = Some(format!("Qwen ASR runtime exited: {exit}"));
        }
    }
    Ok(())
}
async fn stop_child(state: &mut RuntimeState) -> Result<(), String> {
    state.connection = None;
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
async fn stop_session(state: &mut RuntimeState) -> Result<(), String> {
    state.session = state.session.wrapping_add(1);
    state.selection = None;
    state.status = "not_loaded";
    state.device = None;
    state.fallback = None;
    stop_child(state).await
}
fn with_diagnostics(error: String, tail: &StdMutex<VecDeque<String>>) -> String {
    let lines = tail
        .lock()
        .expect("runtime diagnostics lock")
        .iter()
        .cloned()
        .collect::<Vec<_>>()
        .join("\n");
    if lines.is_empty() {
        error
    } else {
        format!("{error}\n{lines}")
    }
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
        "--ctx-size".into(),
        "16384".into(),
        "--parallel".into(),
        "2".into(),
    ];
    if device == "cpu" || !cfg!(feature = "vulkan") {
        args.extend([
            "--device".into(),
            "none".into(),
            "--gpu-layers".into(),
            "0".into(),
            "--no-mmproj-offload".into(),
            "--no-op-offload".into(),
        ]);
    } else {
        args.extend(["--gpu-layers".into(), "all".into()]);
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
