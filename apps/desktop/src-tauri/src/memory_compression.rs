//! Fixed, local Windows MMAgent operations. The main window owns consent;
//! this module never restarts Windows and never accepts shell text from the UI.
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

#[derive(Default)]
pub(crate) struct MemoryCompressionState(tokio::sync::Mutex<()>);

#[derive(Debug, Serialize)]
pub(crate) struct Status {
    supported: bool,
    elevated: bool,
    enabled: Option<bool>,
    restart_required: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SystemState {
    enabled: bool,
    boot_id: String,
}

#[derive(Deserialize, Serialize)]
struct RestartMarker {
    boot_id: String,
}

#[tauri::command]
pub(crate) async fn player_audio_memory_status(
    app: tauri::AppHandle,
    state: State<'_, MemoryCompressionState>,
) -> Result<Status, String> {
    let _guard = state.0.lock().await;
    execute(app, None).await
}

#[tauri::command]
pub(crate) async fn player_audio_set_memory_compression(
    app: tauri::AppHandle,
    state: State<'_, MemoryCompressionState>,
    enabled: bool,
) -> Result<Status, String> {
    let _guard = state.0.lock().await;
    execute(app, Some(enabled)).await
}

async fn execute(app: tauri::AppHandle, change: Option<bool>) -> Result<Status, String> {
    if !cfg!(windows) {
        return if change.is_some() {
            Err("Memory compression management requires Windows".into())
        } else {
            Ok(Status {
                supported: false,
                elevated: false,
                enabled: None,
                restart_required: false,
            })
        };
    }
    if !super::player_audio_elevation::elevated()? {
        return if change.is_some() {
            Err("Administrator privileges are required to change memory compression".into())
        } else {
            Ok(Status {
                supported: true,
                elevated: false,
                enabled: None,
                restart_required: false,
            })
        };
    }
    let marker = app
        .path()
        .local_data_dir()
        .map_err(|error| error.to_string())?
        .join(".vrcs")
        .join("memory-compression-restart.json");
    tauri::async_runtime::spawn_blocking(move || operate(&marker, change, run_system))
        .await
        .map_err(|error| error.to_string())?
}

fn restart_pending(marker: &Path, boot_id: &str) -> Result<bool, String> {
    let data = match std::fs::read(marker) {
        Ok(data) => data,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.to_string()),
    };
    let saved: RestartMarker = match serde_json::from_slice(&data) {
        Ok(saved) => saved,
        Err(_) => {
            // Recover an interrupted write without permanently blocking restoration.
            record_restart(marker, boot_id)?;
            return Ok(true);
        }
    };
    if saved.boot_id == boot_id {
        return Ok(true);
    }
    std::fs::remove_file(marker).map_err(|error| error.to_string())?;
    Ok(false)
}

fn record_restart(marker: &Path, boot_id: &str) -> Result<(), String> {
    use std::io::Write;
    std::fs::create_dir_all(marker.parent().ok_or("Invalid restart record path")?)
        .map_err(|error| error.to_string())?;
    let data = serde_json::to_vec(&RestartMarker {
        boot_id: boot_id.into(),
    })
    .map_err(|error| error.to_string())?;
    let mut file = std::fs::File::create(marker).map_err(|error| error.to_string())?;
    file.write_all(&data)
        .and_then(|_| file.sync_all())
        .map_err(|error| error.to_string())
}

fn operate(
    marker: &Path,
    change: Option<bool>,
    mut run: impl FnMut(Option<bool>) -> Result<SystemState, String>,
) -> Result<Status, String> {
    let mut current = run(None)?;
    let mut restart_required = restart_pending(marker, &current.boot_id)?;
    if let Some(enabled) = change.filter(|enabled| *enabled != current.enabled) {
        // Persist before the system change so a crash cannot lose the restart reminder.
        record_restart(marker, &current.boot_id)?;
        current = run(Some(enabled))?;
        if current.enabled != enabled {
            return Err("Windows did not confirm the requested memory compression setting".into());
        }
        restart_required = true;
    }
    Ok(Status {
        supported: true,
        elevated: true,
        enabled: Some(current.enabled),
        restart_required,
    })
}

