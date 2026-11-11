mod capture;
mod procfs;
mod scan;

use std::io::{self, Read};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zeroize::Zeroize;

use player_audio_helper::{
    read_command, write_event, Command, Event, ScanProgress, ScanResult, VERSION,
};

fn main() {
    if let Err(error) = run() {
        eprintln!("vrcs-network-helper: {error}");
        std::process::exit(1);
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

fn run() -> Result<(), String> {
    if std::env::args().nth(1).as_deref() == Some("--version") {
        println!(
            "vrcs-network-helper {} (protocol {VERSION})",
            env!("CARGO_PKG_VERSION")
        );
        return Ok(());
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err("must run as root through pkexec".into());
    }
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
    let mut args = std::env::args().skip(1);
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
    let expected_uid = std::env::var("PKEXEC_UID")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .ok_or("missing or invalid PKEXEC_UID")?;
    procfs::parent_is_private(&socket, Some(expected_uid))?;

    let mut token = String::new();
    io::stdin()
        .read_to_string(&mut token)
        .map_err(|error| format!("cannot read authorization token: {error}"))?;
    let token = token.trim().to_string();
    if token.is_empty() || token.len() > 128 {
        return Err("invalid authorization token".into());
    }
    let stream =
        UnixStream::connect(&socket).map_err(|error| format!("cannot connect to VRCS: {error}"))?;
    verify_peer(&stream, expected_uid)?;
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
        Ok(capture) => capture,
        Err(error) => {
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
        if let Err(error) = capture.run(&packet_writer) {
            packet_stop.store(true, Ordering::Relaxed);
            let _ = send(
                &packet_writer,
                Event::Status {
                    fatal: true,
                    detail: error,
                },
            );
            let _ = packet_shutdown.shutdown(Shutdown::Both);
        }
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
                            key: outcome.key.as_deref().copied(),
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

fn send(writer: &Mutex<UnixStream>, mut event: Event) -> Result<(), String> {
    let result = writer
        .lock()
        .map_err(|_| "helper writer lock failed".to_string())
        .and_then(|mut writer| {
            write_event(&mut *writer, &event).map_err(|error| error.to_string())
        });
    if let Event::ScanResult(result) = &mut event {
        if let Some(key) = &mut result.key {
            key.zeroize();
        }
    }
    result
}

fn verify_peer(stream: &UnixStream, expected_uid: u32) -> Result<(), String> {
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
    if result != 0 {
        return Err("cannot verify the VRCS peer".into());
    }
    if credentials.uid != expected_uid {
        return Err("VRCS peer uid does not match pkexec caller".into());
    }
    Ok(())
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
}
