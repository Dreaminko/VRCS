use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore};

use super::plamo::{self, CompletionRequest, MODEL_ID};
use super::{LlmProgress, TranslationError};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
const MAX_WAITING_REQUESTS: usize = 8;

struct WaitingRequest<'a>(&'a AtomicUsize);

impl Drop for WaitingRequest<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Clone)]
struct Connection {
    origin: String,
    token: String,
    generation: u64,
}

struct RuntimeState {
    child: Option<Child>,
    model: Option<PathBuf>,
    connection: Option<Connection>,
    generation: u64,
    status: &'static str,
    error: Option<String>,
}

impl Default for RuntimeState {
    fn default() -> Self {
        Self {
            child: None,
            model: None,
            connection: None,
            generation: 0,
            status: "not_loaded",
            error: None,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct PlamoRuntimeStatus {
    pub(crate) status: &'static str,
    pub(crate) device: &'static str,
    pub(crate) model: &'static str,
    pub(crate) runtime_available: bool,
    pub(crate) error: Option<String>,
}

pub(crate) struct PlamoRuntime {
    runtime_directory: PathBuf,
    state: Mutex<RuntimeState>,
    startup: Mutex<()>,
    slots: Arc<Semaphore>,
    waiting: AtomicUsize,
    http: reqwest::Client,
}

impl PlamoRuntime {
    pub(crate) fn new(runtime_directory: PathBuf) -> Self {
        Self {
            runtime_directory,
            state: Mutex::new(RuntimeState::default()),
            startup: Mutex::new(()),
            slots: Arc::new(Semaphore::new(2)),
            waiting: AtomicUsize::new(0),
            http: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("static local PLaMo HTTP options are valid"),
        }
    }

    pub(crate) async fn ensure_started(&self, model: &Path) -> Result<(), TranslationError> {
        ensure_supported_platform()?;
        let model = checked_model(model)?;
        let _startup = self.startup.lock().await;
        let generation = {
            let mut state = self.state.lock().await;
            check_child(&mut state)?;
            if state.model.as_ref() == Some(&model) && state.connection.is_some() {
                return Ok(());
            }
            stop_child(&mut state).await.map_err(|detail| {
                local_error("translation.local_runtime_unavailable", detail, true)
            })?;
            state.model = Some(model.clone());
            state.status = "loading";
            state.error = None;
            state.generation = state.generation.wrapping_add(1);
            state.generation
        };
        let executable = match crate::asr::llama_server_executable_path(&self.runtime_directory) {
            Ok(path) => path,
            Err(detail) => {
                let error = local_error("translation.local_runtime_unavailable", detail, false);
                self.fail_startup(generation, &error.detail).await?;
                return Err(error);
            }
        };
        let result = self.launch(&executable, &model, generation).await;
        if let Err(error) = &result {
            self.fail_startup(generation, &error.detail).await?;
        }
        result
    }

    pub(crate) async fn translate(
        &self,
        request: CompletionRequest<'_>,
        on_progress: Option<&LlmProgress>,
    ) -> Result<String, TranslationError> {
        let connection = {
            let mut state = self.state.lock().await;
            check_child(&mut state)?;
            state.connection.clone().ok_or_else(|| {
                local_error(
                    "translation.local_runtime_unavailable",
                    "The local PLaMo runtime is not loaded",
                    true,
                )
            })?
        };
        let started = Instant::now();
        let _permit = self.acquire_slot(REQUEST_TIMEOUT).await?;
        let remaining = REQUEST_TIMEOUT.saturating_sub(started.elapsed());
        let result = tokio::time::timeout(remaining, {
            plamo::translate(
                &self.http,
                &connection.origin,
                &connection.token,
                request,
                on_progress,
            )
        })
        .await;
        match result {
            Ok(result) => {
                let mut state = self.state.lock().await;
                check_child(&mut state)?;
                if state.generation != connection.generation {
                    return Err(local_error(
                        "translation.local_runtime_unavailable",
                        "The local PLaMo runtime changed while translating",
                        true,
                    ));
                }
                result
            }
            Err(_) => Err(local_error(
                "translation.timeout",
                "Local PLaMo translation timed out",
                true,
            )),
        }
    }

    async fn acquire_slot(
        &self,
        timeout: Duration,
    ) -> Result<OwnedSemaphorePermit, TranslationError> {
        let waiting = self.waiting.fetch_add(1, Ordering::AcqRel);
        let _waiting = WaitingRequest(&self.waiting);
        if waiting >= MAX_WAITING_REQUESTS && self.slots.available_permits() == 0 {
            return Err(local_error(
                "translation.local_queue_full",
                "The local PLaMo translation queue is full",
                true,
            ));
        }
        let permit = tokio::time::timeout(timeout, Arc::clone(&self.slots).acquire_owned()).await;
        match permit {
            Ok(Ok(permit)) => Ok(permit),
            Ok(Err(_)) => Err(local_error(
                "translation.local_runtime_unavailable",
                "The local PLaMo request queue is closed",
                true,
            )),
            Err(_) => Err(local_error(
                "translation.timeout",
                "Local PLaMo translation timed out while waiting",
                true,
            )),
        }
    }

    pub(crate) async fn snapshot(&self) -> PlamoRuntimeStatus {
        let mut state = self.state.lock().await;
        if !crate::platform_capabilities::local_translation_supported() {
            state.status = "error";
            state.error = Some("Local PLaMo CPU translation requires Windows x64 or Linux GNU x86_64; ARM64 is not yet verified".to_owned());
        }
        if let Err(error) = check_child(&mut state) {
            state.status = "error";
            state.error = Some(error.detail);
        }
        PlamoRuntimeStatus {
            status: state.status,
            device: "cpu",
            model: MODEL_ID,
            runtime_available: crate::platform_capabilities::local_translation_supported()
                && crate::asr::llama_server_executable_path(&self.runtime_directory)
                    .is_ok_and(|path| path.is_file()),
            error: state.error.clone(),
        }
    }

    pub(crate) async fn stop(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        state.model = None;
        state.status = "not_loaded";
        state.error = None;
        state.generation = state.generation.wrapping_add(1);
        stop_child(&mut state).await
    }

    async fn launch(
        &self,
        executable: &Path,
        model: &Path,
        generation: u64,
    ) -> Result<(), TranslationError> {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|error| {
            local_error(
                "translation.local_runtime_unavailable",
                format!("Could not reserve a local PLaMo port: {error}"),
                true,
            )
        })?;
        let port = listener
            .local_addr()
            .map_err(|error| {
                local_error(
                    "translation.local_runtime_unavailable",
                    error.to_string(),
                    true,
                )
            })?
            .port();
        let token = uuid::Uuid::new_v4().simple().to_string();
        let threads = std::thread::available_parallelism()
            .map(|count| (count.get() / 2).clamp(1, 4))
            .unwrap_or(1);
        let mut command = runtime_command(executable);
        command
            .args(launch_args(model, port, &token, threads))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let diagnostics = Arc::new(StdMutex::new(VecDeque::<String>::new()));
        {
            let state = self.state.lock().await;
            if state.generation != generation {
                return Err(local_error(
                    "translation.local_runtime_unavailable",
                    "Local PLaMo startup was cancelled",
                    false,
                ));
            }
        }
        drop(listener);
        let mut child = command.spawn().map_err(|error| {
            local_error(
                "translation.local_runtime_unavailable",
                format!("Could not start the local PLaMo runtime: {error}"),
                true,
            )
        })?;
        {
            let mut state = self.state.lock().await;
            if state.generation != generation {
                drop(state);
                let _ = child.kill().await;
                return Err(local_error(
                    "translation.local_runtime_unavailable",
                    "Local PLaMo startup was cancelled",
                    false,
                ));
            }
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
                    let mut tail = tail.lock().expect("PLaMo runtime diagnostics lock");
                    if tail.len() == 12 {
                        tail.pop_front();
                    }
                    tail.push_back(line);
                }
            });
            state.child = Some(child);
        }
        let origin = format!("http://127.0.0.1:{port}");
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            {
                let mut state = self.state.lock().await;
                check_child(&mut state)?;
                if state.generation != generation {
                    return Err(local_error(
                        "translation.local_runtime_unavailable",
                        "Local PLaMo startup was cancelled",
                        false,
                    ));
                }
                if state.child.is_none() {
                    let error = with_diagnostics(
                        state
                            .error
                            .clone()
                            .unwrap_or_else(|| "Local PLaMo runtime exited during startup".into()),
                        &diagnostics,
                    );
                    return Err(local_error(
                        "translation.local_runtime_unavailable",
                        error,
                        true,
                    ));
                }
            }
            match probe_ready(&self.http, &origin, &token).await {
                Ok(true) => {
                    let mut state = self.state.lock().await;
                    if state.generation != generation {
                        return Err(local_error(
                            "translation.local_runtime_unavailable",
                            "Local PLaMo startup was cancelled",
                            false,
                        ));
                    }
                    state.connection = Some(Connection {
                        origin,
                        token,
                        generation,
                    });
                    state.status = "ready";
                    state.error = None;
                    return Ok(());
                }
                Ok(false) => {}
                Err(error) => {
                    let error = with_diagnostics(error, &diagnostics);
                    return Err(local_error(
                        "translation.local_runtime_unavailable",
                        error,
                        true,
                    ));
                }
            }
            if Instant::now() >= deadline {
                let error = with_diagnostics(
                    "Local PLaMo runtime did not become ready in time".into(),
                    &diagnostics,
                );
                return Err(local_error("translation.timeout", error, true));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    }

