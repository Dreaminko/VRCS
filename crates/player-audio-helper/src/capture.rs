use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use player_audio_helper::packet::{self, PHOTON_PORT};
use player_audio_helper::procfs;
use player_audio_helper::{write_event, Event, Packet};

const SO_LOCK_FILTER: libc::c_int = 36;

pub struct Capture {
    fd: libc::c_int,
    pid: u32,
    starttime: u64,
    stop: Arc<AtomicBool>,
}

impl Capture {
    pub fn open(pid: u32, starttime: u64) -> Result<Self, String> {
        let fd = unsafe {
            libc::socket(
                libc::AF_PACKET,
                libc::SOCK_DGRAM | libc::SOCK_NONBLOCK | libc::SOCK_CLOEXEC,
                0, // Enable reception only after installing and locking the filter.
            )
        };
        if fd < 0 {
            return Err(format!(
                "Cannot open Linux packet capture (CAP_NET_RAW required): {}",
                io::Error::last_os_error()
            ));
        }
        let capture = Self {
            fd,
            pid,
            starttime,
            stop: Arc::new(AtomicBool::new(false)),
        };
        capture.configure()?;
        Ok(capture)
    }

    fn configure(&self) -> Result<(), String> {
        let size: libc::c_int = 8 * 1024 * 1024;
        let result = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUF,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(format!(
                "Cannot configure packet receive buffer: {}",
                io::Error::last_os_error()
            ));
        }
        let mut filter = instructions();
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        let result = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                (&program as *const libc::sock_fprog).cast(),
                std::mem::size_of_val(&program) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(format!(
                "Cannot install packet filter: {}",
                io::Error::last_os_error()
            ));
        }
        let locked: libc::c_int = 1;
        let result = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_SOCKET,
                SO_LOCK_FILTER,
                (&locked as *const libc::c_int).cast(),
                std::mem::size_of_val(&locked) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(format!(
                "Cannot lock packet filter: {}",
                io::Error::last_os_error()
            ));
        }
        let mut address: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
        address.sll_family = libc::AF_PACKET as u16;
        address.sll_protocol = (libc::ETH_P_ALL as u16).to_be();
        let result = unsafe {
            libc::bind(
                self.fd,
                (&address as *const libc::sockaddr_ll).cast(),
                std::mem::size_of_val(&address) as libc::socklen_t,
            )
        };
        if result != 0 {
            return Err(format!(
                "Cannot enable packet capture: {}",
                io::Error::last_os_error()
            ));
        }
        Ok(())
    }

    pub fn run(&self, writer: &Mutex<std::os::unix::net::UnixStream>) -> Result<(), String> {
        let mut buffer = vec![0u8; 65_536];
        let mut endpoints = Vec::new();
        let mut refreshed = Instant::now() - Duration::from_secs(2);
        let mut alive_checked = refreshed;
        while !self.stop.load(Ordering::Relaxed) {
            if refreshed.elapsed() >= Duration::from_secs(1) {
                endpoints = procfs::udp_endpoints(self.pid);
                refreshed = Instant::now();
            }
            if alive_checked.elapsed() >= Duration::from_secs(1) {
                if !procfs::alive(self.pid, self.starttime) {
                    return Err("VRChat exited".into());
                }
                alive_checked = Instant::now();
            }
            let mut received = 0;
            for _ in 0..128 {
                match receive(self.fd, &mut buffer) {
                    Ok(Some((bytes, outgoing))) => {
                        received += 1;
                        let Some(packet) = parse(bytes, outgoing, &endpoints) else {
                            continue;
                        };
                        if write_event(&mut *writer.lock().unwrap(), &Event::Packet(packet))
                            .is_err()
                        {
                            return Err("Packet event channel closed".into());
                        }
                    }
                    Ok(None) => break,
                    Err(error) => return Err(error),
                }
            }
            if received == 0 {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        Ok(())
    }

    pub fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stop)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

fn filter(code: u32, jt: u8, jf: u8, k: u32) -> libc::sock_filter {
    libc::sock_filter {
        code: code as u16,
        jt,
        jf,
        k,
    }
}

fn instructions() -> [libc::sock_filter; 12] {
    use libc::{
        BPF_ABS, BPF_ALU, BPF_AND, BPF_B, BPF_H, BPF_IND, BPF_JEQ, BPF_JMP, BPF_K, BPF_LD, BPF_LDX,
        BPF_MSH, BPF_RET,
    };
    [
        filter(BPF_LD | BPF_B | BPF_ABS, 0, 0, 0),
        filter(BPF_ALU | BPF_AND | BPF_K, 0, 0, 0xf0),
        filter(BPF_JMP | BPF_JEQ | BPF_K, 0, 7, 0x40), // IPv4
        filter(BPF_LD | BPF_B | BPF_ABS, 0, 0, 9),
        filter(BPF_JMP | BPF_JEQ | BPF_K, 0, 5, 17), // UDP
        filter(BPF_LDX | BPF_B | BPF_MSH, 0, 0, 0),  // x = 4 * IHL
        filter(BPF_LD | BPF_H | BPF_IND, 0, 0, 2),   // destination port
        filter(BPF_JMP | BPF_JEQ | BPF_K, 3, 0, u32::from(PHOTON_PORT)),
        filter(BPF_LD | BPF_H | BPF_IND, 0, 0, 0), // source port
        filter(BPF_JMP | BPF_JEQ | BPF_K, 1, 0, u32::from(PHOTON_PORT)),
        filter(BPF_RET | BPF_K, 0, 0, 0),
        filter(BPF_RET | BPF_K, 0, 0, 65_535),
    ]
}

fn receive(fd: libc::c_int, buffer: &mut [u8]) -> Result<Option<(&[u8], bool)>, String> {
    let mut address: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    let mut length = std::mem::size_of::<libc::sockaddr_ll>() as libc::socklen_t;
    let count = unsafe {
        libc::recvfrom(
            fd,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            0,
            (&mut address as *mut libc::sockaddr_ll).cast(),
            &mut length,
        )
    };
    if count < 0 {
        let error = io::Error::last_os_error();
        return match error.raw_os_error() {
            Some(libc::EAGAIN) => Ok(None),
            _ => Err(format!("Linux packet capture failed: {error}")),
        };
    }
    Ok(Some((
        &buffer[..count as usize],
        address.sll_pkttype == libc::PACKET_OUTGOING,
    )))
}

fn parse(frame: &[u8], outgoing: bool, endpoints: &[procfs::Endpoint]) -> Option<Packet> {
    let (bytes, offset) = frame_layout(frame)?;
    let bytes = bytes.get(offset..)?;
    let datagram = packet::ipv4_udp(bytes)?;
    let source_ip = datagram.source.ip().octets();
    let source_port = datagram.source.port();
    let target_ip = datagram.destination.ip().octets();
    let target_port = datagram.destination.port();
    let (local_ip, local_port, remote_ip, remote_port, incoming) = if source_port == PHOTON_PORT
        && procfs::owns_flow(endpoints, target_ip, target_port, source_ip, source_port)
    {
        (target_ip, target_port, source_ip, source_port, true)
    } else if target_port == PHOTON_PORT
        && procfs::owns_flow(endpoints, source_ip, source_port, target_ip, target_port)
    {
        (source_ip, source_port, target_ip, target_port, false)
    } else {
        return None;
    };
    if incoming == outgoing {
        // AF_PACKET metadata and the endpoint owner must agree.
        return None;
    }
    let payload = datagram.payload;
    if !packet::encrypted(payload) {
        return None;
    }
    Some(Packet {
        monotonic_us: procfs::monotonic_us(),
        incoming,
        local_ip,
        local_port,
        remote_ip,
        remote_port,
        challenge: payload[3..7].try_into().ok()?,
        payload: payload.to_vec(),
    })
}

fn frame_layout(frame: &[u8]) -> Option<(&[u8], usize)> {
    if frame.first().is_some_and(|byte| byte >> 4 == 4) {
        return Some((frame, 0));
    }
    if frame.len() < 14 {
        return None;
    }
    let mut ethertype = u16::from_be_bytes([frame[12], frame[13]]);
    let mut offset = 14;
    if ethertype == 0x8100 && frame.len() >= 18 {
        ethertype = u16::from_be_bytes([frame[16], frame[17]]);
        offset = 18;
    }
    (ethertype == 0x0800).then_some((frame, offset))
}
#[cfg(test)]
mod tests {
    use super::*;

    fn accepts(frame: &[u8]) -> bool {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;

        let (sender, receiver) = UnixDatagram::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        let mut filter = instructions();
        let program = libc::sock_fprog {
            len: filter.len() as u16,
            filter: filter.as_mut_ptr(),
        };
        let result = unsafe {
            libc::setsockopt(
                receiver.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                (&program as *const libc::sock_fprog).cast(),
                std::mem::size_of_val(&program) as libc::socklen_t,
            )
        };
        assert_eq!(result, 0, "{}", io::Error::last_os_error());
        sender.send(frame).unwrap();
        let mut received = vec![0; frame.len()];
        match receiver.recv(&mut received) {
            Ok(length) => {
                assert_eq!(&received[..length], frame);
                true
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => false,
            Err(error) => panic!("cannot receive filtered packet: {error}"),
        }
    }

    fn ipv4_udp(ihl: usize, source: u16, target: u16) -> Vec<u8> {
        let mut payload = vec![0; 40];
        payload[2] = 1;
        payload[3..7].copy_from_slice(&[7, 8, 9, 10]);
        ipv4_udp_payload(ihl, source, target, &payload)
    }

    fn ipv4_udp_payload(ihl: usize, source: u16, target: u16, payload: &[u8]) -> Vec<u8> {
        let mut packet = vec![0; ihl * 4];
        packet[0] = 0x40 | ihl as u8;
        packet[9] = 17;
        packet[12..16].copy_from_slice(&[192, 0, 2, 2]);
        packet[16..20].copy_from_slice(&[203, 0, 113, 1]);
        if ihl > 5 {
            packet[20..22].copy_from_slice(&27002u16.to_be_bytes());
        }
        let mut udp = vec![0; 8];
        udp[0..2].copy_from_slice(&source.to_be_bytes());
        udp[2..4].copy_from_slice(&target.to_be_bytes());
        udp[4..6].copy_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
        packet.extend(udp);
        packet.extend(payload);
        let total = packet.len() as u16;
        packet[2..4].copy_from_slice(&total.to_be_bytes());
        packet
    }

    #[test]
    fn classic_filter_follows_ip_header_length_and_both_directions() {
        assert!(accepts(&ipv4_udp(5, 27002, 53000)));
        assert!(accepts(&ipv4_udp(6, 53000, 27002)));
        assert!(!accepts(&ipv4_udp(6, 28000, 41000)));
        let mut unrelated = ipv4_udp(5, 27002, 53000);
        unrelated[9] = 6;
        assert!(!accepts(&unrelated));
    }
}
#[cfg(test)]
mod parse_tests {
    use super::*;

    fn packet(incoming: bool) -> Vec<u8> {
        let mut payload = vec![0; 40];
        payload[2] = 1;
        payload[3..7].copy_from_slice(&[7, 8, 9, 10]);
        let mut frame = vec![0; 20];
        frame[0] = 0x45;
        frame[9] = 17;
        frame[12..16].copy_from_slice(&[192, 0, 2, 2]);
        frame[16..20].copy_from_slice(&[203, 0, 113, 1]);
        let (source, target): (u16, u16) = if incoming {
            (27002, 53000)
        } else {
            (53000, 27002)
        };
        frame.extend(source.to_be_bytes());
        frame.extend(target.to_be_bytes());
        frame.extend(((payload.len() + 8) as u16).to_be_bytes());
        frame.extend([0, 0]);
        frame.extend(payload);
        let total = frame.len() as u16;
        frame[2..4].copy_from_slice(&total.to_be_bytes());
        frame
    }

    #[test]
    fn direction_requires_the_game_endpoint_and_keeps_the_challenge() {
        let endpoints = vec![procfs::Endpoint {
            local_ip: [0; 4],
            local_port: 53000,
            remote_ip: [0; 4],
            remote_port: 0,
            inode: 1,
            owned: true,
        }];
        let incoming = parse(&packet(true), false, &endpoints).unwrap();
        assert!(incoming.incoming);
        assert_eq!(incoming.local_port, 53000);
        assert_eq!(incoming.challenge, [7, 8, 9, 10]);
        assert!(parse(&packet(true), true, &endpoints).is_none());
        assert!(parse(&packet(false), false, &[]).is_none());
        assert!(!parse(&packet(false), true, &endpoints).unwrap().incoming);
    }
}
