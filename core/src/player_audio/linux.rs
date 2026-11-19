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

use player_audio_helper::{
    procfs, read_event, write_command, Command as HelperCommand, Event, ScanResult,
};

use super::connection::{Connection, Datagram};
use super::SharedStatus;
use crate::vrchat_process::ProcessIdentity;

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

struct Waiter {
    request: u64,
    sender: mpsc::SyncSender<ScanResult>,
}

pub(super) struct Network {
    child: HelperChild,
    writer: UnixStream,
    reader: Option<std::thread::JoinHandle<()>>,
    packets: mpsc::Receiver<Datagram>,
    waiter: Arc<Mutex<Option<Waiter>>>,
    fatal: Arc<Mutex<Option<String>>>,
    next_request: AtomicU64,
    _runtime: RuntimeDir,
}

/// Establish the reaper before spawning pkexec, so every later error path can
/// hand off the child without depending on another thread being available.
struct HelperChild {
    process: Option<Child>,
    reaper: mpsc::Sender<Child>,
}

impl HelperChild {
    fn spawn(command: &mut Command) -> Result<Self, String> {
        let (reaper, receiver) = mpsc::channel::<Child>();
        std::thread::Builder::new()
            .name("vrcs-helper-reaper".into())
            .spawn(move || {
                if let Ok(mut child) = receiver.recv() {
                    let _ = child.wait();
                }
            })
            .map_err(|error| format!("Cannot start the helper reaper: {error}"))?;
        let process = command
            .spawn()
            .map_err(|error| format!("Cannot start the network helper: {error}"))?;
        Ok(Self {
            process: Some(process),
            reaper,
        })
    }

    fn process(&mut self) -> &mut Child {
        self.process.as_mut().expect("helper child")
    }
}

impl Drop for HelperChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.take() {
            // pkexec may still be waiting for authorization. Once privileged,
            // the helper instead exits on IPC shutdown; kill may return EPERM.
            let _ = child.kill();
            let _ = self.reaper.send(child);
        }
    }
}

struct StartupReader<'a> {
    stream: &'a mut UnixStream,
    stop: &'a AtomicBool,
    owner: ProcessIdentity,
    deadline: Instant,
}

impl std::io::Read for StartupReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        loop {
            if self.stop.load(Ordering::Relaxed)
                || procfs::exited(self.owner.pid, self.owner.created)
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::ConnectionAborted,
                    "Network helper startup cancelled",
                ));
            }
            if Instant::now() >= self.deadline {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "Timed out reading the network helper handshake",
                ));
            }
            match std::io::Read::read(self.stream, buffer) {
                Err(error)
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                result => return result,
            }
        }
    }
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
        let mut child = HelperChild::spawn(
            Command::new("/usr/bin/pkexec")
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
                .stderr(Stdio::inherit()),
        )?;
        if let Some(mut stdin) = child.process().stdin.take() {
            use std::io::Write;
            stdin
                .write_all(token.as_bytes())
                .map_err(|error| format!("Cannot authorize the network helper: {error}"))?;
        }
        let deadline = Instant::now() + Duration::from_secs(120);
        let stream = loop {
            if stop.load(Ordering::Relaxed) {
                return Err("Player audio stopped".into());
            }
            if procfs::exited(owner.pid, owner.created) {
                return Err("VRChat exited before network helper startup".into());
            }
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Some(state) = child
                        .process()
                        .try_wait()
                        .map_err(|error| error.to_string())?
                    {
                        return Err(pkexec_failure(state.code()));
                    }
                    if Instant::now() >= deadline {
                        return Err("Timed out waiting for network capture authorization".into());
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(error) => return Err(format!("Cannot accept the network helper: {error}")),
            }
        };
        if stop.load(Ordering::Relaxed) {
            return Err("Player audio stopped".into());
        }
        drop(listener);
        verify_root_peer(&stream)?;
        stream
            .set_read_timeout(Some(Duration::from_millis(100)))
            .map_err(|error| error.to_string())?;
        let mut hello = stream;
        let Some(Event::Hello {
            version,
            token: received,
            pid,
            starttime,
            monotonic_us,
        }) = read_event(&mut StartupReader {
            stream: &mut hello,
            stop,
            owner,
            deadline: Instant::now() + Duration::from_secs(5),
        })
        .map_err(|error| error.to_string())?
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
            child,
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
            .process()
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
        // Best effort only: stopping capture must not wait behind a helper
        // that no longer reads commands or is stuck in a kernel memory read.
        let _ = self.writer.set_nonblocking(true);
        let _ = write_command(&mut self.writer, &HelperCommand::Shutdown);
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        // The socket shutdown wakes the reader. Dropping its handle detaches it
        // rather than making user stop depend on its scheduling or status lock.
        self.reader.take();
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
    let Ok((_, uid, _)) = procfs::peer_credentials(stream) else {
        return Err("The network helper credentials could not be verified".into());
    };
    if uid != 0 {
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
    fn abandoning_helper_startup_kills_and_reaps_the_child() {
        let mut child = HelperChild::spawn(Command::new("/usr/bin/sleep").arg("30")).unwrap();
        let pid = child.process().id();
        let status = PathBuf::from(format!("/proc/{pid}/stat"));
        assert!(status.exists());
        drop(child);
        let deadline = Instant::now() + Duration::from_secs(2);
        while status.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !status.exists(),
            "helper must be killed and reaped on startup failure"
        );
    }

    #[test]
    fn key_acquisition_ends_when_helper_fails_with_a_request_pending() {
        let directory = tempfile::tempdir().unwrap();
        let runtime = directory.path().join("network");
        fs::create_dir(&runtime).unwrap();
        let (writer, _peer) = UnixStream::pair().unwrap();
        let (_packets, packets) = mpsc::sync_channel(1);
        let network = Network {
            child: HelperChild::spawn(&mut Command::new("/usr/bin/true")).unwrap(),
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
