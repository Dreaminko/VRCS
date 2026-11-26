//! Linux network capture through a session-scoped pkexec helper. Core never
//! receives raw packet sockets, game memory, or unauthenticated key candidates.
use std::fs;
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::{Duration, Instant};

use super::helper::{
    Command as HelperCommand, Event, ScanResult, VERSION, procfs, read_event, write_command,
};

use super::SharedStatus;
use super::connection::{Connection, Datagram};
use crate::vrchat_process::ProcessIdentity;

const HELPER_PATHS: [&str; 2] = ["/usr/bin/vrcs-desktop", "/usr/local/bin/vrcs-desktop"];
const AVAILABILITY_CACHE_TTL: Duration = Duration::from_secs(5);

static PKEXEC_ACTIVE: AtomicBool = AtomicBool::new(false);
static AVAILABILITY_CACHE: Mutex<Option<(Instant, PathBuf, bool)>> = Mutex::new(None);

fn clean_helper_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    command
        .env_remove("LD_LIBRARY_PATH")
        .env_remove("LD_PRELOAD")
        .env_remove("APPIMAGE");
    command
}

fn parse_helper_version(stdout: &str) -> Result<(), String> {
    let expected_protocol = format!("(protocol {VERSION})");
    if stdout.contains(&expected_protocol) {
        Ok(())
    } else {
        Err("The installed network helper is incompatible with this VRCS build".into())
    }
}

fn current_executable() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| fs::canonicalize(exe).ok())
}

fn probe_helper(path: &Path) -> Result<(), String> {
    if current_executable().is_some_and(|exe| exe == path) {
        return Ok(());
    }
    let version = clean_helper_command(path)
        .args(["--network-helper", "--version"])
        .output()
        .map_err(|error| format!("Cannot run the network helper: {error}"))?;
    if !version.status.success() {
        return Err("The installed network helper is incompatible with this VRCS build".into());
    }
    parse_helper_version(&String::from_utf8_lossy(&version.stdout))
}

pub(crate) fn network_available() -> bool {
    if !Path::new("/usr/bin/pkexec").is_file() {
        return false;
    }
    let Some(helper) = helper_path() else {
        return false;
    };
    if let Ok(guard) = AVAILABILITY_CACHE.lock()
        && let Some((checked_at, cached_path, available)) = &*guard
        && checked_at.elapsed() < AVAILABILITY_CACHE_TTL
        && cached_path == &helper
    {
        return *available;
    }
    let available = probe_helper(&helper).is_ok();
    if let Ok(mut guard) = AVAILABILITY_CACHE.lock() {
        *guard = Some((Instant::now(), helper, available));
    }
    available
}

fn helper_path() -> Option<PathBuf> {
    current_executable()
        .into_iter()
        .chain(HELPER_PATHS.iter().map(PathBuf::from))
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
    wake: Arc<(Mutex<bool>, Condvar)>,
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
    active: Option<&'static AtomicBool>,
}

impl HelperChild {
    #[cfg(test)]
    fn spawn(command: &mut Command) -> Result<Self, String> {
        Self::spawn_with_flag(command, None)
    }

    fn spawn_with_flag(
        command: &mut Command,
        active: Option<&'static AtomicBool>,
    ) -> Result<Self, String> {
        let (reaper, receiver) = mpsc::channel::<Child>();
        std::thread::Builder::new()
            .name("vrcs-helper-reaper".into())
            .spawn(move || {
                if let Ok(mut child) = receiver.recv() {
                    let _ = child.wait();
                    if let Some(flag) = active {
                        flag.store(false, Ordering::Release);
                    }
                }
            })
            .map_err(|error| {
                if let Some(flag) = active {
                    flag.store(false, Ordering::Release);
                }
                format!("Cannot start the helper reaper: {error}")
            })?;
        let process = match command.spawn() {
            Ok(process) => process,
            Err(error) => {
                if let Some(flag) = active {
                    flag.store(false, Ordering::Release);
                }
                return Err(format!("Cannot start the network helper: {error}"));
            }
        };
        Ok(Self {
            process: Some(process),
            reaper,
            active,
        })
    }

    fn process(&mut self) -> &mut Child {
        self.process.as_mut().expect("helper child")
    }

    fn try_wait_and_clear(&mut self) -> Result<Option<std::process::ExitStatus>, std::io::Error> {
        let Some(child) = self.process.as_mut() else {
            return Ok(None);
        };
        let status = child.try_wait()?;
        if status.is_some() {
            self.process.take();
            if let Some(flag) = self.active.take() {
                flag.store(false, Ordering::Release);
            }
        }
        Ok(status)
    }
}

