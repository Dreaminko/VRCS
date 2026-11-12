//! Length-prefixed protocol between Core and the privileged packet helper.
//! Only authenticated keys and already-attributed datagrams cross this boundary.
use std::io::{self, Read, Write};
use zeroize::Zeroizing;

pub mod packet;
#[cfg(target_os = "linux")]
pub mod procfs;

pub const VERSION: u16 = 1;
const MAX_FRAME: usize = 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Packet {
    pub monotonic_us: u64,
    pub incoming: bool,
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub challenge: [u8; 4],
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanProgress {
    pub request: u64,
    pub scanned: u64,
    pub total: u64,
    pub candidates: u64,
    pub elapsed_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScanResult {
    pub request: u64,
    pub key: Option<[u8; 32]>,
    pub scanned: u64,
    pub candidates: u64,
    pub elapsed_ms: u64,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Scan { request: u64, packets: Vec<Vec<u8>> },
    Cancel { request: u64 },
    Shutdown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Hello {
        version: u16,
        token: String,
        pid: u32,
        starttime: u64,
        monotonic_us: u64,
    },
    Packet(Packet),
    ScanProgress(ScanProgress),
    ScanResult(ScanResult),
    Status {
        fatal: bool,
        detail: String,
    },
}

pub fn write_command(writer: &mut impl Write, command: &Command) -> io::Result<()> {
    let mut body = Vec::new();
    match command {
        Command::Scan { request, packets } => {
            body.push(1);
            body.extend_from_slice(&request.to_le_bytes());
            body.extend_from_slice(&(packets.len() as u32).to_le_bytes());
            for packet in packets {
                let length = u32::try_from(packet.len())
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "packet too large"))?;
                body.extend_from_slice(&length.to_le_bytes());
                body.extend_from_slice(packet);
            }
        }
        Command::Cancel { request } => {
            body.push(2);
            body.extend_from_slice(&request.to_le_bytes());
        }
        Command::Shutdown => body.push(3),
    }
    write_frame(writer, &body)
}

pub fn read_command(reader: &mut impl Read) -> io::Result<Option<Command>> {
    let Some(body) = read_frame(reader)? else {
        return Ok(None);
    };
    let mut cursor = Cursor::new(&body);
    let command = match cursor.byte()? {
        1 => {
            let request = cursor.u64()?;
            let count = cursor.u32()? as usize;
            if count > 8 {
                return Err(invalid("too many scan packets"));
            }
            let mut packets = Vec::with_capacity(count);
            for _ in 0..count {
                packets.push(cursor.bytes()?.to_vec());
            }
            Command::Scan { request, packets }
        }
        2 => Command::Cancel {
            request: cursor.u64()?,
        },
        3 => Command::Shutdown,
        _ => return Err(invalid("unknown command")),
    };
    if cursor.remaining() != 0 {
        return Err(invalid("trailing command bytes"));
    }
    Ok(Some(command))
}

pub fn write_event(writer: &mut impl Write, event: &Event) -> io::Result<()> {
    let capacity = match event {
        Event::ScanResult(result) => 70 + result.error.as_ref().map_or(0, String::len),
        _ => 0,
    };
    let mut body = Zeroizing::new(Vec::with_capacity(capacity));
    match event {
        Event::Hello {
            version,
            token,
            pid,
            starttime,
            monotonic_us,
        } => {
            body.push(10);
            body.extend_from_slice(&version.to_be_bytes());
            put_string(&mut body, token)?;
            body.extend_from_slice(&pid.to_le_bytes());
            body.extend_from_slice(&starttime.to_le_bytes());
            body.extend_from_slice(&monotonic_us.to_le_bytes());
        }
        Event::Packet(packet) => {
            body.push(11);
            body.extend_from_slice(&packet.monotonic_us.to_le_bytes());
            body.push(u8::from(packet.incoming));
            body.extend_from_slice(&packet.local_ip);
            body.extend_from_slice(&packet.local_port.to_be_bytes());
            body.extend_from_slice(&packet.remote_ip);
            body.extend_from_slice(&packet.remote_port.to_be_bytes());
            body.extend_from_slice(&packet.challenge);
            let length = u32::try_from(packet.payload.len())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "packet too large"))?;
            body.extend_from_slice(&length.to_le_bytes());
            body.extend_from_slice(&packet.payload);
        }
        Event::ScanProgress(progress) => {
            body.push(12);
            body.extend_from_slice(&progress.request.to_le_bytes());
            for value in [
                progress.scanned,
                progress.total,
                progress.candidates,
                progress.elapsed_ms,
            ] {
                body.extend_from_slice(&value.to_le_bytes());
            }
        }
        Event::ScanResult(result) => {
            body.push(13);
            body.extend_from_slice(&result.request.to_le_bytes());
            body.push(u8::from(result.key.is_some()));
            body.extend_from_slice(&result.scanned.to_le_bytes());
            body.extend_from_slice(&result.candidates.to_le_bytes());
            body.extend_from_slice(&result.elapsed_ms.to_le_bytes());
            body.extend_from_slice(result.key.as_ref().unwrap_or(&[0; 32]));
            put_string(&mut body, result.error.as_deref().unwrap_or(""))?;
        }
        Event::Status { fatal, detail } => {
            body.push(14);
            body.push(u8::from(*fatal));
            put_string(&mut body, detail)?;
        }
    }
    write_frame(writer, &body)
}

