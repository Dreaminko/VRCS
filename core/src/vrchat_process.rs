//! Read-only discovery of the running VRChat process.
#[cfg(target_os = "linux")]
use player_audio_helper::procfs;
#[cfg(target_os = "linux")]
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ProcessIdentity {
    pub pid: u32,
    pub created: u64,
}

#[cfg(windows)]
pub(crate) fn identity(pid: u32) -> Result<ProcessIdentity, String> {
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|e| e.to_string())?;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let result =
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|e| e.to_string())?;
    if exited.dwLowDateTime != 0 || exited.dwHighDateTime != 0 {
        return Err("VRChat process exited".into());
    }
    Ok(ProcessIdentity {
        pid,
        created: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    })
}

#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn is_alive(owner: ProcessIdentity) -> bool {
    identity(owner.pid).ok() == Some(owner)
}

#[cfg(windows)]
pub(crate) fn game() -> Result<Option<(ProcessIdentity, PathBuf)>, String> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };

    unsafe {
        let snapshot =
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if matches_exe_name(&entry.szExeFile[..length], b"VRChat.exe") {
                if let Ok(owner) = identity(entry.th32ProcessID) {
                    found = Some(owner);
                    break;
                }
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        let Some(owner) = found else { return Ok(None) };
        // A process may exit between the snapshot and opening its image path.
        // Treat that as still waiting, rather than failing the armed capture.
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, owner.pid) else {
            return Ok(None);
        };
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(path.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        if result.is_err() && !is_alive(owner) {
            return Ok(None);
        }
        result.map_err(|e| e.to_string())?;
        Ok(Some((
            owner,
            PathBuf::from(String::from_utf16_lossy(&path[..length as usize])),
        )))
    }
}

#[cfg(windows)]
fn matches_exe_name(units: &[u16], expected: &[u8]) -> bool {
    units.len() == expected.len()
        && units
            .iter()
            .zip(expected)
            .all(|(&unit, &byte)| unit <= 0x7f && (unit as u8).eq_ignore_ascii_case(&byte))
}

#[cfg(target_os = "linux")]
pub(crate) fn game() -> Result<Option<(ProcessIdentity, PathBuf)>, String> {
    let mut found = None;
    let entries = fs::read_dir("/proc").map_err(|error| error.to_string())?;
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if pid == std::process::id() {
            continue;
        }
        if !same_uid(pid) {
            continue;
        }
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if !comm.to_ascii_lowercase().contains("vrchat.exe")
            && !String::from_utf8_lossy(&cmdline)
                .to_ascii_lowercase()
                .contains("vrchat.exe")
        {
            continue;
        }
        let created = match procfs::starttime(pid) {
            Ok(created) => created,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::PermissionDenied
                ) || error.raw_os_error() == Some(libc::ESRCH) =>
            {
                continue;
            }
            Err(error) => return Err(format!("Cannot read VRChat process: {error}")),
        };
        let owner = ProcessIdentity { pid, created };
        // The EAC launcher also has VRChat in its command line; only the
        // IL2CPP host maps GameAssembly.dll.
        let maps = fs::read_to_string(format!("/proc/{pid}/maps")).unwrap_or_default();
        if !maps.to_ascii_lowercase().contains("gameassembly.dll") {
            continue;
        }
        if found.replace(owner).is_some() {
            return Err("More than one VRChat process is running; close the extra instance".into());
        }
    }
    Ok(found.map(|owner| (owner, executable(owner.pid))))
}

#[cfg(target_os = "linux")]
fn executable(pid: u32) -> PathBuf {
    fs::read_link(format!("/proc/{pid}/exe")).unwrap_or_default()
}

#[cfg(target_os = "linux")]
fn same_uid(pid: u32) -> bool {
    procfs::uids(pid).is_ok_and(|uids| uids[0] == unsafe { libc::geteuid() })
}

#[cfg(target_os = "linux")]
pub(crate) fn identity(pid: u32) -> Result<ProcessIdentity, String> {
    let created =
        procfs::starttime(pid).map_err(|error| format!("Cannot read VRChat process: {error}"))?;
    Ok(ProcessIdentity { pid, created })
}