    async fn fail_startup(&self, generation: u64, error: &str) -> Result<(), TranslationError> {
        let mut state = self.state.lock().await;
        if state.generation == generation {
            state.status = "error";
            state.error = Some(error.to_owned());
            stop_child(&mut state).await.map_err(|detail| {
                local_error("translation.local_runtime_unavailable", detail, true)
            })?;
        }
        Ok(())
    }
}

fn local_error(code: &'static str, detail: impl Into<String>, retryable: bool) -> TranslationError {
    TranslationError {
        code,
        detail: detail.into(),
        retryable,
        retry_after: None,
    }
}

fn ensure_supported_platform() -> Result<(), TranslationError> {
    if crate::platform_capabilities::local_translation_supported() {
        Ok(())
    } else {
        Err(local_error(
            crate::platform_capabilities::LOCAL_TRANSLATION_UNAVAILABLE,
            "Local PLaMo CPU translation is unavailable on this platform; ARM64 inference is not yet verified",
            false,
        ))
    }
}

fn checked_model(path: &Path) -> Result<PathBuf, TranslationError> {
    let absolute = std::fs::canonicalize(path).map_err(|error| {
        local_error(
            "translation.local_model_missing",
            format!("The local PLaMo model is unavailable: {error}"),
            false,
        )
    })?;
    if !absolute.is_file() {
        return Err(local_error(
            "translation.local_model_missing",
            "The local PLaMo model path is not a file",
            false,
        ));
    }
    Ok(absolute)
}

