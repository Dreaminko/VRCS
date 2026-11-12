use std::fs::{self, File};
use std::io;
use std::os::unix::fs::FileExt;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};
use zeroize::{Zeroize, Zeroizing};

use player_audio_helper::procfs;

const CHUNK: usize = 4 * 1024 * 1024;
const OVERLAP: usize = 32 + 8;

struct Region {
    start: usize,
    end: usize,
}

pub struct ScanOutcome {
    pub key: Option<Zeroizing<[u8; 32]>>,
    pub scanned: u64,
    pub candidates: u64,
    pub elapsed_ms: u64,
}

pub fn scan(
    pid: u32,
    starttime: u64,
    packets: &[Vec<u8>],
    stop: &AtomicBool,
    deadline: Instant,
    mut progress: impl FnMut(u64, u64, u64),
) -> io::Result<ScanOutcome> {
    let mut control = player_audio_scan::ScanControl {
        stop,
        deadline,
        candidates: 0,
    };
    control.check().map_err(scan_abort)?;
    if packets.len() < 2
        || packets
            .iter()
            .any(|packet| !(40..=65_507).contains(&packet.len()) || packet[2] != 1)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid encrypted packet samples",
        ));
    }
    procfs::check_identity(pid, starttime)?;
    let regions = regions(pid)?;
    // An open mem fd remains tied to this mm even if the numeric PID is reused.
    let memory = File::open(format!("/proc/{pid}/mem"))?;
    procfs::check_identity(pid, starttime)?;
    let total: u64 = regions.iter().map(|r| (r.end - r.start) as u64).sum();
    let started = Instant::now();
    let mut scanned = 0u64;
    let mut buffer = player_audio_scan::ScanBuffer::default();
    buffer.bytes().resize(CHUNK + OVERLAP, 0);
    let buffer = buffer.bytes();
    let page = page_size();
    let mut last_report = started;
    for region in regions {
        let mut address = region.start;
        let mut carry = 0;
        while address < region.end {
            control.check().map_err(scan_abort)?;
            procfs::check_identity(pid, starttime)?;
            let length = (region.end - address).min(CHUNK);
            let result = read_memory(&memory, address, &mut buffer[carry..carry + length]);
            // Check after failures and zero reads too, before inspecting any bytes.
            procfs::check_identity(pid, starttime)?;
            control.check().map_err(scan_abort)?;
            let read = result?.unwrap_or(0);
            if read == 0 {
                // Never join candidates across an unreadable gap.
                buffer[..carry].zeroize();
                carry = 0;
                address = address
                    .saturating_add(page - address % page)
                    .min(region.end);
                continue;
            }
            let valid = carry + read;
            buffer[valid..].zeroize();
            let key = player_audio_scan::scan_candidates(&buffer[..valid], packets, &mut control)
                .map_err(scan_abort)?;
            scanned += read as u64;
            if key.is_some() {
                progress(scanned, total, control.candidates);
                return Ok(ScanOutcome {
                    key,
                    scanned,
                    candidates: control.candidates,
                    elapsed_ms: started.elapsed().as_millis() as u64,
                });
            }
            if last_report.elapsed() >= Duration::from_millis(250) {
                progress(scanned, total, control.candidates);
                last_report = Instant::now();
            }
            // Both full and short contiguous reads retain the same candidate tail.
            carry = valid.min(OVERLAP);
            buffer.copy_within(valid - carry..valid, 0);
            address += read;
        }
        buffer.as_mut_slice().zeroize();
    }
    progress(scanned, total, control.candidates);
    Ok(ScanOutcome {
        key: None,
        scanned,
        candidates: control.candidates,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
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
        let _inode = fields.next();
        let path = fields.next().unwrap_or("");
        if !perms.starts_with("rw")
            || path.starts_with("/dev")
            || path.starts_with("/sys")
            || path.starts_with("/proc")
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
            regions.push(Region { start, end });
        }
    }
    regions.sort_by_key(|region| std::cmp::Reverse(region.end - region.start));
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
            &[],
            &stop,
            Instant::now() + Duration::from_secs(1),
            |_, _, _| {},
        );
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::Interrupted);
        let stop = AtomicBool::new(false);
        let result = scan(0, 0, &[], &stop, Instant::now(), |_, _, _| {});
        assert_eq!(result.err().unwrap().kind(), io::ErrorKind::TimedOut);
    }
}
