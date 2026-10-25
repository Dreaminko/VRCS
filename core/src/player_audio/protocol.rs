//! Bounded Photon v5 / USpeak parsing.
use std::collections::HashMap;
use std::time::{Duration, Instant};

const MAX_MESSAGE: usize = 1024 * 1024;
// Large room property snapshots exceed 64 MTU-sized fragments. The total byte
// and concurrent-assembly limits still bound allocation independently.
const MAX_FRAGMENTS: usize = 1024;
const MAX_ASSEMBLIES: usize = 16;
const FRAGMENT_TTL: Duration = Duration::from_secs(2);

#[derive(Debug, PartialEq)]
pub(super) struct VoicePacket {
    pub actor: u16,
    pub sequence: u16,
    pub opus: Vec<u8>,
}

struct Assembly {
    created: Instant,
    total: usize,
    count: usize,
    parts: HashMap<u32, (usize, Vec<u8>)>,
}

/// One parser per connection and direction; fragment sequence numbers are local.
#[derive(Default)]
pub(super) struct Parser {
    fragments: HashMap<(u8, u32), Assembly>,
}

impl Parser {
    pub(super) fn messages(&mut self, plaintext: &[u8], now: Instant) -> Vec<Vec<u8>> {
        self.fragments
            .retain(|_, v| now.saturating_duration_since(v.created) < FRAGMENT_TTL);
        let mut messages = Vec::new();
        if plaintext.len() < 5 {
            return messages;
        }
        let mut offset = 5;
        for _ in 0..plaintext[0] {
            let Some(header) = plaintext.get(offset..offset + 12) else {
                break;
            };
            // Photon stores the entire command length, including this 12-byte
            // header. Using it as payload length loses coalesced voice commands.
            let total = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
            let Some(length) = total.checked_sub(12) else {
                break;
            };
            if length > MAX_MESSAGE {
                break;
            }
            offset += 12;
            let Some(payload) = plaintext.get(offset..offset + length) else {
                break;
            };
            offset += length;
            if header[0] != 8 {
                messages.push(payload.to_vec());
                continue;
            }
            if payload.len() < 20 {
                continue;
            }
            let field = |i| u32::from_be_bytes(payload[i..i + 4].try_into().unwrap());
            let (start, count, index, total, at) =
                (field(0), field(4), field(8), field(12), field(16));
            let (count, total, at) = (count as usize, total as usize, at as usize);
            if !(1..=MAX_FRAGMENTS).contains(&count)
                || index as usize >= count
                || total == 0
                || total > MAX_MESSAGE
                || at > total
                || payload.len() - 20 > total - at
            {
                continue;
            }
            let key = (header[1], start);
            if !self.fragments.contains_key(&key) && self.fragments.len() >= MAX_ASSEMBLIES {
                continue;
            }
            let entry = self.fragments.entry(key).or_insert_with(|| Assembly {
                created: now,
                total,
                count,
                parts: HashMap::new(),
            });
            if entry.total != total || entry.count != count {
                self.fragments.remove(&key);
                continue;
            }
            entry
                .parts
                .entry(index)
                .or_insert_with(|| (at, payload[20..].to_vec()));
            if entry.parts.len() != count {
                continue;
            }
            let entry = self.fragments.remove(&key).unwrap();
            let mut parts: Vec<_> = entry.parts.into_values().collect();
            parts.sort_by_key(|(at, _)| *at);
            let mut data = Vec::with_capacity(total);
            for (at, bytes) in parts {
                if at != data.len() {
                    break;
                }
                data.extend(bytes);
            }
            if data.len() == total {
                messages.push(data);
            }
        }
        messages
    }
}

fn varint(bytes: &[u8], cursor: &mut usize) -> Option<usize> {
    let mut value = 0usize;
    for shift in (0..35).step_by(7) {
        let byte = *bytes.get(*cursor)?;
        *cursor += 1;
        if shift == 28 && byte > 15 {
            return None;
        }
        value |= usize::from(byte & 127) << shift;
        if byte & 128 == 0 {
            return Some(value);
        }
    }
    None
}