fn check_child(state: &mut RuntimeState) -> Result<(), TranslationError> {
    if let Some(child) = &mut state.child {
        if let Some(exit) = child.try_wait().map_err(|error| {
            local_error(
                "translation.local_runtime_unavailable",
                error.to_string(),
                true,
            )
        })? {
            state.child = None;
            state.connection = None;
            state.status = "error";
            state.error = Some(format!("Local PLaMo runtime exited: {exit}"));
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
                .map_err(|error| format!("Could not stop the local PLaMo runtime: {error}"))?;
        }
    }
    Ok(())
}

fn runtime_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    #[cfg(target_os = "linux")]
    command.env_remove("LD_LIBRARY_PATH").env_remove("LD_PRELOAD");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.as_std_mut().creation_flags(0x0800_0000);
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

fn launch_args(model: &Path, port: u16, token: &str, threads: usize) -> Vec<OsString> {
    vec![
        "--model".into(),
        model.as_os_str().into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string().into(),
        "--alias".into(),
        MODEL_ID.into(),
        "--api-key".into(),
        token.into(),
        "--no-webui".into(),
        "--cors-origins".into(),
        "localhost".into(),
        "--ctx-size".into(),
        "8192".into(),
        "--parallel".into(),
        "2".into(),
        "--threads".into(),
        threads.to_string().into(),
        "--threads-batch".into(),
        threads.to_string().into(),
        "--device".into(),
        "none".into(),
        "--gpu-layers".into(),
        "0".into(),
        "--no-op-offload".into(),
    ]
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

async fn probe_ready(http: &reqwest::Client, origin: &str, token: &str) -> Result<bool, String> {
    let health = match http.get(format!("{origin}/health")).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(false),
        Err(error) => return Err(format!("Local PLaMo health check failed: {error}")),
    };
    if health.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return Ok(false);
    }
    if !health.status().is_success() {
        return Err(format!(
            "Local PLaMo health check returned {}",
            health.status()
        ));
    }
    let health = health
        .json::<Health>()
        .await
        .map_err(|error| format!("Invalid local PLaMo health response: {error}"))?;
    if health.status != "ok" {
        return Ok(false);
    }
    let models = http
        .get(format!("{origin}/v1/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("Local PLaMo model check failed: {error}"))?;
    if !models.status().is_success() {
        return Err(format!(
            "Local PLaMo model check returned {}",
            models.status()
        ));
    }
    let models = models
        .json::<Models>()
        .await
        .map_err(|error| format!("Invalid local PLaMo model response: {error}"))?;
    Ok(models.data.iter().any(|model| model.id == MODEL_ID))
}

fn with_diagnostics(error: String, tail: &StdMutex<VecDeque<String>>) -> String {
    let lines = tail
        .lock()
        .expect("PLaMo runtime diagnostics lock")
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translation::TranslationContextEntry;

    #[test]
    fn launch_arguments_isolate_plamo_from_the_asr_runtime() {
        let arguments = launch_args(Path::new("plamo.gguf"), 43210, "secret", 3)
            .iter()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        for pair in [
            ["--model", "plamo.gguf"],
            ["--host", "127.0.0.1"],
            ["--port", "43210"],
            ["--alias", MODEL_ID],
            ["--api-key", "secret"],
            ["--ctx-size", "8192"],
            ["--parallel", "2"],
            ["--device", "none"],
            ["--gpu-layers", "0"],
        ] {
            assert!(
                arguments.windows(2).any(|candidate| candidate == pair),
                "missing arguments {pair:?}"
            );
        }
        assert!(arguments.contains(&"--no-webui".into()));
        assert!(arguments.contains(&"--no-op-offload".into()));
        assert!(!arguments.iter().any(|argument| argument.contains("mmproj")));
    }

    #[cfg(all(target_arch = "x86_64", any(windows, all(target_os = "linux", target_env = "gnu"))))]
    #[tokio::test]
    async fn an_unchanged_ready_runtime_is_not_restarted() {
        let model = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"))
            .unwrap();
        let runtime = PlamoRuntime::new(PathBuf::new());
        {
            let mut state = runtime.state.lock().await;
            state.model = Some(model.clone());
            state.connection = Some(Connection {
                origin: "http://127.0.0.1:1".into(),
                token: "token".into(),
                generation: 7,
            });
            state.generation = 7;
            state.status = "ready";
        }

        runtime.ensure_started(&model).await.unwrap();

        let state = runtime.state.lock().await;
        assert_eq!(state.generation, 7);
        assert!(state.child.is_none());
    }

    #[tokio::test]
    async fn request_queue_rejects_excess_waiters_and_times_out() {
        let runtime = Arc::new(PlamoRuntime::new(PathBuf::new()));
        let first = runtime.acquire_slot(Duration::from_secs(1)).await.unwrap();
        let second = runtime.acquire_slot(Duration::from_secs(1)).await.unwrap();
        let mut waiters = Vec::new();
        for _ in 0..MAX_WAITING_REQUESTS {
            let runtime = Arc::clone(&runtime);
            waiters.push(tokio::spawn(async move {
                runtime.acquire_slot(Duration::from_secs(2)).await
            }));
        }
        tokio::time::timeout(Duration::from_secs(1), async {
            while runtime.waiting.load(Ordering::Acquire) < MAX_WAITING_REQUESTS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        let error = runtime
            .acquire_slot(Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(error.code, "translation.local_queue_full");
        for waiter in waiters {
            waiter.abort();
            let _ = waiter.await;
        }
        assert_eq!(runtime.waiting.load(Ordering::Acquire), 0);
        let error = runtime
            .acquire_slot(Duration::from_millis(10))
            .await
            .unwrap_err();
        assert_eq!(error.code, "translation.timeout");
        drop(first);
        drop(second);
    }

    #[tokio::test]
    async fn exited_children_invalidate_the_connection() {
        #[cfg(windows)]
        let mut command = {
            let mut command = Command::new("cmd.exe");
            command.args(["/C", "exit 7"]);
            command
        };
        #[cfg(not(windows))]
        let mut command = {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 7"]);
            command
        };
        let child = command.spawn().unwrap();
        let mut state = RuntimeState {
            child: Some(child),
            connection: Some(Connection {
                origin: String::new(),
                token: String::new(),
                generation: 3,
            }),
            status: "ready",
            generation: 3,
            ..RuntimeState::default()
        };

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                check_child(&mut state).unwrap();
                if state.child.is_none() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();

        assert_eq!(state.status, "error");
        assert!(state.connection.is_none());
        assert!(state
            .error
            .as_deref()
            .is_some_and(|error| error.contains('7')));
    }

    #[tokio::test]
    async fn unload_clears_runtime_state_without_the_startup_lock() {
        let runtime = PlamoRuntime::new(PathBuf::new());
        {
            let mut state = runtime.state.lock().await;
            state.model = Some(PathBuf::from("model.gguf"));
            state.status = "ready";
            state.generation = 4;
        }

        runtime.stop().await.unwrap();

        let state = runtime.state.lock().await;
        assert!(state.model.is_none());
        assert_eq!(state.status, "not_loaded");
        assert_eq!(state.generation, 5);
    }

    #[tokio::test]
    async fn stop_can_cancel_a_startup_that_still_holds_the_startup_lock() {
        let runtime = PlamoRuntime::new(PathBuf::new());
        let startup = runtime.startup.lock().await;

        tokio::time::timeout(Duration::from_millis(100), runtime.stop())
            .await
            .unwrap()
            .unwrap();

        drop(startup);
        let state = runtime.state.lock().await;
        assert_eq!(state.status, "not_loaded");
        assert_eq!(state.generation, 1);
    }

    #[tokio::test]
    async fn readiness_requires_the_local_token_and_model_alias() {
        use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode};
        use axum::{routing::get, Json, Router};
        use serde_json::json;

        let app = Router::new()
            .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
            .route(
                "/v1/models",
                get(|headers: HeaderMap| async move {
                    if headers
                        .get(AUTHORIZATION)
                        .is_none_or(|value| value != "Bearer secret")
                    {
                        return (StatusCode::UNAUTHORIZED, Json(json!({ "data": [] })));
                    }
                    (
                        StatusCode::OK,
                        Json(json!({ "data": [{ "id": MODEL_ID }] })),
                    )
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();

        assert!(probe_ready(&client, &origin, "secret").await.unwrap());
        assert!(probe_ready(&client, &origin, "wrong").await.is_err());
        server.abort();
    }

    #[tokio::test]
    #[ignore = "requires the pinned PLaMo GGUF and staged llama.cpp b11501 runtime"]
    async fn real_plamo_translates_and_restarts_on_cpu() {
        let smoke_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("plamo-smoke");
        let model = smoke_root
            .join("models")
            .join("asr")
            .join("translation")
            .join(MODEL_ID)
            .join(plamo::MODEL_REVISION)
            .join(plamo::MODEL_FILENAME);
        assert!(model.is_file(), "missing {}", model.display());
        let runtime = Arc::new(PlamoRuntime::new(smoke_root.join("runtimes").join("qwen")));
        runtime.ensure_started(&model).await.unwrap();

        for (text, source, target) in [
            ("Good morning.", "en", "ja"),
            ("今日は会議があります。", "ja", "en"),
            ("这个软件可以在本地翻译。", "zh-Hans", "zh-Hant"),
        ] {
            let translated = runtime
                .translate(
                    CompletionRequest {
                        text,
                        source: Some(source),
                        target,
                        context: &[],
                        glossary: &[],
                        max_chars: 4_000,
                    },
                    None,
                )
                .await
                .unwrap();
            eprintln!("{source}->{target}: {translated}");
            assert!(!translated.trim().is_empty());
            assert!(!translated.contains("<|plamo:"));
        }

        runtime.stop().await.unwrap();
        runtime.ensure_started(&model).await.unwrap();
        let translated = runtime
            .translate(
                CompletionRequest {
                    text: "See you tomorrow.",
                    source: Some("en"),
                    target: "ja",
                    context: &[],
                    glossary: &[],
                    max_chars: 4_000,
                },
                None,
            )
            .await
            .unwrap();
        assert!(!translated.trim().is_empty());

        let context = [TranslationContextEntry {
            source: "chatbox".into(),
            text: "The user is talking about a software project.".into(),
            created_at: String::new(),
        }];
        let glossary = [crate::config::GlossaryEntry {
            source: "project".into(),
            target: Some("プロジェクト".into()),
            category: crate::config::GlossaryCategory::Custom,
            case_sensitive: false,
        }];
        let translated = runtime
            .translate(
                CompletionRequest {
                    text: "Please open the project.",
                    source: Some("en"),
                    target: "ja",
                    context: &context,
                    glossary: &glossary,
                    max_chars: 4_000,
                },
                None,
            )
            .await
            .unwrap();
        eprintln!("context/glossary en->ja: {translated}");
        assert!(!translated.trim().is_empty());

        let interrupted = tokio::spawn({
            let runtime = Arc::clone(&runtime);
            async move {
                runtime
                    .translate(
                        CompletionRequest {
                            text: &"We need to finish this project before tomorrow. ".repeat(40),
                            source: Some("en"),
                            target: "ja",
                            context: &[],
                            glossary: &[],
                            max_chars: 4_000,
                        },
                        None,
                    )
                    .await
            }
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        interrupted.abort();
        let _ = interrupted.await;
        runtime.stop().await.unwrap();
    }
}
