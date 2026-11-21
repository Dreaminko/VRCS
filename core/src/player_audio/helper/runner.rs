use std::io::{self, Read};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    capture, procfs, read_command, scan, write_event, Command, Event, ScanProgress, ScanResult,
    VERSION,
};

struct FsUidGuard;
impl Drop for FsUidGuard {
    fn drop(&mut self) {
        unsafe { libc::setfsuid(0) };
    }
}

struct ActiveScan {
    request: u64,
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<()>,
}

impl ActiveScan {
    fn cancel(&self, request: u64) {
        if self.request == request {
            self.stop.store(true, Ordering::Relaxed);
        }
    }
}

const CAP_NET_ADMIN: u32 = 12;
const CAP_NET_RAW: u32 = 13;
const CAP_SYS_PTRACE: u32 = 19;
const INITIAL_CAPS: u32 = (1 << CAP_NET_ADMIN) | (1 << CAP_NET_RAW) | (1 << CAP_SYS_PTRACE);
const RUNTIME_CAPS: u32 = 1 << CAP_SYS_PTRACE;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const PR_SET_NO_NEW_PRIVS: libc::c_int = 38;

#[repr(C)]
struct CapHeader {
    version: u32,
    pid: libc::c_int,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CapData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

fn apply_capabilities(mask: u32) -> Result<(), String> {
    let mut header = CapHeader {
        version: LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let mut data = [CapData::default(); 2];
    if unsafe { libc::syscall(libc::SYS_capget, &mut header, data.as_mut_ptr()) } != 0 {
        return Err(format!("capget failed: {}", io::Error::last_os_error()));
    }
    let allowed = data[0].permitted & mask;
    data[0] = CapData {
        effective: allowed,
        permitted: allowed,
        inheritable: 0,
    };
    data[1] = CapData::default();
    if unsafe { libc::syscall(libc::SYS_capset, &mut header, data.as_ptr()) } != 0 {
        return Err(format!("capset failed: {}", io::Error::last_os_error()));
    }
    Ok(())
}

fn drop_to_user_with_caps(uid: u32, gid: u32, caps_mask: u32) -> Result<(), String> {
    if unsafe { libc::prctl(libc::PR_SET_KEEPCAPS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot retain helper capabilities across UID drop: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setgroups(0, std::ptr::null()) } != 0 {
        return Err(format!(
            "cannot clear helper supplementary groups: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setresgid(gid, gid, gid) } != 0 {
        return Err(format!(
            "cannot drop helper GID: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::setresuid(uid, uid, uid) } != 0 {
        return Err(format!(
            "cannot drop helper UID: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot disable helper core dumps after UID drop: {}",
            io::Error::last_os_error()
        ));
    }
    apply_capabilities(caps_mask)?;
    if unsafe { libc::prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot enable PR_SET_NO_NEW_PRIVS: {}",
            io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub(crate) fn run(args: impl IntoIterator<Item = String>) -> Result<(), String> {
    let mut args = args.into_iter().peekable();
    if args.peek().map(String::as_str) == Some("--version") {
        println!(
            "vrcs-network-helper {} (protocol {VERSION})",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
        return Err(format!(
            "cannot disable helper core dumps: {}",
            io::Error::last_os_error()
        ));
    }
    let zero_core = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &zero_core) } != 0 {
        return Err(format!(
            "cannot clear core limit: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err("must run as root through pkexec".into());
    }
    let expected_uid = std::env::var("PKEXEC_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|uid| *uid != 0)
        .ok_or("missing or invalid PKEXEC_UID")?;
    let parent = unsafe { libc::getppid() };
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(format!(
            "cannot bind helper to parent lifetime: {}",
            io::Error::last_os_error()
        ));
    }
    if parent == 1 || unsafe { libc::getppid() } != parent {
        return Err("VRCS exited before helper startup".into());
    }
    let mut socket = None;
    let mut pid = None;
    let mut starttime = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--socket" => socket = Some(PathBuf::from(args.next().ok_or("missing socket")?)),
            "--pid" => {
                pid = Some(
                    args.next()
                        .ok_or("missing pid")?
                        .parse::<u32>()
                        .map_err(|_| "invalid pid".to_string())?,
                );
            }
            "--starttime" => {
                starttime = Some(
                    args.next()
                        .ok_or("missing starttime")?
                        .parse::<u64>()
                        .map_err(|_| "invalid starttime".to_string())?,
                );
            }
            _ => return Err("unknown argument".into()),
        }
    }
    let socket = socket.ok_or("missing --socket")?;
    let pid = pid
        .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)
        .ok_or("invalid --pid")?;
    let starttime = starttime.ok_or("missing --starttime")?;
    procfs::parent_is_private(&socket, Some(expected_uid))?;

    let mut token = String::new();
    io::stdin()
        .take(257)
        .read_to_string(&mut token)
        .map_err(|error| format!("cannot read authorization token: {error}"))?;
    if token.len() > 256 {
        return Err("invalid authorization token".into());
    }
    let token = token.trim().to_string();
    if token.is_empty() || token.len() > 128 {
        return Err("invalid authorization token".into());
    }
    let stream = {
        let _fsuid = FsUidGuard;
        unsafe { libc::setfsuid(expected_uid) };
        UnixStream::connect(&socket).map_err(|error| format!("cannot connect to VRCS: {error}"))?
    };
    let expected_gid = verify_peer(&stream, expected_uid)?;
    drop_to_user_with_caps(expected_uid, expected_gid, INITIAL_CAPS)?;
    // setresuid() clears the parent-death signal on Linux; restore it immediately.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) } != 0 {
        return Err(format!(
            "cannot bind helper to parent lifetime after UID drop: {}",
            io::Error::last_os_error()
        ));
    }
    if unsafe { libc::getppid() } != parent {
        return Err("VRCS exited before helper startup".into());
    }
    stream
        .set_write_timeout(Some(Duration::from_millis(500)))
        .map_err(|error| error.to_string())?;
    let shutdown = Arc::new(stream.try_clone().map_err(|error| error.to_string())?);
    let writer = Arc::new(Mutex::new(
        stream.try_clone().map_err(|error| error.to_string())?,
    ));
    send(
        &writer,
        Event::Hello {
            version: VERSION,
            token,
            pid,
            starttime,
            monotonic_us: procfs::monotonic_us(),
        },
    )?;
    let capture = procfs::validate_target(pid, starttime, expected_uid)
        .map_err(|error| error.to_string())
        .and_then(|()| capture::Capture::open(pid, starttime));
    let capture = match capture {
        Ok(capture) => {
            // Socket is open and filter-locked; permanently drop CAP_NET_RAW and CAP_NET_ADMIN.
            apply_capabilities(RUNTIME_CAPS)?;
            capture
        }
        Err(_) if procfs::exited(pid, starttime) => return Ok(()),
        Err(error) => {
            let _ = apply_capabilities(RUNTIME_CAPS);
            let _ = send(
                &writer,
                Event::Status {
                    fatal: true,
                    detail: error.clone(),
                },
            );
            return Err(error);
        }
    };
    let capture_stop = capture.stop_flag();
    let packet_stop = Arc::clone(&capture_stop);
    let packet_writer = Arc::clone(&writer);
    let packet_shutdown = Arc::clone(&shutdown);
    let _packet_thread = std::thread::spawn(move || {
        let result = capture.run(&packet_writer);
        packet_stop.store(true, Ordering::Relaxed);
        if let Err(error) = result {
            let _ = send(
                &packet_writer,
                Event::Status {
                    fatal: true,
                    detail: error,
                },
            );
        }
        let _ = packet_shutdown.shutdown(Shutdown::Both);
    });

    let mut commands = stream;
    let mut active: Option<ActiveScan> = None;
    let result = loop {
        let command = match read_command(&mut commands) {
            Ok(Some(command)) => command,
            Ok(None) => break Ok(()),
            Err(error) => {
                let detail = format!("helper command channel failed: {error}");
                let _ = send(
                    &writer,
                    Event::Status {
                        fatal: true,
                        detail: detail.clone(),
                    },
                );
                break Err(detail);
            }
        };
        if capture_stop.load(Ordering::Relaxed) {
            break Ok(());
        }
        match command {
            Command::Shutdown => break Ok(()),
            Command::Cancel { request } => {
                if let Some(scan) = &active {
                    scan.cancel(request);
                }
            }
            Command::Scan { request, packets } => {
                if active
                    .as_ref()
                    .is_some_and(|scan| !scan.thread.is_finished())
                {
                    if let Err(error) = send(
                        &writer,
                        Event::ScanResult(ScanResult {
                            request,
                            key: None,
                            scanned: 0,
                            candidates: 0,
                            elapsed_ms: 0,
                            error: Some("a key scan is already running".into()),
                        }),
                    ) {
                        break Err(error);
                    }
                    continue;
                }
                // Revalidate the full target constraints at each scan startup.
                if let Err(error) = procfs::validate_target(pid, starttime, expected_uid) {
                    if procfs::exited(pid, starttime) {
                        break Ok(());
                    }
                    let detail = error.to_string();
                    let _ = send(
                        &writer,
                        Event::Status {
                            fatal: true,
                            detail: detail.clone(),
                        },
                    );
                    break Err(detail);
                }
                let stop = Arc::new(AtomicBool::new(false));
                let thread_stop = Arc::clone(&stop);
                let thread_writer = Arc::clone(&writer);
                let thread_shutdown = Arc::clone(&shutdown);
                let session_stop = Arc::clone(&capture_stop);
                let thread = std::thread::spawn(move || {
                    let started = Instant::now();
                    let outcome = scan::scan(
                        pid,
                        starttime,
                        expected_uid,
                        &packets,
                        &thread_stop,
                        started + Duration::from_secs(40),
                        |scanned, total, candidates| {
                            if send(
                                &thread_writer,
                                Event::ScanProgress(ScanProgress {
                                    request,
                                    scanned,
                                    total,
                                    candidates,
                                    elapsed_ms: started.elapsed().as_millis() as u64,
                                }),
                            )
                            .is_err()
                            {
                                thread_stop.store(true, Ordering::Relaxed);
                            }
                        },
                    );
                    let event = match outcome {
                        Ok(outcome) => Event::ScanResult(ScanResult {
                            request,
                            key: outcome.key,
                            scanned: outcome.scanned,
                            candidates: outcome.candidates,
                            elapsed_ms: outcome.elapsed_ms,
                            error: None,
                        }),
                        Err(error)
                            if matches!(
                                error.kind(),
                                io::ErrorKind::Interrupted | io::ErrorKind::TimedOut
                            ) =>
                        {
                            Event::ScanResult(ScanResult {
                                request,
                                key: None,
                                scanned: 0,
                                candidates: 0,
                                elapsed_ms: started.elapsed().as_millis() as u64,
                                error: Some(error.to_string()),
                            })
                        }
                        Err(error) => {
                            session_stop.store(true, Ordering::Relaxed);
                            if procfs::exited(pid, starttime) {
                                let _ = thread_shutdown.shutdown(Shutdown::Both);
                                return;
                            }
                            let _ = send(
                                &thread_writer,
                                Event::Status {
                                    fatal: true,
                                    detail: error.to_string(),
                                },
                            );
                            let _ = thread_shutdown.shutdown(Shutdown::Both);
                            return;
                        }
                    };
                    if send(&thread_writer, event).is_err() {
                        session_stop.store(true, Ordering::Relaxed);
                        let _ = thread_shutdown.shutdown(Shutdown::Both);
                    }
                });
                active = Some(ActiveScan {
                    request,
                    stop,
                    thread,
                });
            }
        }
    };
    // Wake IPC writers first; do not wait for a thread inside a kernel memory read.
    capture_stop.store(true, Ordering::Relaxed);
    if let Some(scan) = active {
        scan.stop.store(true, Ordering::Relaxed);
    }
    let _ = shutdown.shutdown(Shutdown::Both);
    result
}

fn send(writer: &Mutex<UnixStream>, event: Event) -> Result<(), String> {
    writer
        .lock()
        .map_err(|_| "helper writer lock failed".to_string())
        .and_then(|mut writer| write_event(&mut *writer, &event).map_err(|error| error.to_string()))
}

fn verify_peer(stream: &UnixStream, expected_uid: u32) -> Result<u32, String> {
    let (_, uid, gid) =
        procfs::peer_credentials(stream).map_err(|_| "cannot verify the VRCS peer")?;
    if uid != expected_uid {
        return Err("VRCS peer uid does not match pkexec caller".into());
    }
    Ok(gid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_only_stops_the_matching_request() {
        let scan = ActiveScan {
            request: 7,
            stop: Arc::new(AtomicBool::new(false)),
            thread: std::thread::spawn(|| {}),
        };
        scan.cancel(8);
        assert!(!scan.stop.load(Ordering::Relaxed));
        scan.cancel(7);
        assert!(scan.stop.load(Ordering::Relaxed));
    }

    #[test]
    fn verify_peer_requires_matching_caller_uid() {
        let (a, _b) = UnixStream::pair().unwrap();
        let current_uid = unsafe { libc::getuid() };
        let current_gid = unsafe { libc::getgid() };
        assert_eq!(verify_peer(&a, current_uid).unwrap(), current_gid);
        assert_eq!(
            verify_peer(&a, current_uid.wrapping_add(1)).unwrap_err(),
            "VRCS peer uid does not match pkexec caller"
        );
    }
}