pub(super) fn voice(message: &[u8]) -> Option<Vec<VoicePacket>> {
    // Reliable/unreliable messages can have a short sequence prefix.
    let at = message.windows(2).take(9).position(|v| v == [0xf3, 4])?;
    let bytes = &message[at..];
    if *bytes.get(2)? != 1 {
        return None;
    }
    let count = *bytes.get(3)?;
    let mut cursor = 4;
    let mut actor = None;
    let mut audio = None;
    for _ in 0..count {
        let code = *bytes.get(cursor)?;
        let kind = *bytes.get(cursor + 1)?;
        cursor += 2;
        match kind {
            0x43 => {
                let length = varint(bytes, &mut cursor)?;
                let data = bytes.get(cursor..cursor.checked_add(length)?)?;
                cursor += length;
                if code == 245 {
                    audio = Some(data);
                }
            }
            0x0d => {
                let value = i16::from_le_bytes(bytes.get(cursor..cursor + 2)?.try_into().ok()?);
                cursor += 2;
                if code == 254 {
                    actor = u16::try_from(value).ok();
                }
            }
            0x0b => {
                let value = *bytes.get(cursor)?;
                cursor += 1;
                if code == 254 {
                    actor = Some(u16::from(value));
                }
            }
            0x08 => {}
            _ => return None,
        }
    }
    let actor = actor.filter(|v| *v > 0)?;
    let bytes = audio?;
    let mut cursor = 4;
    let mut packets = Vec::new();
    while cursor < bytes.len() {
        let header = bytes.get(cursor..cursor + 4)?;
        let sequence = u16::from_le_bytes(header[..2].try_into().ok()?);
        let length = u16::from_le_bytes(header[2..].try_into().ok()?) as usize;
        cursor += 4;
        if !(1..=1275).contains(&length) || packets.len() >= 64 {
            return None;
        }
        let opus = bytes.get(cursor..cursor + length)?.to_vec();
        cursor += length;
        packets.push(VoicePacket {
            actor,
            sequence,
            opus,
        });
    }
    Some(packets)
}

pub(super) fn joined_world(message: &[u8]) -> Option<String> {
    let at = message
        .windows(3)
        .take(9)
        .position(|v| v == [0xf3, 2, 226])?;
    let bytes = &message[at + 3..];
    let at = bytes.windows(5).position(|v| v == b"wrld_")?;
    let world: Vec<_> = bytes[at..]
        .iter()
        .copied()
        .take(256)
        .take_while(|b| b.is_ascii_graphic())
        .collect();
    let world = String::from_utf8(world).ok()?;
    (world.len() >= 42 && world.contains(':')).then_some(world)
}

