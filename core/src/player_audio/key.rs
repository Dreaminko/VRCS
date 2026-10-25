//! Native key acquisition; no key logging, files, or process-memory reads.
use super::ntfs::{Ntfs, Run};
use super::KeyScanProgress;
use player_audio_scan::{scan_candidates, ScanAbort, ScanBuffer, ScanControl};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use zeroize::Zeroizing;

const MIB: u64 = 1024 * 1024;
const SCAN_CHUNK: u64 = 8 * MIB;
const SCAN_TIMEOUT: Duration = Duration::from_secs(45);

#[cfg(test)]
pub(super) fn scan_bytes(
    data: &[u8],
    packets: &[Vec<u8>],
    stop: &AtomicBool,
) -> Option<Zeroizing<[u8; 32]>> {
    scan_candidates(
        data,
        packets,
        &mut ScanControl {
            stop,
            deadline: Instant::now() + SCAN_TIMEOUT,
            candidates: 0,
        },
    )
    .ok()
    .flatten()
}

enum ScanMessage {
    Chunk {
        bytes: u64,
        candidates: u64,
        key: Option<Zeroizing<[u8; 32]>>,
    },
    Failed(String),
}

fn scan_pagefile<R: std::io::Read + std::io::Seek + Send>(
    readers: Vec<Ntfs<R>>,
    file_size: u64,
    extents: &[Run],
    packets: &[Vec<u8>],
    stop: &AtomicBool,
    started: Instant,
    mut report: impl FnMut(KeyScanProgress),
) -> Result<Zeroizing<[u8; 32]>, String> {
    let scan_started = Instant::now();
    let deadline = scan_started + SCAN_TIMEOUT;
    let next = AtomicU64::new(0);
    let finished = AtomicBool::new(false);
    let mut progress = KeyScanProgress {
        stage: "scanning",
        scanned_bytes: 0,
        total_bytes: file_size,
        candidates_tested: 0,
        elapsed_ms: started.elapsed().as_millis() as u64,
        scan_elapsed_ms: 0,
        workers: readers.len(),
    };
    report(progress.clone());
    let (tx, rx) = std::sync::mpsc::sync_channel(readers.len().max(1) * 2);
    let mut found = None;
    let mut failure = None;
    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for mut reader in readers {
            let tx = tx.clone();
            let next = &next;
            let finished = &finished;
            handles.push(scope.spawn(move || {
                let mut output = ScanBuffer::default();
                let mut scratch = ScanBuffer::default();
                let mut control = ScanControl {
                    stop,
                    deadline,
                    candidates: 0,
                };
                let result = (|| {
                    while !finished.load(Ordering::Relaxed) {
                        control.check().map_err(scan_abort)?;
                        let at = next.fetch_add(SCAN_CHUNK, Ordering::Relaxed);
                        if at >= file_size {
                            break;
                        }
                        let end = at.saturating_add(SCAN_CHUNK).min(file_size);
                        // Include 39 bytes of overlap to find a 40-byte object
                        // crossing a worker, sector, or physical extent boundary.
                        let begin = at.saturating_sub(39);
                        let bytes = reader.read_runs_into(
                            extents,
                            begin,
                            (end - begin) as usize,
                            output.bytes(),
                            scratch.bytes(),
                        )?;
                        let before = control.candidates;
                        let key =
                            scan_candidates(bytes, packets, &mut control).map_err(scan_abort)?;
                        if key.is_some() {
                            finished.store(true, Ordering::Relaxed);
                        }
                        if tx
                            .send(ScanMessage::Chunk {
                                bytes: end - at,
                                candidates: control.candidates - before,
                                key,
                            })
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok::<(), String>(())
                })();
                if let Err(error) = result {
                    finished.store(true, Ordering::Relaxed);
                    let _ = tx.send(ScanMessage::Failed(error));
                }
            }));
        }
        drop(tx);
        for message in rx {
            match message {
                ScanMessage::Chunk {
                    bytes,
                    candidates,
                    key,
                } => {
                    progress.scanned_bytes += bytes;
                    progress.candidates_tested += candidates;
                    if found.is_none() {
                        found = key;
                    }
                }
                ScanMessage::Failed(error) => {
                    failure.get_or_insert(error);
                }
            }
            progress.elapsed_ms = started.elapsed().as_millis() as u64;
            progress.scan_elapsed_ms = scan_started.elapsed().as_millis() as u64;
            report(progress.clone());
        }
        for handle in handles {
            if handle.join().is_err() {
                failure.get_or_insert("Pagefile scan worker failed".into());
            }
        }
    });
    // Count secure buffer cleanup and worker joins in the displayed total time.
    progress.elapsed_ms = started.elapsed().as_millis() as u64;
    progress.scan_elapsed_ms = scan_started.elapsed().as_millis() as u64;
    if stop.load(Ordering::Relaxed) {
        return Err("Player audio stopped".into());
    }
    if let Some(key) = found {
        progress.stage = "verified";
        report(progress);
        return Ok(key);
    }
    report(progress.clone());
    Err(format!("{} ({} / {} MiB, {} candidates, {:.1} s total, {:.1} s scanning).",
        failure.unwrap_or_else(|| "No key authenticated the current session. Check the pagefile drive and Windows memory compression; retry after the world has loaded".into()),
        progress.scanned_bytes / MIB, file_size / MIB, progress.candidates_tested,
        progress.elapsed_ms as f64 / 1000.0, progress.scan_elapsed_ms as f64 / 1000.0,
    ))
}

