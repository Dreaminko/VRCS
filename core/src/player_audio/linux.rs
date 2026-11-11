//! Linux network capture through a session-scoped pkexec helper. Core never
//! receives raw packet sockets, game memory, or unauthenticated key candidates.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use player_audio_helper::{read_event, write_command, Command as HelperCommand, Event, ScanResult};

use super::cache::ProcessIdentity;
use super::connection::{Connection, Datagram};
use super::SharedStatus;

const HELPER_PATHS: [&str; 2] = [
    "/usr/lib/vrcs/vrcs-network-helper",
    "/usr/local/lib/vrcs/vrcs-network-helper",
];

pub(crate) fn network_available() -> bool {
    Path::new("/usr/bin/pkexec").is_file() && helper_path().is_some()
}

fn helper_path() -> Option<PathBuf> {
    HELPER_PATHS
        .iter()
        .map(PathBuf::from)
        .find(|path| trusted_helper(path))
}

fn trusted_helper(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    let Some(parent) = path.parent().and_then(|parent| fs::metadata(parent).ok()) else {
        return false;
    };
    metadata.is_file()
        && metadata.uid() == 0
        && metadata.mode() & 0o111 != 0
        && metadata.mode() & 0o022 == 0
        && parent.is_dir()
        && parent.uid() == 0
        && parent.mode() & 0o022 == 0
}

pub(super) fn game() -> Result<Option<(u32, PathBuf)>, String> {
    let mut found = Vec::new();
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
        let comm = fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        let cmdline = fs::read(format!("/proc/{pid}/cmdline")).unwrap_or_default();
        if !comm.to_ascii_lowercase().contains("vrchat.exe")
            && !String::from_utf8_lossy(&cmdline)
                .to_ascii_lowercase()
                .contains("vrchat.exe")
        {
            continue;
        }
        let Ok(owner) = process_identity(pid) else {
            continue;
        };
        if !same_uid(pid) {
            continue;
        }
        // The EAC launcher also has VRChat in its command line; only the
        // IL2CPP host maps GameAssembly.dll.
        let maps = fs::read_to_string(format!("/proc/{pid}/maps")).unwrap_or_default();
        if !maps.to_ascii_lowercase().contains("gameassembly.dll") {
            continue;
        }
        found.push((pid, owner));
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(Some((found[0].0, executable(found[0].0)))),
        _ => Err("More than one VRChat process is running; close the extra instance".into()),
    }
}

fn executable(pid: u32) -> PathBuf {
    fs::read_link(format!("/proc/{pid}/exe")).unwrap_or_default()
}

fn same_uid(pid: u32) -> bool {
    let Ok(status) = fs::read_to_string(format!("/proc/{pid}/status")) else {
        return false;
    };
    let Some(uid) = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|uid| uid.parse::<u32>().ok())
    else {
        return false;
    };
    uid == unsafe { libc::geteuid() }
}

pub(super) fn process_identity(pid: u32) -> Result<ProcessIdentity, String> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))
        .map_err(|error| format!("Cannot read VRChat process: {error}"))?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| "Invalid VRChat process status".to_string())?;
    let created = stat[end + 1..]
        .split_whitespace()
        .nth(19)
        .ok_or_else(|| "Invalid VRChat process status".to_string())?
        .parse::<u64>()
        .map_err(|_| "Invalid VRChat process start time".to_string())?;
    Ok(ProcessIdentity { pid, created })
}

struct Waiter {
    request: u64,
    sender: mpsc::SyncSender<ScanResult>,
}

pub(super) struct Network {
    child: Option<Child>,
    writer: UnixStream,
    reader: Option<std::thread::JoinHandle<()>>,
    packets: mpsc::Receiver<Datagram>,
    waiter: Arc<Mutex<Option<Waiter>>>,
    fatal: Arc<Mutex<Option<String>>>,
    next_request: AtomicU64,
    _runtime: RuntimeDir,
}

struct RuntimeDir(PathBuf);

