//! Shared supervision for managed local llama-server processes.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use serde::Deserialize;
use tokio::process::{Child, Command};

pub(crate) fn runtime_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if let Some(parent) = executable.parent() {
        command.current_dir(parent);
    }
    // The fixed Linux ELF closure carries $ORIGIN RUNPATH. Do not inherit a
    // user's shell library overrides into a managed inference subprocess.
    #[cfg(target_os = "linux")]
    command
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD");
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

pub(crate) async fn probe_ready(
    http: &reqwest::Client,
    origin: &str,
    token: &str,
    alias: &str,
    label: &str,
) -> Result<bool, String> {
    let health = match http.get(format!("{origin}/health")).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(false),
        Err(error) => return Err(format!("{label} health check failed: {error}")),
    };
    if health.status() == reqwest::StatusCode::SERVICE_UNAVAILABLE {
        return Ok(false);
    }
    if !health.status().is_success() {
        return Err(format!("{label} health check returned {}", health.status()));
    }
    let health: Health = health
        .json()
        .await
        .map_err(|error| format!("Invalid {label} health response: {error}"))?;
    if health.status != "ok" {
        return Ok(false);
    }
    let models = http
        .get(format!("{origin}/v1/models"))
        .bearer_auth(token)
        .send()
        .await
        .map_err(|error| format!("{label} model check failed: {error}"))?;
    if !models.status().is_success() {
        return Err(format!("{label} model check returned {}", models.status()));
    }
    let models: Models = models
        .json()
        .await
        .map_err(|error| format!("Invalid {label} model response: {error}"))?;
    Ok(models.data.iter().any(|model| model.id == alias))
}

/// Stop a managed child without turning a synchronous wait into an async race.
pub(crate) async fn stop_child(child: &mut Option<Child>, label: &str) -> Result<(), String> {
    if let Some(mut child) = child.take() {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_none()
        {
            child
                .kill()
                .await
                .map_err(|error| format!("Could not stop {label} runtime: {error}"))?;
        }
    }
    Ok(())
}

pub(crate) fn with_diagnostics(error: String, tail: &Mutex<VecDeque<String>>) -> String {
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
