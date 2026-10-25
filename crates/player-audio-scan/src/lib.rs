//! Hot pagefile search path, optimized independently of the desktop debug build.
//! Only a key authenticating every supplied session packet can leave the scanner.
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use zeroize::{Zeroize, Zeroizing};

/// Keep allocation and secure cleanup optimized even when the caller is a debug
/// build. The initialized length never shrinks, so cleanup includes unused tails.
#[derive(Default)]
pub struct ScanBuffer(Vec<u8>);

impl ScanBuffer {
    pub fn bytes(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

impl Drop for ScanBuffer {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ScanAbort {
    Cancelled,
    Deadline,
}

pub struct ScanControl<'a> {
    pub stop: &'a AtomicBool,
    pub deadline: Instant,
    pub candidates: u64,
}

impl ScanControl<'_> {
    pub fn check(&self) -> Result<(), ScanAbort> {
        if self.stop.load(Ordering::Relaxed) {
            Err(ScanAbort::Cancelled)
        } else if Instant::now() >= self.deadline {
            Err(ScanAbort::Deadline)
        } else {
            Ok(())
        }
    }
}

// Filter candidates for a randomly generated AES-256 session key. Most
// length-32 byte arrays are strings, padding, or other low-diversity structures.
// This heuristic intentionally does not support arbitrary low-diversity keys.
fn plausible_key(bytes: &[u8]) -> bool {
    let mut seen = [0u64; 4];
    let mut distinct = 0;
    for &byte in bytes {
        let word = &mut seen[usize::from(byte >> 6)];
        let bit = 1u64 << (byte & 63);
        if *word & bit == 0 {
            *word |= bit;
            distinct += 1;
            if distinct >= 20 {
                return true;
            }
        }
    }
    false
}

pub fn scan_candidates(
    data: &[u8],
    packets: &[Vec<u8>],
    control: &mut ScanControl<'_>,
) -> Result<Option<Zeroizing<[u8; 32]>>, ScanAbort> {
    control.check()?;
    if packets.len() < 2
        || packets
            .iter()
            .any(|p| !(40..=65_507).contains(&p.len()) || p[2] != 1)
    {
        return Ok(None);
    }
    let mut scratch = Zeroizing::new(Vec::with_capacity(
        packets.iter().map(Vec::len).max().unwrap(),
    ));
    for (index, offset) in memchr::memmem::find_iter(data, &[32, 0, 0, 0, 0, 0, 0, 0]).enumerate() {
        // Avoid a clock query on every rejected object, while bounding CPU work
        // between cancellation/deadline checks even on a dense synthetic page.
        if index & 1023 == 0 {
            control.check()?;
        }
        let Some(bytes) = data.get(offset + 8..offset + 40) else {
            continue;
        };
        if !plausible_key(bytes) {
            continue;
        }
        control.candidates += 1;
        let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, bytes).expect("AES key length"));
        let authenticated = packets.iter().all(|packet| {
            let length = packet.len() - 19;
            if scratch.len() < length {
                scratch.resize(length, 0);
            }
            scratch[..length].copy_from_slice(&packet[19..]);
            key.open_in_place(
                Nonce::try_assume_unique_for_key(&packet[7..19]).expect("nonce length"),
                Aad::from(&packet[..19]),
                &mut scratch[..length],
            )
            .is_ok()
        });
        if authenticated {
            return Ok(Some(Zeroizing::new(bytes.try_into().expect("key length"))));
        }
    }
    control.check()?;
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn seal(key: &[u8; 32], counter: u8) -> Vec<u8> {
        let key = LessSafeKey::new(UnboundKey::new(&AES_256_GCM, key).unwrap());
        let mut header = vec![0, 1, 1, 4, 3, 2, 1];
        let nonce = [counter; 12];
        header.extend(nonce);
        let mut data = vec![99; 512];
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(&header),
            &mut data,
        )
        .unwrap();
        header.extend(data);
        header
    }

    #[test]
    fn filters_non_keys_and_authenticates_every_packet() {
        let secret = std::array::from_fn(|i| i as u8 + 32);
        let mut data = vec![0; 128];
        data[..8].copy_from_slice(&32u64.to_le_bytes());
        data[64..72].copy_from_slice(&32u64.to_le_bytes());
        data[72..104].copy_from_slice(&secret);
        let packets = [seal(&secret, 1), seal(&secret, 2)];
        let stop = AtomicBool::new(false);
        let mut control = ScanControl {
            stop: &stop,
            deadline: Instant::now() + Duration::from_secs(5),
            candidates: 0,
        };
        let found = scan_candidates(&data, &packets, &mut control)
            .unwrap()
            .unwrap();
        assert_eq!(*found, secret);
        assert_eq!(control.candidates, 1);
        let mut corrupt = packets;
        corrupt[1][5] ^= 1;
        assert!(scan_candidates(&data, &corrupt, &mut control)
            .unwrap()
            .is_none());
        stop.store(true, Ordering::Relaxed);
        assert_eq!(
            scan_candidates(&data, &corrupt, &mut control).err(),
            Some(ScanAbort::Cancelled)
        );
        stop.store(false, Ordering::Relaxed);
        control.deadline = Instant::now();
        assert_eq!(
            scan_candidates(&data, &corrupt, &mut control).err(),
            Some(ScanAbort::Deadline)
        );
    }

    #[test]
    #[ignore = "Explicit 8.3 GiB CPU-only scan benchmark; no real pagefile or secrets"]
    fn benchmark_scan_8455_mib() {
        let mut data = vec![0; 64 * 1024 * 1024];
        for (index, object) in data.as_chunks_mut::<512>().0.iter_mut().enumerate() {
            object[..8].copy_from_slice(&32u64.to_le_bytes());
            // Mostly low-diversity objects, with one random-looking false key
            // every 64 KiB. Neither fixture nor packet uses real session data.
            if index % 128 == 0 {
                for (i, byte) in object[8..40].iter_mut().enumerate() {
                    *byte = (i as u8).wrapping_add(index as u8);
                }
            } else {
                object[8..40].fill(9);
            }
        }
        let secret = std::array::from_fn(|i| (i as u8).wrapping_mul(7));
        let packets = [seal(&secret, 1), seal(&secret, 2)];
        let stop = AtomicBool::new(false);
        let started = Instant::now();
        let mut control = ScanControl {
            stop: &stop,
            deadline: started + Duration::from_secs(60),
            candidates: 0,
        };
        let mut scanned = 0usize;
        let total = 8455usize * 1024 * 1024;
        while scanned < total {
            let count = data.len().min(total - scanned);
            assert!(
                scan_candidates(std::hint::black_box(&data[..count]), &packets, &mut control)
                    .unwrap()
                    .is_none()
            );
            scanned += count;
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "CPU scan: {scanned} bytes in {elapsed:.3}s, {:.1} MiB/s, {} candidate authentications",
            scanned as f64 / 1048576.0 / elapsed,
            control.candidates
        );
    }
}