impl Drop for RuntimeDir {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0.join("network.sock"));
        let _ = fs::remove_dir(&self.0);
    }
}

impl Network {
    pub(super) fn open(
        owner: ProcessIdentity,
        status: SharedStatus,
        stop: &AtomicBool,
    ) -> Result<Self, String> {
        let helper = helper_path().ok_or(
            "The VRCS network helper is not installed. Install the matching vrcs-network-helper package",
        )?;
        let (runtime, socket_path) = runtime_path()?;
        let listener = UnixListener::bind(&socket_path)
            .map_err(|error| format!("Cannot create network helper socket: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| error.to_string())?;
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut child = Command::new("/usr/bin/pkexec")
            .arg("--disable-internal-agent")
            .arg(&helper)
            .arg("--socket")
            .arg(&socket_path)
            .arg("--pid")
            .arg(owner.pid.to_string())
            .arg("--starttime")
            .arg(owner.created.to_string())
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            // Keep helper and pkexec diagnostics visible in the terminal that
            // launched `npm run dev`; successful capture writes nothing here.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("Cannot start the network helper: {error}"))?;
        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            stdin
                .write_all(token.as_bytes())
                .map_err(|error| format!("Cannot authorize the network helper: {error}"))?;
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        let stream = loop {
            if stop.load(Ordering::Relaxed) {
                let _ = child.kill();
                return Err("Player audio stopped".into());
            }
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(state) = child.try_wait().map_err(|error| error.to_string())? {
                        return Err(pkexec_failure(state.code()));
                    }
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        return Err("Timed out waiting for network capture authorization".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("Cannot accept the network helper: {error}")),
            }
        };
        if stop.load(Ordering::Relaxed) {
            let _ = child.kill();
            return Err("Player audio stopped".into());
        }
        drop(listener);
        verify_root_peer(&stream)?;
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .map_err(|error| error.to_string())?;
        let mut hello = stream;
        let Some(Event::Hello {
            version,
            token: received,
            pid,
            starttime,
            monotonic_us,
        }) = read_event(&mut hello).map_err(|error| error.to_string())?
        else {
            return Err("The network helper did not identify itself".into());
        };
        if version != player_audio_helper::VERSION {
            return Err("The installed network helper is incompatible with this VRCS build".into());
        }
        if received != token || pid != owner.pid || starttime != owner.created {
            return Err("The network helper session does not match this VRChat process".into());
        }
        hello
            .set_read_timeout(None)
            .map_err(|error| error.to_string())?;
        hello
            .set_write_timeout(Some(Duration::from_secs(2)))
            .map_err(|error| error.to_string())?;
        let writer = hello.try_clone().map_err(|error| error.to_string())?;
        let mut reader_stream = hello;
        let (packet_tx, packets) = mpsc::sync_channel(8192);
        let waiter = Arc::new(Mutex::new(None::<Waiter>));
        let fatal = Arc::new(Mutex::new(None::<String>));
        let thread_waiter = Arc::clone(&waiter);
        let thread_fatal = Arc::clone(&fatal);
        let thread_status = Arc::clone(&status);
        let anchor = Instant::now();
        let reader = std::thread::Builder::new()
            .name("vrcs-linux-network".into())
            .spawn(move || {
                loop {
                    match read_event(&mut reader_stream) {
                        Ok(Some(Event::Packet(packet))) => {
                            let at = anchor
                                + Duration::from_micros(
                                    packet.monotonic_us.saturating_sub(monotonic_us),
                                );
                            let datagram = Datagram {
                                connection: Connection {
                                    local: std::net::SocketAddrV4::new(
                                        packet.local_ip.into(),
                                        packet.local_port,
                                    ),
                                    remote: std::net::SocketAddrV4::new(
                                        packet.remote_ip.into(),
                                        packet.remote_port,
                                    ),
                                    challenge: packet.challenge,
                                },
                                incoming: packet.incoming,
                                payload: packet.payload,
                                at,
                            };
                            if packet_tx.try_send(datagram).is_err() {
                                // A full queue is a slow consumer, not a reason to
                                // accept packets later at the wrong timeline position.
                                continue;
                            }
                        }
                        Ok(Some(Event::ScanProgress(progress))) => {
                            let mut state = thread_status.lock().expect("player status");
                            state.key_scan = Some(super::KeyScanProgress {
                                stage: "scanning",
                                scanned_bytes: progress.scanned,
                                total_bytes: progress.total,
                                candidates_tested: progress.candidates,
                                elapsed_ms: progress.elapsed_ms,
                                scan_elapsed_ms: progress.elapsed_ms,
                                workers: 1,
                            });
                        }
                        Ok(Some(Event::ScanResult(result))) => {
                            let mut pending = thread_waiter.lock().expect("scan waiter");
                            if pending
                                .as_ref()
                                .is_some_and(|wait| wait.request == result.request)
                            {
                                if let Some(wait) = pending.take() {
                                    let _ = wait.sender.try_send(result);
                                }
                            }
                        }
                        Ok(Some(Event::Status { fatal, detail })) => {
                            if fatal {
                                *thread_fatal.lock().expect("helper fatal") = Some(detail);
                                thread_waiter.lock().expect("scan waiter").take();
                                return;
                            }
                            thread_status.lock().expect("player status").detail = Some(detail);
                        }
                        Ok(Some(Event::Hello { .. })) | Err(_) | Ok(None) => {
                            *thread_fatal.lock().expect("helper fatal") =
                                Some("The network helper connection closed".into());
                            thread_waiter.lock().expect("scan waiter").take();
                            return;
                        }
                    }
                }
            })
            .map_err(|error| error.to_string())?;
        Ok(Self {
            child: Some(child),
            writer,
            reader: Some(reader),
            packets,
            waiter,
            fatal,
            next_request: AtomicU64::new(1),
            _runtime: runtime,
        })
    }

    pub(super) fn refresh(&mut self) -> Result<(), String> {
        if let Some(detail) = self.fatal.lock().expect("helper fatal").as_ref() {
            return Err(detail.clone());
        }
        if let Some(status) = self
            .child
            .as_mut()
            .expect("helper child")
            .try_wait()
            .map_err(|error| error.to_string())?
        {
            return Err(pkexec_failure(status.code()));
        }
        Ok(())
    }

    pub(super) fn receive(&self, mut callback: impl FnMut(Datagram)) -> Result<(), String> {
        if let Some(detail) = self.fatal.lock().expect("helper fatal").take() {
            return Err(detail);
        }
        loop {
            match self.packets.try_recv() {
                Ok(packet) => callback(packet),
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("The network helper connection closed".into())
                }
            }
        }
    }

