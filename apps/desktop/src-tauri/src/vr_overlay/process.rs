#[cfg(windows)]
const STEAMVR_SERVER_PROCESS: &str = "vrserver.exe";

#[derive(Default)]
pub struct OpenVrSession {
    connected_pid: Option<u32>,
    quitting_pid: Option<u32>,
}

impl OpenVrSession {
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

#[cfg(all(linux_vr, target_arch = "x86_64"))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LinuxXrDaemons {
    pub wivrn: bool,
    pub monado: bool,
    pub steamvr: bool,
}

#[cfg(windows)]
fn steamvr_pid() -> Option<u32> {
    find_process(None, STEAMVR_SERVER_PROCESS)
}

#[cfg(windows)]
pub fn openvr_server_pid(
    _runtime_path: Option<&std::path::Path>,
    _explicit_openvr: bool,
) -> Option<u32> {
    steamvr_pid()
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
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
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
            let target = name.as_bytes();
            loop {
                if pid.is_none_or(|pid| pid == entry.th32ProcessID) {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|character| *character == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let exe = &entry.szExeFile[..len];
                    if exe.len() == target.len()
                        && exe.iter().zip(target).all(|(&w, &b)| {
                            u8::try_from(w).is_ok_and(|c| c.eq_ignore_ascii_case(&b))
                        })
                    {
                        found = Some(entry.th32ProcessID);
                        break;
                    }
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
fn proc_alive(pid: u32) -> bool {
    let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) else {
        return false;
    };
    let Some(rparen) = stat.rfind(')') else {
        return false;
    };
    stat.get(rparen + 2..)
        .and_then(|rest| rest.split_whitespace().next())
        .is_some_and(|state| !matches!(state, "Z" | "X" | "x"))
}

#[cfg(linux_vr)]
fn proc_matches_any(pid: u32, names: &[&str]) -> bool {
    if std::fs::read_to_string(format!("/proc/{pid}/comm"))
        .is_ok_and(|comm| names.contains(&comm.trim()))
    {
        return true;
    }
    let Ok(cmdline) = std::fs::read(format!("/proc/{pid}/cmdline")) else {
        return false;
    };
    let argv0 = cmdline.split(|&byte| byte == 0).next().unwrap_or_default();
    let base = String::from_utf8_lossy(argv0);
    base.rsplit('/')
        .next()
        .is_some_and(|name| names.contains(&name))
}

#[cfg(linux_vr)]
fn for_each_user_process(mut visit: impl FnMut(u32) -> bool) {
    use std::os::unix::fs::MetadataExt;
    let uid = unsafe { libc::geteuid() };
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return;
    };
    for entry in entries.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
        if entry.metadata().ok().is_some_and(|meta| meta.uid() == uid) && !visit(pid) {
            break;
        }
    }
}

#[cfg(linux_vr)]
fn find_user_process(names: &[&str]) -> Option<u32> {
    let mut found = None;
    for_each_user_process(|pid| {
        if proc_matches_any(pid, names) && proc_alive(pid) {
            found = Some(pid);
            return false;
        }
        true
    });
    found
}

#[cfg(all(linux_vr, target_arch = "x86_64"))]
pub fn linux_xr_daemons() -> LinuxXrDaemons {
    let mut daemons = LinuxXrDaemons::default();
    for_each_user_process(|pid| {
        if !daemons.wivrn && proc_matches_any(pid, &["wivrn-server"]) && proc_alive(pid) {
            daemons.wivrn = true;
        } else if !daemons.monado && proc_matches_any(pid, &["monado-service"]) && proc_alive(pid) {
            daemons.monado = true;
        } else if !daemons.steamvr && proc_matches_any(pid, &["vrserver"]) && proc_alive(pid) {
            daemons.steamvr = true;
        }
        !(daemons.wivrn && daemons.monado && daemons.steamvr)
    });
    daemons
}

#[cfg(linux_vr)]
pub fn openvr_runtime_is_steamvr(runtime_path: Option<&std::path::Path>) -> bool {
    runtime_path.is_some_and(|dir| {
        dir.join("bin/linux64/vrserver").is_file() || dir.join("bin/vrserver").is_file()
    })
}

#[cfg(linux_vr)]
pub fn openvr_server_pid(
    runtime_path: Option<&std::path::Path>,
    explicit_openvr: bool,
) -> Option<u32> {
    if !explicit_openvr || openvr_runtime_is_steamvr(runtime_path) {
        find_user_process(&["vrserver"])
    } else {
        find_user_process(&["vrserver", "wivrn-server", "monado-service"])
    }
}

#[cfg(all(not(windows), not(linux_vr)))]
pub fn openvr_server_pid(
    _runtime_path: Option<&std::path::Path>,
    _explicit_openvr: bool,
) -> Option<u32> {
    None
}

#[cfg(test)]
mod tests {
    use super::OpenVrSession;

    #[test]
    fn steamvr_quit_blocks_the_old_server_until_a_new_server_starts() {
        let mut session = OpenVrSession::default();
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
        let mut session = OpenVrSession::default();
        session.connected(42);
        session.quit();
        session.connected(43);
        session.quit();

        assert!(!session.can_connect(Some(43)));
        assert!(session.can_connect(Some(44)));
    }

    #[test]
    fn missing_server_pid_never_allows_connecting() {
        let mut session = OpenVrSession::default();
        assert!(!session.can_connect(None));
        session.connected(77);
        session.quit();
        assert!(!session.can_connect(None));
        assert!(!session.can_connect(Some(77)));
        assert!(session.can_connect(Some(78)));
    }

    #[cfg(linux_vr)]
    #[test]
    fn steamvr_runtime_detection_checks_vrserver_binary() {
        let root =
            std::env::temp_dir().join(format!("vrcs-openvr-runtime-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let xrizer = root.join("xrizer");
        std::fs::create_dir_all(xrizer.join("bin/linux64")).unwrap();
        assert!(!super::openvr_runtime_is_steamvr(None));
        assert!(!super::openvr_runtime_is_steamvr(Some(&xrizer)));

        let steamvr = root.join("SteamVR");
        std::fs::create_dir_all(steamvr.join("bin/linux64")).unwrap();
        std::fs::write(steamvr.join("bin/linux64/vrserver"), b"").unwrap();
        assert!(super::openvr_runtime_is_steamvr(Some(&steamvr)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