#[cfg(test)]
pub(super) fn voice_fixture(actor: u16, sequence: u16, opus: &[u8]) -> Vec<u8> {
    let mut payload = vec![0; 4];
    payload.extend(sequence.to_le_bytes());
    payload.extend((opus.len() as u16).to_le_bytes());
    payload.extend(opus);
    let mut event = vec![0xf3, 4, 1, 2, 254, 0x0d];
    event.extend(actor.to_le_bytes());
    event.extend([245, 0x43]);
    let mut length = payload.len();
    loop {
        let byte = (length & 127) as u8;
        length >>= 7;
        event.push(byte | if length == 0 { 0 } else { 128 });
        if length == 0 {
            break;
        }
    }
    event.extend(payload);
    let mut datagram = vec![1, 0, 0, 0, 0, 7, 0, 0, 0];
    datagram.extend((event.len() as u32 + 12).to_be_bytes());
    datagram.extend([0; 4]);
    datagram.extend(event);
    datagram
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_room_snapshot_reassembles_more_than_64_fragments_out_of_order() {
        let payload: Vec<u8> = (0..90_000).map(|i| (i % 251) as u8).collect();
        let chunks: Vec<_> = payload.chunks(1200).collect();
        let mut parser = Parser::default();
        let now = Instant::now();
        for index in (0..chunks.len()).rev() {
            let chunk = chunks[index];
            let mut datagram = vec![1, 0, 0, 0, 0, 8, 1, 0, 0];
            datagram.extend((32 + chunk.len() as u32).to_be_bytes());
            datagram.extend([0; 4]);
            for value in [
                17,
                chunks.len() as u32,
                index as u32,
                payload.len() as u32,
                (index * 1200) as u32,
            ] {
                datagram.extend(value.to_be_bytes());
            }
            datagram.extend(chunk);
            let result = parser.messages(&datagram, now);
            if index == 0 {
                assert_eq!(result.as_slice(), std::slice::from_ref(&payload));
            } else {
                assert!(result.is_empty());
            }
        }
    }
    #[test]
    fn separates_players_and_rejects_every_truncated_event() {
        let mut parser = Parser::default();
        for actor in [2, 17] {
            let messages = parser.messages(
                &voice_fixture(actor, 65535, &[0xf8, 0xff, 0xfe]),
                Instant::now(),
            );
            assert_eq!(voice(&messages[0]).unwrap()[0].actor, actor);
            for end in 0..messages[0].len() {
                assert!(voice(&messages[0][..end]).is_none());
            }
        }
    }
    #[test]
    fn hostile_lengths_and_fragment_count_are_bounded() {
        let mut parser = Parser::default();
        let mut data = voice_fixture(1, 1, &[1, 2]);
        data[9..13].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(parser.messages(&data, Instant::now()).is_empty());
        for length in 0..200 {
            let bytes = vec![255; length];
            assert!(parser.messages(&bytes, Instant::now()).is_empty());
            assert!(voice(&bytes).is_none());
        }
    }

    #[test]
    fn command_lengths_include_headers_and_coalesced_voices_are_not_lost() {
        let mut data = voice_fixture(1, 5, &[0xf8, 0xff, 0xfe]);
        let second = voice_fixture(2, 7, &[0xf8, 0xff, 0xfe]);
        data[0] = 2;
        data.extend_from_slice(&second[5..]);
        let messages = Parser::default().messages(&data, Instant::now());
        assert_eq!(messages.len(), 2);
        assert_eq!(voice(&messages[0]).unwrap()[0].actor, 1);
        assert_eq!(voice(&messages[1]).unwrap()[0].actor, 2);
        data.pop();
        assert_eq!(Parser::default().messages(&data, Instant::now()).len(), 1);
    }

    fn fragment(index: u32, offset: u32, payload: &[u8]) -> Vec<u8> {
        let mut data = vec![1, 0, 0, 0, 0, 8, 1, 0, 0];
        data.extend((32 + payload.len() as u32).to_be_bytes());
        data.extend([0; 4]);
        for value in [17u32, 2, index, 6, offset] {
            data.extend(value.to_be_bytes());
        }
        data.extend(payload);
        data
    }

    #[test]
    fn fragments_reorder_expire_and_reject_overlaps() {
        let now = Instant::now();
        let mut parser = Parser::default();
        assert!(parser.messages(&fragment(1, 3, b"def"), now).is_empty());
        assert!(parser.messages(&fragment(1, 3, b"def"), now).is_empty());
        assert_eq!(parser.messages(&fragment(0, 0, b"abc"), now), [b"abcdef"]);
        assert!(parser.messages(&fragment(0, 0, b"abc"), now).is_empty());
        assert!(parser
            .messages(&fragment(1, 3, b"def"), now + FRAGMENT_TTL)
            .is_empty());
        parser = Parser::default();
        parser.messages(&fragment(0, 0, b"abcd"), now);
        assert!(parser.messages(&fragment(1, 3, b"def"), now).is_empty());
        assert!(parser.fragments.is_empty());
    }
}