    pub(super) fn acquire_key(
        &self,
        _config: &crate::config::PlayerAudioConfig,
        packets: &[Vec<u8>],
        stop: &AtomicBool,
        _status: &SharedStatus,
    ) -> Result<zeroize::Zeroizing<[u8; 32]>, String> {
        let mut writer = self.writer.try_clone().map_err(|error| error.to_string())?;
        let request = self.next_request.fetch_add(1, Ordering::Relaxed);
        let (sender, receiver) = mpsc::sync_channel(1);
        {
            let mut pending = self.waiter.lock().expect("scan waiter");
            if pending.is_some() {
                return Err("A network key scan is already running".into());
            }
            *pending = Some(Waiter { request, sender });
        }
        if let Err(error) = write_command(
            &mut writer,
            &HelperCommand::Scan {
                request,
                packets: packets.to_vec(),
            },
        ) {
            self.waiter.lock().expect("scan waiter").take();
            return Err(format!("Cannot request a network key scan: {error}"));
        }
        let deadline = Instant::now() + Duration::from_secs(45);
        loop {
            if stop.load(Ordering::Relaxed) {
                self.cancel(request);
                return Err("Player audio stopped".into());
            }
            if let Some(detail) = self.fatal.lock().expect("helper fatal").as_ref() {
                self.waiter.lock().expect("scan waiter").take();
                return Err(detail.clone());
            }
            if Instant::now() >= deadline {
                self.cancel(request);
                return Err("Timed out waiting for the network key scan".into());
            }
            match receiver.recv_timeout(Duration::from_millis(100)) {
                Ok(result) => {
                    return result.key.map(zeroize::Zeroizing::new).ok_or_else(|| {
                        result
                            .error
                            .unwrap_or_else(|| "No key authenticated the current session".into())
                    })
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    self.waiter.lock().expect("scan waiter").take();
                    return Err(self
                        .fatal
                        .lock()
                        .expect("helper fatal")
                        .clone()
                        .unwrap_or_else(|| {
                            "The network helper stopped during key acquisition".into()
                        }));
                }
            }
        }
    }