// Only these three fixed scripts can run. No execution-policy bypass or downloaded code.
fn script(change: Option<bool>) -> String {
    let operation = match change {
        None => "",
        Some(false) => "Disable-MMAgent -MemoryCompression -ErrorAction Stop;",
        Some(true) => "Enable-MMAgent -MemoryCompression -ErrorAction Stop;",
    };
    format!(
        r#"$ErrorActionPreference='Stop'; $ProgressPreference='SilentlyContinue';
[Console]::OutputEncoding=[System.Text.UTF8Encoding]::new($false);
try {{
  Import-Module (Join-Path $PSHOME 'Modules\MMAgent\MMAgent.psd1') -ErrorAction Stop;
  {operation}
  $compression=(Get-MMAgent -ErrorAction Stop).MemoryCompression;
  if ($compression -isnot [bool]) {{ throw 'Windows did not return a memory compression state' }};
  $boot=(Get-CimInstance -ClassName Win32_OperatingSystem -ErrorAction Stop).LastBootUpTime.ToUniversalTime().Ticks.ToString();
  @{{enabled=$compression;boot_id=$boot}} | ConvertTo-Json -Compress;
}} catch {{ [Console]::Error.WriteLine($_.Exception.Message); exit 1 }}"#
    )
}

fn parse_system_state(bytes: &[u8]) -> Result<SystemState, String> {
    let value: SystemState = serde_json::from_slice(bytes)
        .map_err(|_| "Windows returned an invalid memory compression status".to_string())?;
    if value.boot_id.is_empty()
        || value.boot_id.len() > 32
        || !value.boot_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err("Windows did not return a valid boot identifier".into());
    }
    Ok(value)
}

#[cfg(not(windows))]
fn run_system(_: Option<bool>) -> Result<SystemState, String> {
    Err("Memory compression management requires Windows".into())
}

