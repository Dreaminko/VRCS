//! Read-only discovery of the running VRChat process.
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

#[cfg(windows)]
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