    fn cancel(&self, request: u64) {
        self.waiter.lock().expect("scan waiter").take();
        if let Ok(mut writer) = self.writer.try_clone() {
            let _ = write_command(&mut writer, &HelperCommand::Cancel { request });
        }
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        if let Ok(mut writer) = self.writer.try_clone() {
            let _ = write_command(&mut writer, &HelperCommand::Shutdown);
        }
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let Some(mut child) = self.child.take() else {
            return;
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25))
                }
                _ => {
                    if let Err(error) = child.kill() {
                        tracing::warn!(%error, "network helper did not exit after shutdown");
                    }
                    // A user process cannot always signal its root helper. Reap
                    // it when it exits without blocking capture_stop forever.
                    let _ = std::thread::Builder::new()
                        .name("vrcs-helper-reaper".into())
                        .spawn(move || {
                            let _ = child.wait();
                        });
                    break;
                }
            }
        }
    }
}

fn runtime_path() -> Result<(RuntimeDir, PathBuf), String> {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from(format!("/run/user/{}", unsafe { libc::geteuid() })));
    if !base.is_dir() {
        return Err("No private runtime directory is available for network capture".into());
    }
    let directory = base.join(format!("vrcs-network-{}", uuid::Uuid::new_v4().simple()));
    let mut builder = fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(&directory)
        .map_err(|error| format!("Cannot create network runtime directory: {error}"))?;
    let socket = directory.join("network.sock");
    Ok((RuntimeDir(directory), socket))
}

fn verify_root_peer(stream: &UnixStream) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 || credentials.uid != 0 {
        return Err("The network helper did not run with root privileges".into());
    }
    Ok(())
}

fn pkexec_failure(code: Option<i32>) -> String {
    match code {
        Some(126) => "Network capture authorization was cancelled".into(),
        Some(127) => "Network capture authorization failed; check polkit and the graphical authentication agent".into(),
        Some(code) => format!("The network helper exited ({code})"),
        None => "The network helper exited".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_acquisition_ends_when_helper_fails_with_a_request_pending() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = directory.path().join("network");
        fs::create_dir(&runtime).unwrap();
        let (writer, _peer) = UnixStream::pair().unwrap();
        let (_packets, packets) = mpsc::sync_channel(1);
        let network = Network {
            child: Some(Command::new("/usr/bin/true").spawn().unwrap()),
            writer,
            reader: None,
            packets,
            waiter: Arc::new(Mutex::new(None)),
            fatal: Arc::new(Mutex::new(Some("fixture helper disconnected".into()))),
            next_request: AtomicU64::new(1),
            _runtime: RuntimeDir(runtime),
        };
        let stop = Arc::new(AtomicBool::new(false));
        let thread_stop = Arc::clone(&stop);
        let (sender, receiver) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let result =
                network.acquire_key(&Default::default(), &[], &thread_stop, &Default::default());
            sender.send(result).unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_secs(1));
        stop.store(true, Ordering::Relaxed);
        thread.join().unwrap();
        assert_eq!(result.unwrap().unwrap_err(), "fixture helper disconnected");
    }
}
