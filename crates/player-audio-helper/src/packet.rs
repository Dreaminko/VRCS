//! Platform-neutral IPv4/UDP framing for VRChat Photon voice traffic.
use std::net::{Ipv4Addr, SocketAddrV4};

pub const PHOTON_PORT: u16 = 27002;
const MIN_PACKET: usize = 40;
const MAX_PACKET: usize = 65_507;

pub struct Datagram<'a> {
    pub source: SocketAddrV4,
    pub destination: SocketAddrV4,
    pub payload: &'a [u8],
}

/// Parse one unfragmented IPv4/UDP datagram, preserving its payload slice.
pub fn ipv4_udp(bytes: &[u8]) -> Option<Datagram<'_>> {
    if bytes.len() < 28 || bytes[0] >> 4 != 4 || bytes[9] != 17 {
        return None;
    }
    let header = usize::from(bytes[0] & 15) * 4;
    if header < 20 || bytes.len() < header + 8 {
        return None;
    }
    let total = u16::from_be_bytes([bytes[2], bytes[3]]) as usize;
    let fragments = u16::from_be_bytes([bytes[6], bytes[7]]);
    if total < header + 8 || total > bytes.len() || fragments & 0x3fff != 0 {
        return None;
    }
    let udp = &bytes[header..total];
    let size = u16::from_be_bytes([udp[4], udp[5]]) as usize;
    if size < 8 || size > udp.len() {
        return None;
    }
    Some(Datagram {
        source: SocketAddrV4::new(
            Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]),
            u16::from_be_bytes([udp[0], udp[1]]),
        ),
        destination: SocketAddrV4::new(
            Ipv4Addr::new(bytes[16], bytes[17], bytes[18], bytes[19]),
            u16::from_be_bytes([udp[2], udp[3]]),
        ),
        payload: &udp[8..size],
    })
}

/// Encrypted Photon v5 voice payload.
pub fn encrypted(packet: &[u8]) -> bool {
    (MIN_PACKET..=MAX_PACKET).contains(&packet.len()) && packet[2] == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(ihl: usize, fragment: u16) -> Vec<u8> {
        let mut payload = [7u8; 40];
        payload[2] = 1;
        let mut packet = vec![0; ihl * 4];
        packet[0] = 0x40 | ihl as u8;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[192, 0, 2, 2]);
        packet[16..20].copy_from_slice(&[203, 0, 113, 1]);
        packet[6..8].copy_from_slice(&fragment.to_be_bytes());
        let mut udp = vec![0; 8];
        udp[0..2].copy_from_slice(&27002u16.to_be_bytes());
        udp[2..4].copy_from_slice(&53_000u16.to_be_bytes());
        udp[4..6].copy_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
        packet.extend(udp);
        packet.extend(payload);
        let total = packet.len() as u16;
        packet[2..4].copy_from_slice(&total.to_be_bytes());
        packet
    }

    #[test]
    fn follows_ip_header_length_and_rejects_fragments() {
        let bytes = frame(6, 0);
        let parsed = ipv4_udp(&bytes).unwrap();
        assert_eq!(parsed.source.port(), PHOTON_PORT);
        assert_eq!(parsed.payload.len(), 40);
        assert!(encrypted(parsed.payload));
        assert!(ipv4_udp(&frame(6, 0x2000)).is_none());
        let mut short = frame(5, 0);
        short[0] = 0x44;
        assert!(ipv4_udp(&short).is_none());
    }
}