impl Drop for HelperChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.process.take() {
            // pkexec may still be waiting for authorization. Once privileged,
            // the helper instead exits on IPC shutdown; kill may return EPERM.
            let _ = child.kill();
            let _ = self.reaper.send(child);
        } else if let Some(flag) = self.active.take() {
            flag.store(false, Ordering::Release);
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
            "Linux network capture requires a root-installed VRCS system package (/usr/bin/vrcs-desktop) with /usr/bin/pkexec",
        )?;
        probe_helper(&helper)?;
        if !Path::new("/usr/bin/pkexec").is_file() {
            return Err(
                "Cannot authorize network capture: /usr/bin/pkexec is not installed".into(),
            );
        }
        let wait_deadline = Instant::now() + Duration::from_millis(500);
        while PKEXEC_ACTIVE
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            if stop.load(Ordering::Relaxed) {
                return Err("Player audio stopped".into());
            }
            if Instant::now() >= wait_deadline {
                return Err("A network capture authorization prompt is already pending".into());
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        let (runtime, socket_path) = match runtime_path() {
            Ok(pair) => pair,
            Err(error) => {
                PKEXEC_ACTIVE.store(false, Ordering::Release);
                return Err(error);
            }
        };
        let listener = match UnixListener::bind(&socket_path)
            .and_then(|listener| listener.set_nonblocking(true).map(|()| listener))
        {
            Ok(listener) => listener,
            Err(error) => {
                PKEXEC_ACTIVE.store(false, Ordering::Release);
                return Err(format!("Cannot create network helper socket: {error}"));
            }
        };
        let token = format!(
            "{}{}",
            uuid::Uuid::new_v4().simple(),
            uuid::Uuid::new_v4().simple()
        );
        let mut command = clean_helper_command("/usr/bin/pkexec");
        command
            .arg("--disable-internal-agent")
            .arg(&helper)
            .arg("--network-helper")
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
            .stderr(Stdio::inherit());
        let mut child = HelperChild::spawn_with_flag(&mut command, Some(&PKEXEC_ACTIVE))?;
        if let Some(mut stdin) = child.process().stdin.take() {
            use std::io::Write;
            if let Err(error) = stdin.write_all(token.as_bytes()) {
                if error.kind() == std::io::ErrorKind::BrokenPipe
                    && let Some(state) = child
                        .try_wait_and_clear()
                        .map_err(|wait_err| wait_err.to_string())?
                {
                    return Err(helper_failure(state.code()));
                }
                return Err(format!("Cannot authorize the network helper: {error}"));
            }
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
                        .try_wait_and_clear()
                        .map_err(|error| error.to_string())?
                    {
                        return Err(helper_failure(state.code()));
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
        verify_helper_peer(&stream)?;
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
        if version != VERSION {
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
        let wake = Arc::new((Mutex::new(false), Condvar::new()));
        let waiter = Arc::new(Mutex::new(None::<Waiter>));
        let fatal = Arc::new(Mutex::new(None::<String>));
        let thread_wake = Arc::clone(&wake);
        let thread_waiter = Arc::clone(&waiter);
        let thread_fatal = Arc::clone(&fatal);
        let thread_status = Arc::clone(&status);
        let anchor = Instant::now();
        let reader = std::thread::Builder::new()
            .name("vrcs-linux-network".into())
            .spawn(move || {
                let notify = || {
                    let (lock, cvar) = &*thread_wake;
                    *lock.lock().expect("network wake") = true;
                    cvar.notify_one();
                };
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
                            notify();
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
                            notify();
                        }
                        Ok(Some(Event::ScanResult(result))) => {
                            tracing::debug!(
                                request = result.request,
                                scanned = result.scanned,
                                candidates = result.candidates,
                                elapsed_ms = result.elapsed_ms,
                                matched = result.key.is_some(),
                                "Linux network helper key scan completed"
                            );
                            let mut pending = thread_waiter.lock().expect("scan waiter");
                            if pending
                                .as_ref()
                                .is_some_and(|wait| wait.request == result.request)
                                && let Some(wait) = pending.take()
                            {
                                let _ = wait.sender.try_send(result);
                            }
                            notify();
                        }
                        Ok(Some(Event::Status { fatal, detail })) => {
                            if fatal {
                                *thread_fatal.lock().expect("helper fatal") = Some(detail);
                                thread_waiter.lock().expect("scan waiter").take();
                                notify();
                                return;
                            }
                            thread_status.lock().expect("player status").detail = Some(detail);
                        }
                        Ok(Some(Event::Hello { .. })) | Err(_) | Ok(None) => {
                            *thread_fatal.lock().expect("helper fatal") =
                                Some("The network helper connection closed".into());
                            thread_waiter.lock().expect("scan waiter").take();
                            notify();
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
            wake,
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
            return Err(helper_failure(status.code()));
        }
        Ok(())
    }

    pub(super) fn wait(&self, timeout: Duration) {
        if timeout.is_zero() {
            return;
        }
        let (lock, cvar) = &*self.wake;
        let mut pending = lock.lock().expect("network wake");
        if !*pending {
            let (guard, _) = cvar.wait_timeout(pending, timeout).expect("network wake");
            pending = guard;
        }
        *pending = false;
    }

    pub(super) fn receive(&self, mut callback: impl FnMut(Datagram)) -> Result<(), String> {
        *self.wake.0.lock().expect("network wake") = false;
        if let Some(detail) = self.fatal.lock().expect("helper fatal").take() {
            return Err(detail);
        }
        loop {
            match self.packets.try_recv() {
                Ok(packet) => callback(packet),
                Err(mpsc::TryRecvError::Empty) => return Ok(()),
                Err(mpsc::TryRecvError::Disconnected) => {
                    return Err("The network helper connection closed".into());
                }
            }
        }
    }

    pub(super) fn start_key_scan(&self, packets: &[Vec<u8>]) -> Result<PendingScan, String> {
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
        Ok(PendingScan {
            request,
            receiver,
            deadline: Instant::now() + Duration::from_secs(45),
        })
    }

    pub(super) fn poll_key_scan(
        &self,
        scan: &PendingScan,
    ) -> Option<Result<zeroize::Zeroizing<[u8; 32]>, String>> {
        if let Some(detail) = self.fatal.lock().expect("helper fatal").as_ref() {
            self.waiter.lock().expect("scan waiter").take();
            return Some(Err(detail.clone()));
        }
        match scan.receiver.try_recv() {
            Ok(result) => Some(result.key.ok_or_else(|| {
                result
                    .error
                    .unwrap_or_else(|| "No key authenticated the current session".into())
            })),
            Err(mpsc::TryRecvError::Empty) => {
                if Instant::now() >= scan.deadline {
                    self.cancel(scan.request);
                    Some(Err("Timed out waiting for the network key scan".into()))
                } else {
                    None
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.waiter.lock().expect("scan waiter").take();
                Some(Err(self
                    .fatal
                    .lock()
                    .expect("helper fatal")
                    .clone()
                    .unwrap_or_else(|| {
                        "The network helper stopped during key acquisition".into()
                    })))
            }
        }
    }

    pub(super) fn cancel_key_scan(&self, scan: PendingScan) {
        self.cancel(scan.request);
    }

    fn cancel(&self, request: u64) {
        self.waiter.lock().expect("scan waiter").take();
        if let Ok(mut writer) = self.writer.try_clone() {
            let _ = write_command(&mut writer, &HelperCommand::Cancel { request });
        }
    }
}

pub(super) struct PendingScan {
    request: u64,
    receiver: mpsc::Receiver<ScanResult>,
    deadline: Instant,
}

impl Drop for Network {
    fn drop(&mut self) {
        // Send Shutdown reliably within a short timeout so the helper can zeroize
        // key material before HelperChild::drop kills any remaining process.
        let _ = self
            .writer
            .set_write_timeout(Some(Duration::from_millis(100)));
        let _ = write_command(&mut self.writer, &HelperCommand::Shutdown);
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
        for _ in 0..5 {
            if matches!(self.child.process().try_wait(), Ok(Some(_))) {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
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

fn verify_helper_peer(stream: &UnixStream) -> Result<(), String> {
    let Ok((_, uid, _)) = procfs::peer_credentials(stream) else {
        return Err("The network helper credentials could not be verified".into());
    };
    if uid != 0 {
        return Err("The network helper did not run with root privileges".into());
    }
    Ok(())
}

fn helper_failure(code: Option<i32>) -> String {
    match code {
        Some(126) => "Network capture authorization was cancelled".into(),
        Some(127) => {
            "Network capture authorization failed; check polkit and the graphical authentication agent".into()
        }
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
        let pending = network.start_key_scan(&[]).unwrap();
        let result = network.poll_key_scan(&pending);
        assert_eq!(result.unwrap().unwrap_err(), "fixture helper disconnected");
    }

    #[test]
    fn parses_helper_version_for_matching_and_mismatched_protocols() {
        assert_eq!(
            parse_helper_version(&format!("vrcs-network-helper 0.1.0 (protocol {VERSION})\n")),
            Ok(())
        );
        assert!(parse_helper_version("vrcs-network-helper 0.1.0 (protocol 9999)\n").is_err());
    }
}
