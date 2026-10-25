//! Read-only, bounded NTFS extent discovery. No volume writes or file exports.
use std::io::{Read, Seek, SeekFrom};
use zeroize::Zeroizing;

const LIMIT: usize = 64 * 1024 * 1024;
const BAD: &str = "Unsupported or inconsistent NTFS metadata";

fn word(bytes: &[u8], at: usize) -> Result<u16, String> {
    Ok(u16::from_le_bytes(
        bytes.get(at..at + 2).ok_or(BAD)?.try_into().unwrap(),
    ))
}
fn dword(bytes: &[u8], at: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes.get(at..at + 4).ok_or(BAD)?.try_into().unwrap(),
    ))
}
fn qword(bytes: &[u8], at: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(
        bytes.get(at..at + 8).ok_or(BAD)?.try_into().unwrap(),
    ))
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Run {
    pub vcn: u64,
    pub lcn: Option<u64>,
    pub count: u64,
}

fn runs(bytes: &[u8], mut vcn: u64) -> Result<Vec<Run>, String> {
    let mut cursor = 0;
    let mut lcn = 0i64;
    let mut result = Vec::new();
    while let Some(&header) = bytes.get(cursor) {
        cursor += 1;
        if header == 0 {
            return Ok(result);
        }
        let (length, offset) = ((header & 15) as usize, (header >> 4) as usize);
        if !(1..=8).contains(&length) || offset > 8 || result.len() >= 4096 {
            return Err(BAD.into());
        }
        let mut value = [0; 8];
        value[..length].copy_from_slice(bytes.get(cursor..cursor + length).ok_or(BAD)?);
        cursor += length;
        let count = u64::from_le_bytes(value);
        if count == 0 {
            return Err(BAD.into());
        }
        let physical = if offset == 0 {
            None
        } else {
            let data = bytes.get(cursor..cursor + offset).ok_or(BAD)?;
            let mut signed = if data[offset - 1] & 128 != 0 {
                [255; 8]
            } else {
                [0; 8]
            };
            signed[..offset].copy_from_slice(data);
            lcn = lcn.checked_add(i64::from_le_bytes(signed)).ok_or(BAD)?;
            Some(u64::try_from(lcn).map_err(|_| BAD)?)
        };
        cursor += offset;
        result.push(Run {
            vcn,
            lcn: physical,
            count,
        });
        vcn = vcn.checked_add(count).ok_or(BAD)?;
    }
    Err(BAD.into())
}

fn fixup(bytes: &mut [u8], sector: usize) -> Result<(), String> {
    let at = word(bytes, 4)? as usize;
    let count = word(bytes, 6)? as usize;
    if count != bytes.len() / sector + 1 {
        return Err(BAD.into());
    }
    let update = bytes.get(at..at + count * 2).ok_or(BAD)?.to_vec();
    for i in 1..count {
        let tail = i * sector - 2;
        if bytes.get(tail..tail + 2) != Some(&update[..2]) {
            return Err(BAD.into());
        }
        bytes[tail..tail + 2].copy_from_slice(&update[i * 2..i * 2 + 2]);
    }
    Ok(())
}

struct Attribute<'a> {
    kind: u32,
    bytes: &'a [u8],
    unnamed: bool,
}
impl Attribute<'_> {
    fn resident(&self) -> Result<&[u8], String> {
        if self.bytes[8] != 0 {
            return Err(BAD.into());
        }
        let size = dword(self.bytes, 16)? as usize;
        let at = word(self.bytes, 20)? as usize;
        self.bytes
            .get(at..at.checked_add(size).ok_or(BAD)?)
            .ok_or_else(|| BAD.into())
    }
    fn runs(&self) -> Result<Vec<Run>, String> {
        if self.bytes[8] != 1 {
            return Err(BAD.into());
        }
        runs(
            self.bytes
                .get(word(self.bytes, 32)? as usize..)
                .ok_or(BAD)?,
            qword(self.bytes, 16)?,
        )
    }
}
fn attributes(record: &[u8]) -> Result<Vec<Attribute<'_>>, String> {
    let mut at = word(record, 20)? as usize;
    let mut result = Vec::new();
    while at + 8 <= record.len() {
        let kind = dword(record, at)?;
        if kind == u32::MAX {
            return Ok(result);
        }
        let length = dword(record, at + 4)? as usize;
        if length < 24 || result.len() >= 128 {
            return Err(BAD.into());
        }
        let bytes = record
            .get(at..at.checked_add(length).ok_or(BAD)?)
            .ok_or(BAD)?;
        result.push(Attribute {
            kind,
            bytes,
            unnamed: bytes[9] == 0,
        });
        at += length;
    }
    Err(BAD.into())
}