pub fn read_event(reader: &mut impl Read) -> io::Result<Option<Event>> {
    let Some(body) = read_frame(reader)? else {
        return Ok(None);
    };
    let mut cursor = Cursor::new(&body);
    let event = match cursor.byte()? {
        10 => Event::Hello {
            version: cursor.u16()?,
            token: cursor.string()?,
            pid: cursor.u32()?,
            starttime: cursor.u64()?,
            monotonic_us: cursor.u64()?,
        },
        11 => {
            let monotonic_us = cursor.u64()?;
            let incoming = cursor.byte()? != 0;
            let local_ip = cursor.array()?;
            let local_port = cursor.u16()?;
            let remote_ip = cursor.array()?;
            let remote_port = cursor.u16()?;
            let challenge = cursor.array()?;
            let payload = cursor.bytes()?.to_vec();
            Event::Packet(Packet {
                monotonic_us,
                incoming,
                local_ip,
                local_port,
                remote_ip,
                remote_port,
                challenge,
                payload,
            })
        }
        12 => Event::ScanProgress(ScanProgress {
            request: cursor.u64()?,
            scanned: cursor.u64()?,
            total: cursor.u64()?,
            candidates: cursor.u64()?,
            elapsed_ms: cursor.u64()?,
        }),
        13 => {
            let request = cursor.u64()?;
            let present = cursor.byte()? != 0;
            let scanned = cursor.u64()?;
            let candidates = cursor.u64()?;
            let elapsed_ms = cursor.u64()?;
            let key = cursor.array()?;
            let error = cursor.string()?;
            Event::ScanResult(ScanResult {
                request,
                key: present.then_some(key),
                scanned,
                candidates,
                elapsed_ms,
                error: (!error.is_empty()).then_some(error),
            })
        }
        14 => Event::Status {
            fatal: cursor.byte()? != 0,
            detail: cursor.string()?,
        },
        _ => return Err(invalid("unknown event")),
    };
    if cursor.remaining() != 0 {
        return Err(invalid("trailing event bytes"));
    }
    Ok(Some(event))
}

fn write_frame(writer: &mut impl Write, body: &[u8]) -> io::Result<()> {
    let length = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    writer.write_all(&length.to_le_bytes())?;
    writer.write_all(body)?;
    writer.flush()
}

fn read_frame(reader: &mut impl Read) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
    let mut header = [0; 4];
    let mut read = 0;
    while read < header.len() {
        match reader.read(&mut header[read..])? {
            0 if read == 0 => return Ok(None),
            0 => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "short frame")),
            count => read += count,
        }
    }
    let length = u32::from_le_bytes(header) as usize;
    if length > MAX_FRAME {
        return Err(invalid("frame too large"));
    }
    let mut body = Zeroizing::new(vec![0; length]);
    reader.read_exact(&mut body)?;
    Ok(Some(body))
}

fn put_string(body: &mut Vec<u8>, value: &str) -> io::Result<()> {
    let bytes = value.as_bytes();
    let length = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "string too large"))?;
    body.extend_from_slice(&length.to_le_bytes());
    body.extend_from_slice(bytes);
    Ok(())
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }
    fn remaining(&self) -> usize {
        self.bytes.len() - self.at
    }
    fn take(&mut self, length: usize) -> io::Result<&'a [u8]> {
        let end = self
            .at
            .checked_add(length)
            .filter(|end| *end <= self.bytes.len())
            .ok_or_else(|| invalid("truncated frame"))?;
        let value = &self.bytes[self.at..end];
        self.at = end;
        Ok(value)
    }
    fn byte(&mut self) -> io::Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> io::Result<u16> {
        Ok(u16::from_be_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn u64(&mut self) -> io::Result<u64> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    fn array<const N: usize>(&mut self) -> io::Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn bytes(&mut self) -> io::Result<&'a [u8]> {
        let length = self.u32()? as usize;
        self.take(length)
    }
    fn string(&mut self) -> io::Result<String> {
        String::from_utf8(self.bytes()?.to_vec())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid string"))
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_round_trips_packets_and_scan_requests() {
        let packet = Packet {
            monotonic_us: 42,
            incoming: true,
            local_ip: [192, 0, 2, 2],
            local_port: 53000,
            remote_ip: [203, 0, 113, 1],
            remote_port: 27002,
            challenge: [1, 2, 3, 4],
            payload: vec![7; 40],
        };
        let hello = Event::Hello {
            version: VERSION,
            token: "token".into(),
            pid: 7,
            starttime: 11,
            monotonic_us: 13,
        };
        let mut wire = Vec::new();
        write_event(&mut wire, &hello).unwrap();
        assert_eq!(read_event(&mut &wire[..]).unwrap(), Some(hello));

        let mut wire = Vec::new();
        write_event(&mut wire, &Event::Packet(packet.clone())).unwrap();
        assert_eq!(
            read_event(&mut &wire[..]).unwrap(),
            Some(Event::Packet(packet))
        );

        let command = Command::Scan {
            request: 9,
            packets: vec![vec![1; 40], vec![2; 41]],
        };
        let mut wire = Vec::new();
        write_command(&mut wire, &command).unwrap();
        assert_eq!(read_command(&mut &wire[..]).unwrap(), Some(command));
    }
}
