const STEAMVR_SERVER_PROCESS: &str = "vrserver.exe";

#[cfg(windows)]
pub fn steamvr_running() -> bool {
    use std::mem::{size_of, zeroed};

    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };

    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            tracing::warn!("Failed to enumerate processes while detecting SteamVR");
            return false;
        }

        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut running = false;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|character| *character == 0)
                    .unwrap_or(entry.szExeFile.len());
                let executable = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if executable.eq_ignore_ascii_case(STEAMVR_SERVER_PROCESS) {
                    running = true;
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        running
    }
}

#[cfg(not(windows))]
pub fn steamvr_running() -> bool {
    false
}
