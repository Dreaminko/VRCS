use std::fs::{self, File};
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

use super::procfs;

const CHUNK: usize = 4 * 1024 * 1024;
const OVERLAP: usize = 32 + 8;
const SMALL_ANON_LIMIT: usize = 64 * 1024 * 1024;
const MAX_WORKERS: usize = 4;
const PAGEMAP_BATCH: usize = 512;
const PAGEMAP_PRESENT_OR_SWAPPED: u64 = (1 << 63) | (1 << 62);

struct Region {
    start: usize,
    end: usize,
    anonymous: bool,
}

pub(super) struct ScanOutcome {
    pub key: Option<Zeroizing<[u8; 32]>>,
    pub scanned: u64,
    pub candidates: u64,
    pub elapsed_ms: u64,
}

pub(super) fn scan(
    pid: u32,
    starttime: u64,
    expected_uid: u32,
    packets: &[Vec<u8>],
    stop: &AtomicBool,
    deadline: Instant,
    mut progress: impl FnMut(u64, u64, u64) + Send,
) -> io::Result<ScanOutcome> {
    let control = player_audio_scan::ScanControl {
        stop,
        deadline,
        candidates: 0,
    };
    control.check().map_err(scan_abort)?;
    if packets.len() < 2
        || packets
            .iter()
            .any(|packet| !(40..=65_507).contains(&packet.len()) || packet[2] != 1)
        || packets
            .iter()
            .map(|packet| &packet[7..19])
            .collect::<std::collections::HashSet<_>>()
            .len()
            < 2
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid encrypted packet samples",
        ));
    }
    if fs::read_to_string("/proc/sys/kernel/yama/ptrace_scope")
        .ok()
        .is_some_and(|scope| scope.trim() == "3")
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Linux key scan is blocked because kernel.yama.ptrace_scope = 3",
        ));
    }
    procfs::check_identity(pid, starttime)?;
    let regions = regions(pid)?;
    // An open mem fd remains tied to this mm even if the numeric PID is reused.
    let memory = File::open(format!("/proc/{pid}/mem"))?;
    let pagemap = File::open(format!("/proc/{pid}/pagemap")).ok();
    procfs::validate_target(pid, starttime, expected_uid)?;
    let total: u64 = regions.iter().map(|r| (r.end - r.start) as u64).sum();
    let started = Instant::now();
    if regions.is_empty() {
        progress(0, 0, 0);
        return Ok(ScanOutcome {
            key: None,
            scanned: 0,
            candidates: 0,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }

    let page = page_size();
    let next_region = AtomicUsize::new(0);
    let found = AtomicBool::new(false);
    let scanned_total = AtomicU64::new(0);
    let candidates_total = AtomicU64::new(0);
    let key_slot: Mutex<Option<Zeroizing<[u8; 32]>>> = Mutex::new(None);
    let error_slot: Mutex<Option<io::Error>> = Mutex::new(None);
    let progress_state = Mutex::new((&mut progress, started));
    let worker_count = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .clamp(1, MAX_WORKERS)
        .min(regions.len());

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                let mut buffer = player_audio_scan::ScanBuffer::default();
                buffer.bytes().resize(CHUNK + OVERLAP, 0);
                let buf = buffer.bytes();
                let mut pagemap_entries = vec![0u64; PAGEMAP_BATCH];
                let mut worker_control = player_audio_scan::ScanControl {
                    stop,
                    deadline,
                    candidates: 0,
                };
                let mut last_identity = Instant::now();
                let mut reported_candidates = 0u64;

                while let Some(region) = if found.load(Ordering::Acquire) {
                    None
                } else {
                    regions.get(next_region.fetch_add(1, Ordering::Relaxed))
                } {
                    let mut address = region.start;
                    let mut prev_end = address;
                    let mut carry = 0;
                    let mut high_water = 0;
                    while address < region.end {
                        if found.load(Ordering::Acquire) {
                            break;
                        }
                        if let Err(abort) = worker_control.check() {
                            record_error(&error_slot, scan_abort(abort));
                            break;
                        }
                        if last_identity.elapsed() >= Duration::from_millis(64) {
                            if let Err(err) = procfs::check_identity(pid, starttime) {
                                record_error(&error_slot, err);
                                break;
                            }
                            last_identity = Instant::now();
                        }

                        let (run_start, run_end) = next_resident_run(
                            pagemap.as_ref(),
                            address,
                            region.end,
                            page,
                            &mut pagemap_entries,
                        );
                        if run_start >= region.end {
                            break;
                        }
                        if run_start != prev_end {
                            if high_water > 0 {
                                buf[..high_water].zeroize();
                                high_water = 0;
                            }
                            carry = 0;
                        }
                        let length = (run_end - run_start).min(CHUNK);
                        let result =
                            read_memory(&memory, run_start, &mut buf[carry..carry + length]);
                        if let Err(abort) = worker_control.check() {
                            record_error(&error_slot, scan_abort(abort));
                            break;
                        }
                        let read = match result {
                            Ok(Some(read)) => read,
                            Ok(None) => {
                                if high_water > 0 {
                                    buf[..high_water].zeroize();
                                    high_water = 0;
                                }
                                carry = 0;
                                address = run_start
                                    .saturating_add(page - run_start % page)
                                    .min(region.end);
                                prev_end = address;
                                continue;
                            }
                            Err(err) => {
                                record_error(&error_slot, err);
                                break;
                            }
                        };
                        let valid = carry + read;
                        if valid < high_water {
                            buf[valid..high_water].zeroize();
                        }
                        high_water = valid;
                        let key = match player_audio_scan::scan_candidates(
                            &buf[..valid],
                            packets,
                            &mut worker_control,
                        ) {
                            Ok(key) => key,
                            Err(abort) => {
                                record_error(&error_slot, scan_abort(abort));
                                break;
                            }
                        };
                        let delta_candidates = worker_control.candidates - reported_candidates;
                        reported_candidates = worker_control.candidates;
                        if delta_candidates > 0 {
                            candidates_total.fetch_add(delta_candidates, Ordering::Relaxed);
                        }
                        let total_scanned =
                            scanned_total.fetch_add(read as u64, Ordering::Relaxed) + read as u64;

                        if key.is_some() {
                            if !found.swap(true, Ordering::AcqRel) {
                                *key_slot.lock().unwrap() = key;
                            }
                            break;
                        }
                        if let Ok(mut guard) = progress_state.try_lock() {
                            if guard.1.elapsed() >= Duration::from_millis(250) {
                                guard.1 = Instant::now();
                                let cands = candidates_total.load(Ordering::Relaxed);
                                (guard.0)(total_scanned, total, cands);
                            }
                        }
                        carry = valid.min(OVERLAP);
                        buf.copy_within(valid - carry..valid, 0);
                        address = run_start + read;
                        prev_end = address;
                    }
                    if high_water > 0 {
                        buf[..high_water].zeroize();
                    }
                    if found.load(Ordering::Acquire) || error_slot.lock().unwrap().is_some() {
                        break;
                    }
                }
            });
        }
    });

    let scanned = scanned_total.load(Ordering::Relaxed);
    let candidates = candidates_total.load(Ordering::Relaxed);
    if let Some(key) = key_slot.into_inner().unwrap() {
        procfs::check_identity(pid, starttime)?;
        progress(scanned, total, candidates);
        return Ok(ScanOutcome {
            key: Some(key),
            scanned,
            candidates,
            elapsed_ms: started.elapsed().as_millis() as u64,
        });
    }
    if let Some(err) = error_slot.into_inner().unwrap() {
        return Err(err);
    }
    procfs::check_identity(pid, starttime)?;
    progress(scanned, total, candidates);
    Ok(ScanOutcome {
        key: None,
        scanned,
        candidates,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

fn record_error(slot: &Mutex<Option<io::Error>>, error: io::Error) {
    let mut guard = slot.lock().unwrap();
    if guard.is_none() {
        *guard = Some(error);
    }
}

fn next_resident_run(
    pagemap: Option<&File>,
    mut address: usize,
    end: usize,
    page: usize,
    entries: &mut [u64],
) -> (usize, usize) {
    let Some(pagemap) = pagemap else {
        return (address, (address + CHUNK).min(end));
    };
    while address < end {
        let aligned = address - (address % page);
        let max_pages = ((end - aligned).div_ceil(page)).min(entries.len());
        let byte_offset = (aligned / page) as u64 * 8;
        let byte_slice = unsafe {
            std::slice::from_raw_parts_mut(entries.as_mut_ptr().cast::<u8>(), max_pages * 8)
        };
        let Ok(read_bytes) = pagemap.read_at(byte_slice, byte_offset) else {
            return (address, (address + CHUNK).min(end));
        };
        let count = read_bytes / 8;
        if count == 0 {
            return (address, (address + CHUNK).min(end));
        }
        let mut first_resident = None;
        for (idx, &entry) in entries[..count].iter().enumerate() {
            if (entry & PAGEMAP_PRESENT_OR_SWAPPED) != 0 {
                first_resident = Some(idx);
                break;
            }
        }
        let Some(start_idx) = first_resident else {
            address = (aligned + count * page).min(end);
            continue;
        };
        let mut end_idx = start_idx + 1;
        while end_idx < count
            && (entries[end_idx] & PAGEMAP_PRESENT_OR_SWAPPED) != 0
            && (end_idx - start_idx) * page < CHUNK
        {
            end_idx += 1;
        }
        let run_start = if start_idx == 0 {
            address
        } else {
            aligned + start_idx * page
        };
        let run_end = (aligned + end_idx * page).min(end).min(run_start + CHUNK);
        return (run_start, run_end);
    }
    (end, end)
}

fn scan_abort(abort: player_audio_scan::ScanAbort) -> io::Error {
    match abort {
        player_audio_scan::ScanAbort::Cancelled => {
            io::Error::new(io::ErrorKind::Interrupted, "Player audio stopped")
        }
        player_audio_scan::ScanAbort::Deadline => io::Error::new(
            io::ErrorKind::TimedOut,
            "Linux key scan timed out after 40 s",
        ),
    }
}

fn regions(pid: u32) -> io::Result<Vec<Region>> {
    let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
    let mut regions = Vec::new();
    for line in maps.lines() {
        let mut fields = line.split_whitespace();
        let Some(range) = fields.next() else { continue };
        let Some(perms) = fields.next() else { continue };
        let _offset = fields.next();
        let _device = fields.next();
        let inode = fields
            .next()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(1);
        let path = fields.next().unwrap_or("");
        if !matches!(perms, "rw-p" | "rwxp")
            || path.starts_with("/dev")
            || path.starts_with("/sys")
            || path.starts_with("/proc")
            || path.starts_with("anon_inode:")
            || path.starts_with("/memfd:")
            || matches!(path, "[vvar]" | "[vdso]" | "[vsyscall]")
        {
            continue;
        }
        let Some((start, end)) = range.split_once('-') else {
            continue;
        };
        let (Ok(start), Ok(end)) = (
            usize::from_str_radix(start, 16),
            usize::from_str_radix(end, 16),
        ) else {
            continue;
        };
        if end > start && end - start >= 64 * 1024 {
            regions.push(Region {
                start,
                end,
                anonymous: inode == 0 && path.is_empty(),
            });
        }
    }
    regions.sort_by_key(|region| {
        let size = region.end - region.start;
        let tier = if region.anonymous {
            if size <= SMALL_ANON_LIMIT { 0u8 } else { 1 }
        } else {
            2
        };
        (tier, size)
    });
    Ok(regions)
}

fn read_memory(memory: &File, address: usize, output: &mut [u8]) -> io::Result<Option<usize>> {
    match memory.read_at(output, address as u64) {
        Ok(read) => Ok(Some(read)),
        Err(error) => match error.raw_os_error() {
            Some(libc::EFAULT) | Some(libc::EIO) => Ok(None),
            Some(libc::ESRCH) => Err(io::Error::new(
                io::ErrorKind::NotFound,
                "VRChat exited during key acquisition",
            )),
            _ => Err(error),
        },
    }
}

fn page_size() -> usize {
    let value = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    usize::try_from(value).unwrap_or(4096).max(4096)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelled_or_expired_scan_does_not_open_target_memory() {
        let stop = AtomicBool::new(true);
        let result = scan(
            0,
            0,
            0,
            &[],
            &stop,
            Instant::now() + Duration::from_secs(1),
            |_, _, _| {},
        );
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
        let stop = AtomicBool::new(false);
        let result = scan(0, 0, 0, &[], &stop, Instant::now(), |_, _, _| {});
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn regions_include_rwxp_and_deprioritize_huge_reservations() {
        let mut list = [
            Region {
                start: 0,
                end: 512 * 1024 * 1024,
                anonymous: true,
            },
            Region {
                start: 0,
                end: 1024 * 1024,
                anonymous: false,
            },
            Region {
                start: 0,
                end: 2 * 1024 * 1024,
                anonymous: true,
            },
        ];
        list.sort_by_key(|r| {
            let size = r.end - r.start;
            let tier = if r.anonymous {
                if size <= SMALL_ANON_LIMIT { 0u8 } else { 1 }
            } else {
                2
            };
            (tier, size)
        });
        assert_eq!(list[0].end, 2 * 1024 * 1024);
        assert_eq!(list[1].end, 512 * 1024 * 1024);
        assert_eq!(list[2].end, 1024 * 1024);
    }
}