pub(super) struct Ntfs<R> {
    disk: R,
    pub sector: usize,
    pub cluster: u64,
    record_size: usize,
    mft: Vec<Run>,
}

impl<R: Read + Seek> Ntfs<R> {
    #[cfg(test)]
    pub(super) fn scan_fixture(disk: R) -> Self {
        Self {
            disk,
            sector: 512,
            cluster: 512,
            record_size: 1024,
            mft: Vec::new(),
        }
    }

    /// Each parallel scanner needs an independently opened volume handle:
    /// duplicating a File would share its seek position on Windows.
    pub(super) fn with_reader<T: Read + Seek>(&self, disk: T) -> Ntfs<T> {
        Ntfs {
            disk,
            sector: self.sector,
            cluster: self.cluster,
            record_size: self.record_size,
            mft: self.mft.clone(),
        }
    }

    pub(super) fn new(disk: R) -> Result<Self, String> {
        let mut fs = Self {
            disk,
            sector: 4096,
            cluster: 0,
            record_size: 0,
            mft: Vec::new(),
        };
        let boot = fs.read_at(0, 4096)?;
        if boot.get(3..11) != Some(b"NTFS    ") {
            return Err("Pagefile volume must use NTFS".into());
        }
        fs.sector = word(&boot, 11)? as usize;
        if ![512, 1024, 2048, 4096].contains(&fs.sector) {
            return Err(BAD.into());
        }
        fs.cluster = (fs.sector as u64)
            .checked_mul(u64::from(boot[13]))
            .ok_or(BAD)?;
        if fs.cluster == 0 || fs.cluster > 2 * 1024 * 1024 {
            return Err(BAD.into());
        }
        let code = boot[64] as i8;
        fs.record_size = if code > 0 {
            (u64::from(code as u8) * fs.cluster) as usize
        } else {
            1usize
                .checked_shl(u32::from(code.unsigned_abs()))
                .ok_or(BAD)?
        };
        if !(512..=65536).contains(&fs.record_size) {
            return Err(BAD.into());
        }
        let mut record = fs.read_at(
            qword(&boot, 48)?.checked_mul(fs.cluster).ok_or(BAD)?,
            fs.record_size,
        )?;
        fixup(&mut record, fs.sector)?;
        for attr in attributes(&record)? {
            if attr.kind == 0x80 && attr.unnamed {
                fs.mft.extend(attr.runs()?);
            }
        }
        if fs.mft.is_empty() {
            return Err("NTFS MFT extents are unavailable".into());
        }
        Ok(fs)
    }

    pub(super) fn read_at(
        &mut self,
        offset: u64,
        length: usize,
    ) -> Result<Zeroizing<Vec<u8>>, String> {
        let mut data = Zeroizing::new(Vec::new());
        let skip = (offset % self.sector as u64) as usize;
        self.read_at_into(offset, length, &mut data)?;
        data.copy_within(skip..skip + length, 0);
        data.truncate(length);
        Ok(data)
    }