fn scan_abort(error: ScanAbort) -> String {
    match error {
        ScanAbort::Cancelled => "Player audio stopped".into(),
        ScanAbort::Deadline => "Pagefile scan timed out after 45 s".into(),
    }
}

#[cfg(windows)]
pub(super) fn acquire(
    pid: u32,
    drive: &str,
    packets: &[Vec<u8>],
    stop: &AtomicBool,
    status: &super::SharedStatus,
) -> Result<Zeroizing<[u8; 32]>, String> {
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_QUOTA,
    };
    let started = Instant::now();
    let bytes = drive.as_bytes();
    if bytes.len() != 2 || !bytes[0].is_ascii_alphabetic() || bytes[1] != b':' {
        return Err("Invalid pagefile drive".into());
    }
    let open = || {
        let path = format!("\\\\.\\{drive}");
        // Same sequential-I/O hint as the reference; some volume drivers reject it.
        OpenOptions::new()
            .read(true)
            .share_mode(3)
            .custom_flags(0x0800_0000)
            .open(&path)
            .or_else(|_| OpenOptions::new().read(true).share_mode(3).open(&path))
            .map_err(|e| {
                format!("Cannot open pagefile volume; administrator access is required: {e}")
            })
    };
    let mut ntfs = Ntfs::new(open()?)?;
    let mut paging = KeyScanProgress {
        stage: "paging",
        scanned_bytes: 0,
        total_bytes: 0,
        candidates_tested: 0,
        elapsed_ms: 0,
        scan_elapsed_ms: 0,
        workers: 0,
    };
    status.lock().expect("player status").key_scan = Some(paging.clone());
    let pager = Pager::new();
    let baseline = pager.modified_pages();
    // Match the reference's two trims before scanning. The old full pre-scan
    // delayed newly paged keys and could exhaust the shared timeout entirely.
    for pass in 1..=2 {
        if stop.load(Ordering::Relaxed) {
            return Err("Player audio stopped".into());
        }
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_QUOTA,
                false,
                pid,
            )
        }
        .map_err(|e| format!("Cannot access the current VRChat working set: {e}"))?;
        let trimmed = unsafe { EmptyWorkingSet(handle) };
        let _ = unsafe { CloseHandle(handle) };
        trimmed.map_err(|e| format!("Cannot trim the current VRChat working set: {e}"))?;
        pager.drain(baseline, stop);
        wait_cancellable(stop, Duration::from_millis(200));
        paging.elapsed_ms = started.elapsed().as_millis() as u64;
        status.lock().expect("player status").key_scan = Some(paging.clone());
        tracing::info!(
            pass,
            elapsed_ms = paging.elapsed_ms,
            "player audio working-set paging completed"
        );
    }
    if stop.load(Ordering::Relaxed) {
        return Err("Player audio stopped".into());
    }
    // Refresh extents after paging; a system-managed pagefile may grow.
    let (file_size, extents) = ntfs.pagefile()?;
    let worker_count = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 8)
        .min(file_size.div_ceil(SCAN_CHUNK) as usize)
        .max(1);
    let mut readers = Vec::with_capacity(worker_count);
    for _ in 1..worker_count {
        readers.push(ntfs.with_reader(open()?));
    }
    readers.push(ntfs);
    let mut last_logged = Instant::now() - Duration::from_secs(5);
    scan_pagefile(
        readers,
        file_size,
        &extents,
        packets,
        stop,
        started,
        |progress| {
            if progress.stage == "verified" || last_logged.elapsed() >= Duration::from_secs(1) {
                tracing::info!(
                    stage = progress.stage,
                    scanned_mib = progress.scanned_bytes / MIB,
                    total_mib = progress.total_bytes / MIB,
                    elapsed_ms = progress.elapsed_ms,
                    scan_elapsed_ms = progress.scan_elapsed_ms,
                    workers = progress.workers,
                    candidates = progress.candidates_tested,
                    "player audio key acquisition progress"
                );
                last_logged = Instant::now();
            }
            status.lock().expect("player status").key_scan = Some(progress);
        },
    )
}