#[cfg(windows)]
fn run_system(change: Option<bool>) -> Result<SystemState, String> {
    use std::io::Read;
    use std::os::windows::ffi::OsStringExt;
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::System::SystemInformation::GetSystemDirectoryW;
    use windows_sys::Win32::System::Threading::CREATE_NO_WINDOW;

    let mut system_path = vec![0u16; 32_768];
    let size = unsafe { GetSystemDirectoryW(system_path.as_mut_ptr(), system_path.len() as u32) };
    if size == 0 || size as usize >= system_path.len() {
        return Err("Windows system directory is unavailable".into());
    }
    let system_path =
        std::path::PathBuf::from(std::ffi::OsString::from_wide(&system_path[..size as usize]));
    let powershell = system_path.join("WindowsPowerShell").join("v1.0");
    let mut child = Command::new(powershell.join("powershell.exe"))
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &script(change),
        ])
        .env("PSModulePath", powershell.join("Modules"))
        .current_dir(&system_path)
        .creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or("Windows command stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Windows command stderr is unavailable")?;
    fn read_bounded(reader: impl Read) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        reader.take(16_385).read_to_end(&mut bytes)?;
        Ok(bytes)
    }
    let output = std::thread::spawn(move || read_bounded(stdout));
    let errors = std::thread::spawn(move || read_bounded(stderr));
    let started = Instant::now();
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(error) => break Err(error.to_string()),
            Ok(None) if started.elapsed() >= Duration::from_secs(30) => {
                break Err(
                    "Windows memory compression operation timed out; check the status again".into(),
                );
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = output
        .join()
        .map_err(|_| "Windows command output reader stopped")?
        .map_err(|error| error.to_string())?;
    let stderr = errors
        .join()
        .map_err(|_| "Windows command error reader stopped")?
        .map_err(|error| error.to_string())?;
    let status = result?;
    if !status.success() {
        return Err(format!(
            "Windows memory compression operation failed: {}",
            String::from_utf8_lossy(&stderr)
                .chars()
                .take(1200)
                .collect::<String>()
                .trim()
        ));
    }
    if stdout.len() > 16_384 {
        return Err("Windows memory compression response is too large".into());
    }
    parse_system_state(&stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(enabled: bool, boot: &str) -> SystemState {
        SystemState {
            enabled,
            boot_id: boot.into(),
        }
    }

    #[test]
    fn changes_are_verified_and_restart_survives_app_relaunch_until_a_new_boot() {
        let directory =
            std::env::temp_dir().join(format!("vrcs-memory-test-{}", uuid::Uuid::new_v4()));
        let marker = directory.join("restart.json");
        let mut requests = Vec::new();
        let result = operate(&marker, Some(false), |change| {
            requests.push(change);
            Ok(state(change.unwrap_or(true), "100"))
        })
        .unwrap();
        assert_eq!(requests, vec![None, Some(false)]);
        assert_eq!(result.enabled, Some(false));
        assert!(result.restart_required);
        assert!(
            operate(&marker, None, |_| Ok(state(false, "100")))
                .unwrap()
                .restart_required
        );
        // Restoring also requires an explicit system change and a reboot reminder.
        assert!(
            operate(&marker, Some(true), |change| Ok(state(
                change.unwrap_or(false),
                "100"
            )))
            .unwrap()
            .restart_required
        );
        assert!(
            !operate(&marker, None, |_| Ok(state(true, "200")))
                .unwrap()
                .restart_required
        );
        assert!(!marker.exists());
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn no_op_never_mutates_and_failed_verification_is_not_success() {
        let directory =
            std::env::temp_dir().join(format!("vrcs-memory-test-{}", uuid::Uuid::new_v4()));
        let marker = directory.join("restart.json");
        let unchanged = operate(&marker, Some(false), |change| {
            assert_eq!(change, None);
            Ok(state(false, "100"))
        })
        .unwrap();
        assert!(!unchanged.restart_required);
        assert!(!marker.exists());
        assert!(operate(&marker, Some(false), |_| Ok(state(true, "100"))).is_err());
        assert!(restart_pending(&marker, "100").unwrap());
        std::fs::remove_file(marker).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn strict_status_and_fixed_commands_reject_ambiguous_results() {
        assert!(parse_system_state(br#"{"enabled":false,"boot_id":"123"}"#).is_ok());
        for bytes in [
            br#"{"enabled":"False","boot_id":"123"}"#.as_slice(),
            br#"{"enabled":false,"boot_id":""}"#,
            br#"{"enabled":false}"#,
        ] {
            assert!(parse_system_state(bytes).is_err());
        }
        assert!(!script(None).contains("Disable-MMAgent"));
        assert!(script(Some(false)).contains("Disable-MMAgent -MemoryCompression"));
        assert!(script(Some(true)).contains("Enable-MMAgent -MemoryCompression"));
        for command in [script(None), script(Some(false)), script(Some(true))] {
            assert!(!command.contains("Restart-Computer"));
            assert!(!command.contains("-ExecutionPolicy"));
        }
    }

    #[test]
    fn interrupted_restart_record_recovers_without_changing_windows() {
        let directory =
            std::env::temp_dir().join(format!("vrcs-memory-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let marker = directory.join("restart.json");
        std::fs::write(&marker, b"{\"boot_id\":").unwrap();
        assert!(
            operate(&marker, None, |change| {
                assert_eq!(change, None);
                Ok(state(false, "100"))
            })
            .unwrap()
            .restart_required
        );
        assert!(
            !operate(&marker, None, |_| Ok(state(false, "200")))
                .unwrap()
                .restart_required
        );
        std::fs::remove_dir(directory).unwrap();
    }
}