    /// Reuse the pagefile scan buffer; allocating and wiping it per chunk is
    /// particularly expensive in debug builds. Keep its full initialized length
    /// so the caller's Zeroizing owner also wipes any unused tail on drop.
    pub(super) fn read_at_into<'a>(
        &mut self,
        offset: u64,
        length: usize,
        data: &'a mut Vec<u8>,
    ) -> Result<&'a [u8], String> {
        if length > LIMIT {
            return Err(BAD.into());
        }
        let start = offset / self.sector as u64 * self.sector as u64;
        let skip = (offset - start) as usize;
        let aligned = (length + skip).div_ceil(self.sector) * self.sector;
        if data.len() < aligned {
            data.resize(aligned, 0);
        }
        self.disk
            .seek(SeekFrom::Start(start))
            .map_err(|e| format!("Cannot seek pagefile volume: {e}"))?;
        self.disk
            .read_exact(&mut data[..aligned])
            .map_err(|e| format!("Cannot read pagefile volume: {e}"))?;
        Ok(&data[skip..skip + length])
    }

    fn read_runs(
        &mut self,
        list: &[Run],
        offset: u64,
        length: usize,
    ) -> Result<Zeroizing<Vec<u8>>, String> {
        if length > LIMIT {
            return Err(BAD.into());
        }
        let mut result = Zeroizing::new(Vec::with_capacity(length));
        let mut position = offset;
        while result.len() < length {
            let vcn = position / self.cluster;
            let run = list
                .iter()
                .find(|r| vcn >= r.vcn && vcn - r.vcn < r.count)
                .ok_or(BAD)?;
            let within = position % self.cluster;
            let available = (run.count - (vcn - run.vcn))
                .checked_mul(self.cluster)
                .ok_or(BAD)?
                - within;
            let take =
                (length - result.len()).min(usize::try_from(available).unwrap_or(usize::MAX));
            if let Some(lcn) = run.lcn {
                let physical = lcn
                    .checked_add(vcn - run.vcn)
                    .and_then(|v| v.checked_mul(self.cluster))
                    .and_then(|v| v.checked_add(within))
                    .ok_or(BAD)?;
                result.extend_from_slice(&self.read_at(physical, take)?);
            } else {
                let size = result.len() + take;
                result.resize(size, 0);
            }
            position += take as u64;
        }
        Ok(result)
    }

    /// Read a logical range across fragmented/sparse extents without per-chunk
    /// allocations. Callers retain the full initialized buffers for zeroization.
    pub(super) fn read_runs_into<'a>(
        &mut self,
        list: &[Run],
        offset: u64,
        length: usize,
        output: &'a mut Vec<u8>,
        scratch: &'a mut Vec<u8>,
    ) -> Result<&'a [u8], String> {
        if length > LIMIT {
            return Err(BAD.into());
        }
        // Scan a contiguous physical range directly from its read buffer.
        // Only fragmented/sparse ranges need the assembly buffer below.
        let vcn = offset / self.cluster;
        if let Some(run) = list.iter().find(|r| vcn >= r.vcn && vcn - r.vcn < r.count) {
            let within = offset % self.cluster;
            let available = (run.count - (vcn - run.vcn))
                .checked_mul(self.cluster)
                .ok_or(BAD)?
                - within;
            if let Some(lcn) = run.lcn.filter(|_| length as u64 <= available) {
                let physical = lcn
                    .checked_add(vcn - run.vcn)
                    .and_then(|v| v.checked_mul(self.cluster))
                    .and_then(|v| v.checked_add(within))
                    .ok_or(BAD)?;
                return self.read_at_into(physical, length, scratch);
            }
        }
        if output.len() < length {
            output.resize(length, 0);
        }
        let mut copied = 0;
        while copied < length {
            let position = offset.checked_add(copied as u64).ok_or(BAD)?;
            let vcn = position / self.cluster;
            let run = list
                .iter()
                .find(|r| vcn >= r.vcn && vcn - r.vcn < r.count)
                .ok_or(BAD)?;
            let within = position % self.cluster;
            let available = (run.count - (vcn - run.vcn))
                .checked_mul(self.cluster)
                .ok_or(BAD)?
                - within;
            let take = (length - copied).min(usize::try_from(available).unwrap_or(usize::MAX));
            if let Some(lcn) = run.lcn {
                let physical = lcn
                    .checked_add(vcn - run.vcn)
                    .and_then(|v| v.checked_mul(self.cluster))
                    .and_then(|v| v.checked_add(within))
                    .ok_or(BAD)?;
                output[copied..copied + take]
                    .copy_from_slice(self.read_at_into(physical, take, scratch)?);
            } else {
                output[copied..copied + take].fill(0);
            }
            copied += take;
        }
        Ok(&output[..length])
    }

    fn record(&mut self, number: u64) -> Result<Zeroizing<Vec<u8>>, String> {
        let mut bytes = self.read_runs(
            &self.mft.clone(),
            number.checked_mul(self.record_size as u64).ok_or(BAD)?,
            self.record_size,
        )?;
        if bytes.get(..4) != Some(b"FILE") {
            return Err(BAD.into());
        }
        fixup(&mut bytes, self.sector)?;
        Ok(bytes)
    }

    pub(super) fn pagefile(&mut self) -> Result<(u64, Vec<Run>), String> {
        let root = self.record(5)?;
        let mut index_size = 4096;
        let mut allocation = Vec::new();
        let mut allocation_size = 0;
        let mut number = None;
        for attr in attributes(&root)? {
            if attr.kind == 0x90 {
                let value = attr.resident()?;
                index_size = dword(value, 8)? as usize;
                number = lookup_index(value, 16)?;
            } else if attr.kind == 0xa0 {
                allocation = attr.runs()?;
                allocation_size = qword(attr.bytes, 48)?;
            }
        }
        if number.is_none() {
            if !(512..=65536).contains(&index_size) || allocation_size > LIMIT as u64 {
                return Err(BAD.into());
            }
            let mut offset = 0;
            while offset + index_size as u64 <= allocation_size {
                let mut index = self.read_runs(&allocation, offset, index_size)?;
                offset += index_size as u64;
                if index.get(..4) != Some(b"INDX") {
                    continue;
                }
                if fixup(&mut index, self.sector).is_err() {
                    continue;
                }
                number = lookup_index(&index, 24)?;
                if number.is_some() {
                    break;
                }
            }
        }
        let number = number.ok_or("pagefile.sys was not found on the selected drive")?;
        let record = self.record(number)?;
        let mut data_runs = Vec::new();
        let mut size = 0;
        let mut extensions = Vec::new();
        for attr in attributes(&record)? {
            if attr.kind == 0x80 && attr.unnamed {
                // File size is valid only in the extent whose LowestVcn is zero.
                if qword(attr.bytes, 16)? == 0 {
                    size = qword(attr.bytes, 48)?;
                }
                data_runs.extend(attr.runs()?);
            } else if attr.kind == 0x20 {
                let list = if attr.bytes[8] == 0 {
                    Zeroizing::new(attr.resident()?.to_vec())
                } else {
                    self.read_runs(
                        &attr.runs()?,
                        0,
                        usize::try_from(qword(attr.bytes, 48)?).map_err(|_| BAD)?,
                    )?
                };
                let mut at = 0;
                while at + 26 <= list.len() {
                    let length = word(&list, at + 4)? as usize;
                    if length < 26 || at + length > list.len() || extensions.len() > 32 {
                        return Err(BAD.into());
                    }
                    if dword(&list, at)? == 0x80 && list[at + 6] == 0 {
                        let reference = qword(&list, at + 16)? & 0x0000_ffff_ffff_ffff;
                        if reference != number && !extensions.contains(&reference) {
                            extensions.push(reference);
                        }
                    }
                    at += length;
                }
            }
        }
        for number in extensions {
            let extension = self.record(number)?;
            for attr in attributes(&extension)? {
                if attr.kind == 0x80 && attr.unnamed {
                    if qword(attr.bytes, 16)? == 0 {
                        size = qword(attr.bytes, 48)?;
                    }
                    data_runs.extend(attr.runs()?);
                }
            }
        }
        data_runs.sort_by_key(|r| r.vcn);
        if data_runs.is_empty() || size == 0 {
            return Err("Pagefile data extents are unavailable".into());
        }
        Ok((size, data_runs))
    }
}

