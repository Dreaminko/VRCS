#[cfg(windows)]
const STEAMVR_SERVER_PROCESS: &str = "vrserver.exe";

#[derive(Default)]
pub struct SteamVrSession {
    connected_pid: Option<u32>,
    quitting_pid: Option<u32>,
}

impl SteamVrSession {
    pub fn can_connect(&self, pid: Option<u32>) -> bool {
        pid.is_some() && pid != self.quitting_pid
    }

    pub fn connected(&mut self, pid: u32) {
        self.connected_pid = Some(pid);
        self.quitting_pid = None;
    }

    pub fn quit(&mut self) {
        self.quitting_pid = self.connected_pid.take();
    }
}

#[cfg(windows)]
pub fn steamvr_pid() -> Option<u32> {
    find_process(None, STEAMVR_SERVER_PROCESS)
}

#[cfg(windows)]
pub fn vrchat_process(pid: u32) -> bool {
    find_process(Some(pid), "vrchat.exe").is_some()
}

#[cfg(windows)]
fn find_process(pid: Option<u32>, name: &str) -> Option<u32> {
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
            return None;
        }

        let mut entry: PROCESSENTRY32W = zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut found = None;

        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let len = entry
                    .szExeFile
                    .iter()
                    .position(|character| *character == 0)
                    .unwrap_or(entry.szExeFile.len());
                let executable = String::from_utf16_lossy(&entry.szExeFile[..len]);
                if executable.eq_ignore_ascii_case(name)
                    && pid.is_none_or(|pid| pid == entry.th32ProcessID)
                {
                    found = Some(entry.th32ProcessID);
                    break;
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }

        let _ = CloseHandle(snapshot);
        found
    }
}

#[cfg(linux_vr)]
pub fn steamvr_pid() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    let uid = unsafe { libc::geteuid() };
    for entry in std::fs::read_dir("/proc").ok()?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if std::fs::read_to_string(format!("/proc/{pid}/comm"))
            .is_ok_and(|comm| comm.trim() == "vrserver")
            && entry.metadata().ok().is_some_and(|meta| meta.uid() == uid)
        {
            return Some(pid);
        }
    }
    None
}

#[cfg(all(not(windows), not(linux_vr)))]
pub fn steamvr_pid() -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::SteamVrSession;

    #[test]
    fn quit_blocks_the_old_server_until_a_new_server_starts() {
        let mut session = SteamVrSession::default();
        assert!(!session.can_connect(None));
        assert!(session.can_connect(Some(42)));
        session.connected(42);
        session.quit();

        assert!(!session.can_connect(Some(42)));
        assert!(!session.can_connect(None));
        assert!(!session.can_connect(Some(42)));
        assert!(session.can_connect(Some(43)));
    }

    #[test]
    fn a_second_shutdown_blocks_the_new_server() {
        let mut session = SteamVrSession::default();
        session.connected(42);
        session.quit();
        session.connected(43);
        session.quit();

        assert!(!session.can_connect(Some(43)));
        assert!(session.can_connect(Some(44)));
    }
}