#[cfg(windows)]
type Query = unsafe extern "system" fn(u32, *mut std::ffi::c_void, u32, *mut u32) -> i32;
#[cfg(windows)]
type Set = unsafe extern "system" fn(u32, *const std::ffi::c_void, u32) -> i32;

#[cfg(windows)]
struct Pager {
    _library: Option<libloading::Library>,
    query: Option<Query>,
    set: Option<Set>,
}

#[cfg(windows)]
impl Pager {
    fn new() -> Self {
        let system = std::env::var_os("SystemRoot")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| "C:\\Windows".into());
        let library = unsafe { libloading::Library::new(system.join("System32/ntdll.dll")) }.ok();
        let query = library
            .as_ref()
            .and_then(|l| unsafe { l.get::<Query>(b"NtQuerySystemInformation\0") }.ok())
            .map(|s| *s);
        let set = library
            .as_ref()
            .and_then(|l| unsafe { l.get::<Set>(b"NtSetSystemInformation\0") }.ok())
            .map(|s| *s);
        Self {
            _library: library,
            query,
            set,
        }
    }
    fn modified_pages(&self) -> Option<usize> {
        let query = self.query?;
        let mut counts = [0usize; 22];
        let mut returned = 0;
        let result = unsafe {
            query(
                80,
                counts.as_mut_ptr().cast(),
                std::mem::size_of_val(&counts) as u32,
                &mut returned,
            )
        };
        (result == 0 && returned as usize >= 3 * std::mem::size_of::<usize>()).then_some(counts[2])
    }
    fn drain(&self, baseline: Option<usize>, stop: &AtomicBool) {
        let Some(baseline) = baseline else {
            wait_cancellable(stop, Duration::from_secs(2));
            return;
        };
        let started = Instant::now();
        let mut flush_reported = false;
        while started.elapsed() < Duration::from_secs(5) && !stop.load(Ordering::Relaxed) {
            if let Some(set) = self.set {
                let command = 3u32;
                let status = unsafe { set(80, (&command as *const u32).cast(), 4) };
                if status != 0 && !flush_reported {
                    tracing::debug!(
                        ntstatus = status,
                        "modified-page flush unavailable; waiting for Windows writeback"
                    );
                    flush_reported = true;
                }
            }
            // Check relative to the pre-trim baseline instead of waiting for
            // every unrelated application's dirty pages to drain to zero.
            wait_cancellable(stop, Duration::from_millis(150));
            match self.modified_pages() {
                Some(current) if current <= baseline.saturating_add(8192) => break,
                Some(_) => {}
                None => {
                    wait_cancellable(stop, Duration::from_secs(2));
                    break;
                }
            }
            wait_cancellable(stop, Duration::from_millis(150));
        }
    }
}