fn lookup_index(bytes: &[u8], header: usize) -> Result<Option<u64>, String> {
    let mut at = header
        .checked_add(dword(bytes, header)? as usize)
        .ok_or(BAD)?;
    let end = header
        .checked_add(dword(bytes, header + 4)? as usize)
        .ok_or(BAD)?;
    if end > bytes.len() {
        return Err(BAD.into());
    }
    while at + 16 <= end {
        let length = word(bytes, at + 8)? as usize;
        let key_size = word(bytes, at + 10)? as usize;
        let flags = word(bytes, at + 12)?;
        if length < 16 || at + length > end || key_size > length - 16 {
            return Err(BAD.into());
        }
        let key = &bytes[at + 16..at + 16 + key_size];
        if key.len() >= 66 {
            let chars = usize::from(key[64]);
            if let Some(name) = key.get(66..66 + chars * 2) {
                let name = String::from_utf16_lossy(
                    &name
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_le_bytes([b[0], b[1]]))
                        .collect::<Vec<_>>(),
                );
                if name.eq_ignore_ascii_case("pagefile.sys") {
                    return Ok(Some(qword(bytes, at)? & 0x0000_ffff_ffff_ffff));
                }
            }
        }
        if flags & 2 != 0 {
            break;
        }
        at += length;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reused_buffers_preserve_alignment_fragment_boundaries_and_sparse_runs() {
        let mut disk = vec![0; 8192];
        disk[512..1024].fill(7);
        disk[3072..3584].fill(9);
        let mut fs = Ntfs::scan_fixture(std::io::Cursor::new(disk));
        let runs = [
            Run {
                vcn: 0,
                lcn: Some(1),
                count: 1,
            },
            Run {
                vcn: 1,
                lcn: Some(6),
                count: 1,
            },
            Run {
                vcn: 2,
                lcn: None,
                count: 1,
            },
        ];
        let mut output = vec![42; 4096];
        let mut scratch = vec![42; 4096];
        assert_eq!(
            fs.read_runs_into(&runs, 508, 8, &mut output, &mut scratch)
                .unwrap(),
            &[7, 7, 7, 7, 9, 9, 9, 9]
        );
        assert_eq!(
            fs.read_runs_into(&runs, 1020, 8, &mut output, &mut scratch)
                .unwrap(),
            &[9, 9, 9, 9, 0, 0, 0, 0]
        );
        assert_eq!((output.len(), scratch.len()), (4096, 4096));
        assert_eq!(fs.read_at_into(513, 513, &mut scratch).unwrap().len(), 513);
        assert!(fs
            .read_runs_into(&runs, 1535, 2, &mut output, &mut scratch)
            .is_err());
    }

    fn record(attrs: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![0; 1024];
        bytes[..4].copy_from_slice(b"FILE");
        bytes[4..6].copy_from_slice(&48u16.to_le_bytes());
        bytes[6..8].copy_from_slice(&3u16.to_le_bytes());
        bytes[20..22].copy_from_slice(&56u16.to_le_bytes());
        let mut at = 56;
        for attr in attrs {
            bytes[at..at + attr.len()].copy_from_slice(attr);
            at += attr.len();
        }
        bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        for (i, tail) in [510, 1022].into_iter().enumerate() {
            let original = [bytes[tail], bytes[tail + 1]];
            bytes[50 + i * 2..52 + i * 2].copy_from_slice(&original);
            bytes[tail..tail + 2].copy_from_slice(&[0xab, 0xcd]);
        }
        bytes[48..50].copy_from_slice(&[0xab, 0xcd]);
        bytes
    }
    fn data_attr(runlist: &[u8], size: u64) -> Vec<u8> {
        let length = (64 + runlist.len()).div_ceil(8) * 8;
        let mut attr = vec![0; length];
        attr[..4].copy_from_slice(&0x80u32.to_le_bytes());
        attr[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        attr[8] = 1;
        attr[32..34].copy_from_slice(&64u16.to_le_bytes());
        attr[48..56].copy_from_slice(&size.to_le_bytes());
        attr[64..64 + runlist.len()].copy_from_slice(runlist);
        attr
    }

    fn attribute_list(entries: &[(u64, u64)]) -> Vec<u8> {
        let length = 24 + entries.len() * 32;
        let mut attr = vec![0; length];
        attr[..4].copy_from_slice(&0x20u32.to_le_bytes());
        attr[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        attr[16..20].copy_from_slice(&((length - 24) as u32).to_le_bytes());
        attr[20..22].copy_from_slice(&24u16.to_le_bytes());
        for (entry, &(vcn, number)) in attr[24..].as_chunks_mut::<32>().0.iter_mut().zip(entries) {
            entry[..4].copy_from_slice(&0x80u32.to_le_bytes());
            entry[4..6].copy_from_slice(&32u16.to_le_bytes());
            entry[8..16].copy_from_slice(&vcn.to_le_bytes());
            entry[16..24].copy_from_slice(&number.to_le_bytes());
        }
        attr
    }

    fn pagefile_disk() -> Vec<u8> {
        let mut disk = vec![0; 256 * 512];
        disk[3..11].copy_from_slice(b"NTFS    ");
        disk[11..13].copy_from_slice(&512u16.to_le_bytes());
        disk[13] = 1;
        disk[48..56].copy_from_slice(&8u64.to_le_bytes());
        disk[64] = (-10i8) as u8;
        disk[4096..5120].copy_from_slice(&record(&[data_attr(&[0x11, 64, 8, 0], 32768)]));
        let name: Vec<_> = "pagefile.sys"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        let key_size = 66 + name.len();
        let entry_size = (16 + key_size).div_ceil(8) * 8;
        let mut index = vec![0; 32 + entry_size + 16];
        index[8..12].copy_from_slice(&4096u32.to_le_bytes());
        index[16..20].copy_from_slice(&16u32.to_le_bytes());
        let index_length = (index.len() - 16) as u32;
        index[20..24].copy_from_slice(&index_length.to_le_bytes());
        index[32..40].copy_from_slice(&9u64.to_le_bytes());
        index[40..42].copy_from_slice(&(entry_size as u16).to_le_bytes());
        index[42..44].copy_from_slice(&(key_size as u16).to_le_bytes());
        index[48 + 64] = (name.len() / 2) as u8;
        index[48 + 66..48 + key_size].copy_from_slice(&name);
        index[32 + entry_size + 8..32 + entry_size + 10].copy_from_slice(&16u16.to_le_bytes());
        index[32 + entry_size + 12] = 2;
        let length = 24 + index.len();
        let mut attr = vec![0; length];
        attr[..4].copy_from_slice(&0x90u32.to_le_bytes());
        attr[4..8].copy_from_slice(&(length as u32).to_le_bytes());
        attr[16..20].copy_from_slice(&(index.len() as u32).to_le_bytes());
        attr[20..22].copy_from_slice(&24u16.to_le_bytes());
        attr[24..].copy_from_slice(&index);
        disk[4096 + 5 * 1024..4096 + 6 * 1024].copy_from_slice(&record(&[attr]));
        disk[4096 + 9 * 1024..4096 + 10 * 1024]
            .copy_from_slice(&record(&[data_attr(&[0x11, 1, 100, 0x11, 1, 30, 0], 1024)]));
        disk[100 * 512..101 * 512].fill(1);
        disk[130 * 512..131 * 512].fill(2);
        disk
    }

    #[test]
    fn discovers_and_reads_fragmented_pagefile_from_ntfs_metadata() {
        let mut ntfs = Ntfs::new(std::io::Cursor::new(pagefile_disk())).unwrap();
        let (size, extents) = ntfs.pagefile().unwrap();
        assert_eq!(size, 1024);
        assert_eq!(extents.len(), 2);
        let mut output = Vec::new();
        let mut scratch = Vec::new();
        let direct = ntfs
            .read_runs_into(&extents, 10, 20, &mut output, &mut scratch)
            .unwrap();
        assert_eq!(direct, &[1; 20]);
        let direct_address = direct.as_ptr();
        assert_eq!(
            output.capacity(),
            0,
            "contiguous reads must not allocate a second buffer"
        );
        assert!(direct_address >= scratch.as_ptr());
        assert!(direct_address < scratch.as_ptr().wrapping_add(scratch.len()));
        assert_eq!(
            &ntfs.read_runs(&extents, 508, 8).unwrap()[..],
            &[1, 1, 1, 1, 2, 2, 2, 2]
        );
    }

    #[test]
    fn discovers_pagefile_when_data_starts_in_an_extension_record() {
        let mut disk = pagefile_disk();
        disk.copy_within(4096 + 9 * 1024..4096 + 10 * 1024, 4096 + 10 * 1024);
        disk[4096 + 9 * 1024..4096 + 10 * 1024]
            .copy_from_slice(&record(&[attribute_list(&[(0, 10)])]));
        let mut ntfs = Ntfs::new(std::io::Cursor::new(disk)).unwrap();
        let (size, extents) = ntfs.pagefile().unwrap();
        assert_eq!(size, 1024);
        assert_eq!(extents.len(), 2);
        assert_eq!(
            &ntfs.read_runs(&extents, 508, 8).unwrap()[..],
            &[1, 1, 1, 1, 2, 2, 2, 2]
        );
    }

    #[test]
    fn pagefile_size_ignores_continuation_extent_headers() {
        for first_in_base in [true, false] {
            let mut disk = pagefile_disk();
            let first = data_attr(&[0x11, 1, 100, 0], 1024);
            let mut continuation = data_attr(&[0x21, 1, 130, 0, 0], u64::MAX);
            continuation[16..24].copy_from_slice(&1u64.to_le_bytes());
            continuation[24..32].copy_from_slice(&1u64.to_le_bytes());
            let (base, extension, entries) = if first_in_base {
                (first, continuation, [(0, 9), (1, 10)])
            } else {
                (continuation, first, [(0, 10), (1, 9)])
            };
            disk[4096 + 9 * 1024..4096 + 10 * 1024]
                .copy_from_slice(&record(&[base, attribute_list(&entries)]));
            disk[4096 + 10 * 1024..4096 + 11 * 1024].copy_from_slice(&record(&[extension]));
            let mut ntfs = Ntfs::new(std::io::Cursor::new(disk)).unwrap();
            let (size, extents) = ntfs.pagefile().unwrap();
            assert_eq!(size, 1024, "first_in_base={first_in_base}");
            assert_eq!(
                &ntfs.read_runs(&extents, 508, 8).unwrap()[..],
                &[1, 1, 1, 1, 2, 2, 2, 2]
            );
        }
    }

    #[test]
    fn runlists_handle_negative_deltas_sparse_runs_and_truncation() {
        let data = [0x11, 3, 100, 0x11, 2, 0xf6, 0x01, 4, 0];
        assert_eq!(
            runs(&data, 7).unwrap(),
            vec![
                Run {
                    vcn: 7,
                    lcn: Some(100),
                    count: 3
                },
                Run {
                    vcn: 10,
                    lcn: Some(90),
                    count: 2
                },
                Run {
                    vcn: 12,
                    lcn: None,
                    count: 4
                }
            ]
        );
        assert!(runs(&[0x11, 1], 0).is_err());
        assert!(runs(&[0x99], 0).is_err());
    }
    #[test]
    fn torn_file_records_are_rejected() {
        let mut data = vec![0; 1024];
        data[4..6].copy_from_slice(&48u16.to_le_bytes());
        data[6..8].copy_from_slice(&3u16.to_le_bytes());
        data[48..54].copy_from_slice(&[9, 9, 1, 2, 3, 4]);
        data[510..512].copy_from_slice(&[9, 9]);
        assert!(fixup(&mut data.clone(), 512).is_err());
        data[1022..1024].copy_from_slice(&[9, 9]);
        fixup(&mut data, 512).unwrap();
        assert_eq!(&data[510..512], &[1, 2]);
        assert_eq!(&data[1022..1024], &[3, 4]);
    }
}
