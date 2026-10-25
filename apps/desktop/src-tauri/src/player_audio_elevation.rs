//! User-confirmed UAC relaunch of this exact executable. No arbitrary commands,
//! credentials, audio or keys are passed through the handoff.
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

pub(crate) struct ElevationState {
    pending: Mutex<Option<Action>>,
    launching: AtomicBool,
}
impl ElevationState {
    pub(crate) fn new(pending: Option<Action>) -> Self {
        Self {
            pending: Mutex::new(pending),
            launching: AtomicBool::new(false),
        }
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Outcome {
    Ready,
    Relaunching,
    Cancelled,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Action {
    StartCapture,
    InspectMemoryCompression,
    DisableMemoryCompression,
    EnableMemoryCompression,
}
impl Action {
    fn argument(self) -> &'static str {
        match self {
            Self::StartCapture => "start_capture",
            Self::InspectMemoryCompression => "inspect_memory_compression",
            Self::DisableMemoryCompression => "disable_memory_compression",
            Self::EnableMemoryCompression => "enable_memory_compression",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        [
            Self::StartCapture,
            Self::InspectMemoryCompression,
            Self::DisableMemoryCompression,
            Self::EnableMemoryCompression,
        ]
        .into_iter()
        .find(|action| action.argument() == value)
    }
}

#[tauri::command]
pub(crate) fn player_audio_pending_action(state: State<'_, ElevationState>) -> Option<Action> {
    *state
        .pending
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

#[tauri::command]
pub(crate) fn player_audio_consume_action(state: State<'_, ElevationState>) -> Option<Action> {
    state
        .pending
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .take()
}

pub(crate) fn elevated() -> Result<bool, String> {
    #[cfg(windows)]
    {
        native::elevated()
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

#[tauri::command]
pub(crate) async fn player_audio_ensure_privileges(
    app: tauri::AppHandle,
    action: Action,
) -> Result<Outcome, String> {
    #[cfg(not(windows))]
    {
        let _ = (app, action);
        Err("Player audio requires Windows".into())
    }
    #[cfg(windows)]
    {
        if native::elevated()? {
            return Ok(Outcome::Ready);
        }
        let state = app.state::<ElevationState>();
        if state.launching.swap(true, Ordering::AcqRel) {
            return Err("A Windows elevation request is already open".into());
        }
        let result = tauri::async_runtime::spawn_blocking(move || native::relaunch(action))
            .await
            .map_err(|error| error.to_string())
            .and_then(|result| result);
        match result {
            Ok(Outcome::Relaunching) => {
                // The child acknowledged its identity and is waiting for this
                // process to release Core, its database and the single-instance lock.
                app.exit(0);
                Ok(Outcome::Relaunching)
            }
            result => {
                state.launching.store(false, Ordering::Release);
                result
            }
        }
    }
}

const RESUME_FLAG: &str = "--player-audio-handoff";
#[derive(Debug, PartialEq, Eq)]
struct Handoff {
    action: Action,
    pid: u32,
    created: u64,
    nonce: uuid::Uuid,
}

fn parse_handoff(args: &[String]) -> Result<Option<Handoff>, String> {
    if !args.iter().any(|arg| arg == RESUME_FLAG) {
        return Ok(None);
    }
    if args.len() != 5 || args[0] != RESUME_FLAG {
        return Err("Invalid elevation handoff".into());
    }
    let action = Action::parse(&args[1]);
    let pid = args[2].parse::<u32>().ok().filter(|pid| *pid != 0);
    let created = args[3].parse::<u64>().ok().filter(|created| *created != 0);
    let nonce = uuid::Uuid::parse_str(&args[4]).ok();
    match (action, pid, created, nonce) {
        (Some(action), Some(pid), Some(created), Some(nonce)) => Ok(Some(Handoff {
            action,
            pid,
            created,
            nonce,
        })),
        _ => Err("Invalid elevation handoff".into()),
    }
}

pub(crate) fn startup() -> Result<Option<Action>, String> {
    let Some(handoff) = parse_handoff(&std::env::args().skip(1).collect::<Vec<_>>())? else {
        return Ok(None);
    };
    #[cfg(windows)]
    {
        let action = handoff.action;
        native::accept(handoff)?;
        Ok(Some(action))
    }
    #[cfg(not(windows))]
    {
        let _ = handoff;
        Err("Player audio elevation requires Windows".into())
    }
}

#[cfg(windows)]
mod native {
    use super::{Action, Handoff, Outcome, RESUME_FLAG};
    use std::ffi::OsStr;
    use std::mem::size_of;
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::*;
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::System::Com::*;
    use windows_sys::Win32::System::Threading::*;
    use windows_sys::Win32::UI::Shell::*;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    struct Handle(HANDLE);
    impl Drop for Handle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    fn owned(handle: HANDLE) -> Result<Handle, String> {
        if handle.is_null() {
            Err(std::io::Error::last_os_error().to_string())
        } else {
            Ok(Handle(handle))
        }
    }
    fn wide(value: impl AsRef<OsStr>) -> Vec<u16> {
        value.as_ref().encode_wide().chain(Some(0)).collect()
    }
    fn token(process: HANDLE) -> Result<Handle, String> {
        let mut handle = std::ptr::null_mut();
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        owned(handle)
    }
    pub(super) fn elevated() -> Result<bool, String> {
        let token = token(unsafe { GetCurrentProcess() })?;
        let mut value: TOKEN_ELEVATION = unsafe { std::mem::zeroed() };
        let mut length = 0;
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenElevation,
                (&mut value as *mut TOKEN_ELEVATION).cast(),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut length,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(value.TokenIsElevated != 0)
    }
    fn creation(process: HANDLE) -> Result<u64, String> {
        let mut times = [FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        }; 4];
        if unsafe {
            GetProcessTimes(
                process,
                &mut times[0],
                &mut times[1],
                &mut times[2],
                &mut times[3],
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok((u64::from(times[0].dwHighDateTime) << 32) | u64::from(times[0].dwLowDateTime))
    }
    fn user(process: HANDLE) -> Result<Vec<usize>, String> {
        let token = token(process)?;
        let mut length = 0;
        unsafe {
            GetTokenInformation(token.0, TokenUser, std::ptr::null_mut(), 0, &mut length);
        }
        if length < size_of::<TOKEN_USER>() as u32 || length > 16_384 {
            return Err("Invalid Windows account identity".into());
        }
        let mut data = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                data.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
        Ok(data)
    }
    fn same_user(process: HANDLE) -> Result<bool, String> {
        let other = user(process)?;
        let current = user(unsafe { GetCurrentProcess() })?;
        Ok(unsafe {
            EqualSid(
                (*(other.as_ptr().cast::<TOKEN_USER>())).User.Sid,
                (*(current.as_ptr().cast::<TOKEN_USER>())).User.Sid,
            )
        } != 0)
    }
    fn event_name(nonce: uuid::Uuid) -> Vec<u16> {
        wide(format!("Local\\VRCS-player-audio-{nonce}"))
    }

    pub(super) fn relaunch(action: Action) -> Result<Outcome, String> {
        std::thread::Builder::new()
            .name("vrcs-player-audio-uac".into())
            .spawn(move || relaunch_on_sta(action))
            .map_err(|error| error.to_string())?
            .join()
            .map_err(|_| "Windows elevation thread stopped unexpectedly".to_string())?
    }

    fn relaunch_on_sta(action: Action) -> Result<Outcome, String> {
        // ShellExecuteEx may activate shell COM extensions; use its own STA thread.
        struct Com(bool);
        impl Drop for Com {
            fn drop(&mut self) {
                if self.0 {
                    unsafe {
                        CoUninitialize();
                    }
                }
            }
        }
        let initialized = unsafe {
            CoInitializeEx(
                std::ptr::null(),
                COINIT_APARTMENTTHREADED as u32 | COINIT_DISABLE_OLE1DDE as u32,
            )
        };
        if initialized < 0 {
            return Err(format!(
                "Windows elevation initialization failed ({initialized:#x})"
            ));
        }
        let _com = Com(true);
        let handoff = Handoff {
            action,
            pid: std::process::id(),
            created: creation(unsafe { GetCurrentProcess() })?,
            nonce: uuid::Uuid::new_v4(),
        };
        let name = event_name(handoff.nonce);
        let ready = owned(unsafe { CreateEventW(std::ptr::null(), 1, 0, name.as_ptr()) })?;
        if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
            return Err("Elevation handoff already exists".into());
        }
        let executable = std::env::current_exe().map_err(|error| error.to_string())?;
        let file = wide(&executable);
        let directory = wide(
            executable
                .parent()
                .ok_or("Executable directory is unavailable")?,
        );
        let verb = wide("runas");
        let parameters = wide(format!(
            "{RESUME_FLAG} {} {} {} {}",
            handoff.action.argument(),
            handoff.pid,
            handoff.created,
            handoff.nonce
        ));
        let mut info: SHELLEXECUTEINFOW = unsafe { std::mem::zeroed() };
        info.cbSize = size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS | SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = parameters.as_ptr();
        info.lpDirectory = directory.as_ptr();
        info.nShow = SW_SHOWNORMAL;
        if unsafe { ShellExecuteExW(&mut info) } == 0 {
            let error = unsafe { GetLastError() };
            return if error == ERROR_CANCELLED {
                Ok(Outcome::Cancelled)
            } else {
                Err(std::io::Error::from_raw_os_error(error as i32).to_string())
            };
        }
        let child = owned(info.hProcess)?;
        let handles = [ready.0, child.0];
        if unsafe { WaitForMultipleObjects(2, handles.as_ptr(), 0, 15_000) } != WAIT_OBJECT_0 {
            return Err("The elevated application did not accept the handoff. This window has been kept open; use the same Windows account for UAC.".into());
        }
        Ok(Outcome::Relaunching)
    }

    pub(super) fn accept(handoff: Handoff) -> Result<(), String> {
        if !elevated()? || handoff.pid == std::process::id() {
            return Err("Invalid elevated startup".into());
        }
        let parent = owned(unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
                0,
                handoff.pid,
            )
        })?;
        if creation(parent.0)? != handoff.created || !same_user(parent.0)? {
            return Err("Elevation parent identity changed".into());
        }
        let mut path = vec![0u16; 32_768];
        let mut length = path.len() as u32;
        if unsafe { QueryFullProcessImageNameW(parent.0, 0, path.as_mut_ptr(), &mut length) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let parent_path =
            std::path::PathBuf::from(String::from_utf16_lossy(&path[..length as usize]));
        let parent_path = parent_path
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let own_path = std::env::current_exe()
            .and_then(|path| path.canonicalize())
            .map_err(|error| error.to_string())?;
        if !parent_path
            .to_string_lossy()
            .eq_ignore_ascii_case(&own_path.to_string_lossy())
        {
            return Err("Elevation executable mismatch".into());
        }
        let ready = owned(unsafe {
            OpenEventW(EVENT_MODIFY_STATE, 0, event_name(handoff.nonce).as_ptr())
        })?;
        if unsafe { SetEvent(ready.0) } == 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        if unsafe { WaitForSingleObject(parent.0, 45_000) } != WAIT_OBJECT_0 {
            return Err("Previous VRCS instance did not finish shutting down".into());
        }
        Ok(())
    }

    #[test]
    fn current_process_identity_is_readable_without_elevating() {
        assert!(creation(unsafe { GetCurrentProcess() }).unwrap() > 0);
        assert!(same_user(unsafe { GetCurrentProcess() }).unwrap());
        let _ = elevated().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handoff_accepts_only_allowed_actions_numeric_arguments_and_nonce() {
        let nonce = "01234567-0123-4123-8123-012345678901";
        for action in [
            Action::StartCapture,
            Action::InspectMemoryCompression,
            Action::DisableMemoryCompression,
            Action::EnableMemoryCompression,
        ] {
            let args = [RESUME_FLAG, action.argument(), "42", "100", nonce].map(str::to_owned);
            let parsed = parse_handoff(&args).unwrap().unwrap();
            assert_eq!(parsed.pid, 42);
            assert_eq!(parsed.action, action);
        }
        assert!(parse_handoff(&[]).unwrap().is_none());
        for invalid in [
            vec![RESUME_FLAG, "42"],
            vec![RESUME_FLAG, "start_capture", "0", "100", nonce],
            vec![RESUME_FLAG, "start_capture", "42", "100", "file.exe"],
            vec![RESUME_FLAG, "powershell.exe", "42", "100", nonce],
            vec!["other", RESUME_FLAG, "start_capture", "42", "100", nonce],
        ] {
            assert!(
                parse_handoff(&invalid.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err()
            );
        }
    }
}