#[cfg(windows)]
fn wait_cancellable(stop: &AtomicBool, duration: Duration) {
    let end = Instant::now() + duration;
    while !stop.load(Ordering::Relaxed) {
        let remaining = end.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        std::thread::sleep(remaining.min(Duration::from_millis(50)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::Arc;

    #[test]
    #[ignore = "8.3 GiB synthetic volume benchmark; no real pagefile or secrets"]
    fn benchmark_parallel_pagefile_8455_mib() {
        use std::io::{Read, Seek, SeekFrom};
        struct RepeatedVolume {
            data: Arc<[u8]>,
            at: u64,
            length: u64,
            last: [u8; 40],
        }
        impl Read for RepeatedVolume {
            fn read(&mut self, output: &mut [u8]) -> std::io::Result<usize> {
                let count = output
                    .len()
                    .min(self.length.saturating_sub(self.at) as usize);
                let mut copied = 0;
                while copied < count {
                    let offset = (self.at as usize + copied) % self.data.len();
                    let take = (count - copied).min(self.data.len() - offset);
                    output[copied..copied + take]
                        .copy_from_slice(&self.data[offset..offset + take]);
                    copied += take;
                }
                let last_start = self.length - 40;
                let begin = self.at.max(last_start);
                let end = self.at + count as u64;
                if begin < end {
                    output[(begin - self.at) as usize..count].copy_from_slice(
                        &self.last[(begin - last_start) as usize..(end - last_start) as usize],
                    );
                }
                self.at = end;
                Ok(count)
            }
        }
        impl Seek for RepeatedVolume {
            fn seek(&mut self, position: SeekFrom) -> std::io::Result<u64> {
                match position {
                    SeekFrom::Start(at) => {
                        self.at = at;
                        Ok(at)
                    }
                    _ => Err(std::io::ErrorKind::InvalidInput.into()),
                }
            }
        }
        let mut data = vec![0; 64 * MIB as usize];
        for (index, object) in data.as_chunks_mut::<512>().0.iter_mut().enumerate() {
            object[..8].copy_from_slice(&32u64.to_le_bytes());
            if index % 128 == 0 {
                for (i, byte) in object[8..40].iter_mut().enumerate() {
                    *byte = (i as u8).wrapping_add(index as u8);
                }
            } else {
                object[8..40].fill(9);
            }
        }
        let data: Arc<[u8]> = data.into();
        let key = std::array::from_fn(|i| (i as u8).wrapping_mul(7));
        let mut last = [0; 40];
        last[..8].copy_from_slice(&32u64.to_le_bytes());
        last[8..].copy_from_slice(&key);
        let packets = vec![
            super::super::crypto::seal(&key, 1, &[99; 512]),
            super::super::crypto::seal(&key, 2, &[99; 512]),
        ];
        let length = 8455 * MIB;
        let workers = std::env::var("VRCS_TEST_SCAN_WORKERS")
            .ok()
            .map(|value| value.parse::<usize>().expect("scan worker count"))
            .unwrap_or(8);
        assert!((1..=8).contains(&workers));
        let readers = (0..workers)
            .map(|_| {
                Ntfs::scan_fixture(RepeatedVolume {
                    data: Arc::clone(&data),
                    at: 0,
                    length,
                    last,
                })
            })
            .collect();
        let extents = [Run {
            vcn: 0,
            lcn: Some(0),
            count: length / 512,
        }];
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let mut progress = None;
        let found = scan_pagefile(readers, length, &extents, &packets, &stop, started, |p| {
            progress = Some(p)
        })
        .unwrap();
        assert_eq!(*found, key);
        let progress = progress.unwrap();
        assert_eq!(progress.scanned_bytes, length);
        println!("Synthetic volume: {} MiB, {} workers, {:.3}s including reads/copies and cleanup, {} candidate authentications; excludes physical I/O and paging",
            length / MIB, progress.workers, started.elapsed().as_secs_f64(), progress.candidates_tested);
    }

    #[test]
    fn parallel_scan_finds_keys_crossing_worker_and_fragment_boundaries() {
        let key = std::array::from_fn(|i| i as u8 + 1);
        let packets = vec![
            super::super::crypto::seal(&key, 1, b"first payload"),
            super::super::crypto::seal(&key, 2, b"second payload"),
        ];
        let length = 2 * SCAN_CHUNK + 512;
        let mut disk = vec![0; length as usize + 4096];
        let mut object = 32u64.to_le_bytes().to_vec();
        object.extend(key);
        let split = SCAN_CHUNK as usize;
        disk[split - 20..split].copy_from_slice(&object[..20]);
        disk[split + 4096..split + 4096 + 20].copy_from_slice(&object[20..]);
        let disk: Arc<[u8]> = disk.into();
        let readers = || {
            (0..4)
                .map(|_| Ntfs::scan_fixture(Cursor::new(Arc::clone(&disk))))
                .collect()
        };
        let extents = [
            Run {
                vcn: 0,
                lcn: Some(0),
                count: SCAN_CHUNK / 512,
            },
            Run {
                vcn: SCAN_CHUNK / 512,
                lcn: Some(SCAN_CHUNK / 512 + 8),
                count: SCAN_CHUNK / 512 + 1,
            },
        ];
        let stop = AtomicBool::new(false);
        let mut last = None;
        let found = scan_pagefile(
            readers(),
            length,
            &extents,
            &packets,
            &stop,
            Instant::now() - Duration::from_millis(750),
            |p| last = Some(p),
        )
        .unwrap();
        assert_eq!(*found, key);
        let last = last.unwrap();
        assert_eq!(last.stage, "verified");
        assert_eq!(last.workers, 4);
        assert!(last.elapsed_ms >= 750 && last.scanned_bytes <= length);
        let mut corrupt = packets;
        corrupt[1][5] ^= 1;
        let error = scan_pagefile(
            readers(),
            length,
            &extents,
            &corrupt,
            &stop,
            Instant::now(),
            |p| assert!(p.scanned_bytes <= length),
        )
        .err()
        .unwrap();
        assert!(error.contains("No key authenticated"));
        stop.store(true, Ordering::Relaxed);
        assert_eq!(
            scan_pagefile(
                readers(),
                length,
                &extents,
                &corrupt,
                &stop,
                Instant::now(),
                |_| {}
            )
            .err()
            .unwrap(),
            "Player audio stopped"
        );
    }

    #[test]
    fn only_returns_keys_matching_multiple_current_packets_and_honors_cancel() {
        let key = std::array::from_fn(|i| i as u8 + 1);
        let packets = vec![
            super::super::crypto::seal(&key, 1, b"first payload"),
            super::super::crypto::seal(&key, 2, b"second payload"),
        ];
        let mut bytes = vec![99; 101];
        bytes.extend([32, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(key);
        let stop = AtomicBool::new(false);
        assert_eq!(scan_bytes(&bytes, &packets, &stop).as_deref(), Some(&key));
        assert!(scan_bytes(&bytes[..bytes.len() - 1], &packets, &stop).is_none());
        stop.store(true, Ordering::Relaxed);
        assert!(scan_bytes(&bytes, &packets, &stop).is_none());
    }
}
