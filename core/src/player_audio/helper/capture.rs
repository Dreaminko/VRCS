use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::packet::{self, PHOTON_PORT};
use super::procfs;
use super::{Event, Packet, write_event};

pub(super) struct Capture {
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
        let mut result = unsafe {
            libc::setsockopt(
                self.fd,
                libc::SOL_SOCKET,
                libc::SO_RCVBUFFORCE,
                (&size as *const libc::c_int).cast(),
                std::mem::size_of_val(&size) as libc::socklen_t,
            )
        };
        if result != 0 {
            result = unsafe {
                libc::setsockopt(
                    self.fd,
                    libc::SOL_SOCKET,
                    libc::SO_RCVBUF,
                    (&size as *const libc::c_int).cast(),
                    std::mem::size_of_val(&size) as libc::socklen_t,
                )
            };
        }
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
                libc::SO_LOCK_FILTER,
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
        let mut flow_cache = procfs::FlowCache::default();
        let mut alive_checked = Instant::now() - Duration::from_secs(2);
        let mut draining = true;
        while !self.stop.load(Ordering::Relaxed) {
            if let Ok(changed) = flow_cache.refresh_if_due(self.pid) {
                if changed {
                    // Only a changed owned inventory can admit newly attributed flows.
                    // Drain old frames before accepting the new ownership snapshot.
                    draining = true;
                }
            }
            if alive_checked.elapsed() >= Duration::from_secs(1) {
                if let Err(error) = procfs::check_identity(self.pid, self.starttime) {
                    return if procfs::exited(self.pid, self.starttime) {
                        Ok(())
                    } else {
                        Err(format!("Cannot inspect VRChat process: {error}"))
                    };
                }
                alive_checked = Instant::now();
            }
            let mut empty = false;
            for _ in 0..128 {
                match receive(self.fd, &mut buffer)? {
                    Receive::Frame(bytes, outgoing) => {
                        if draining {
                            continue;
                        }
                        let Some(candidate) = parse(bytes, outgoing, &mut flow_cache, self.pid)
                        else {
                            continue;
                        };
                        let flow = &candidate.flow;
                        let packet = Packet {
                            monotonic_us: procfs::monotonic_us(),
                            incoming: candidate.incoming,
                            local_ip: flow.local_ip,
                            local_port: flow.local_port,
                            remote_ip: flow.remote_ip,
                            remote_port: flow.remote_port,
                            challenge: candidate.payload[3..7].try_into().unwrap(),
                            payload: candidate.payload.to_vec(),
                        };
                        let mut writer = writer.lock().unwrap();
                        if write_event(&mut *writer, &Event::Packet(packet)).is_err() {
                            return Err("Packet event channel closed".into());
                        }
                    }
                    Receive::Skip => {}
                    Receive::Empty => {
                        draining = false;
                        empty = true;
                        break;
                    }
                }
            }
            if empty {
                wait_readable(self.fd, 200)?;
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

fn instructions() -> [libc::sock_filter; 16] {
    use libc::{
        BPF_ABS, BPF_ALU, BPF_AND, BPF_B, BPF_H, BPF_IND, BPF_JEQ, BPF_JMP, BPF_JSET, BPF_K,
        BPF_LD, BPF_LDX, BPF_MSH, BPF_RET,
    };
    [
        filter(
            BPF_LD | BPF_H | BPF_ABS,
            0,
            0,
            (libc::SKF_AD_OFF + libc::SKF_AD_PROTOCOL) as u32,
        ),
        filter(BPF_JMP | BPF_JEQ | BPF_K, 0, 12, libc::ETH_P_IP as u32),
        filter(BPF_LD | BPF_B | BPF_ABS, 0, 0, 0),
        filter(BPF_ALU | BPF_AND | BPF_K, 0, 0, 0xf0),
        filter(BPF_JMP | BPF_JEQ | BPF_K, 0, 9, 0x40), // IPv4
        filter(BPF_LD | BPF_B | BPF_ABS, 0, 0, 9),
        filter(BPF_JMP | BPF_JEQ | BPF_K, 0, 7, 17), // UDP
        filter(BPF_LD | BPF_H | BPF_ABS, 0, 0, 6),
        filter(BPF_JMP | BPF_JSET | BPF_K, 5, 0, 0x3fff), // reject IPv4 fragments (MF or offset)
        filter(BPF_LDX | BPF_B | BPF_MSH, 0, 0, 0),       // x = 4 * IHL
        filter(BPF_LD | BPF_H | BPF_IND, 0, 0, 2),        // destination port
        filter(BPF_JMP | BPF_JEQ | BPF_K, 3, 0, u32::from(PHOTON_PORT)),
        filter(BPF_LD | BPF_H | BPF_IND, 0, 0, 0), // source port
        filter(BPF_JMP | BPF_JEQ | BPF_K, 1, 0, u32::from(PHOTON_PORT)),
        filter(BPF_RET | BPF_K, 0, 0, 0),
        filter(BPF_RET | BPF_K, 0, 0, 65_535),
    ]
}

enum Receive<'a> {
    Frame(&'a [u8], bool),
    Skip,
    Empty,
}

fn wait_readable(fd: libc::c_int, timeout_ms: libc::c_int) -> Result<(), String> {
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if ready >= 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(format!("Linux packet poll failed: {error}"));
        }
    }
}

fn receive(fd: libc::c_int, buffer: &mut [u8]) -> Result<Receive<'_>, String> {
    let mut address: libc::sockaddr_ll = unsafe { std::mem::zeroed() };
    loop {
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
            match error.kind() {
                io::ErrorKind::Interrupted => continue,
                io::ErrorKind::WouldBlock => return Ok(Receive::Empty),
                _ => return Err(format!("Linux packet capture failed: {error}")),
            }
        }
        if address.sll_pkttype != libc::PACKET_HOST && address.sll_pkttype != libc::PACKET_OUTGOING
        {
            return Ok(Receive::Skip);
        }
        return Ok(Receive::Frame(
            &buffer[..count as usize],
            address.sll_pkttype == libc::PACKET_OUTGOING,
        ));
    }
}

struct Candidate<'a> {
    flow: procfs::Endpoint,
    incoming: bool,
    payload: &'a [u8],
}

fn parse<'a>(
    frame: &'a [u8],
    outgoing: bool,
    flow_cache: &mut procfs::FlowCache,
    pid: u32,
) -> Option<Candidate<'a>> {
    let datagram = packet::ipv4_udp(frame)?;
    let source_ip = datagram.source.ip().octets();
    let source_port = datagram.source.port();
    let target_ip = datagram.destination.ip().octets();
    let target_port = datagram.destination.port();
    let (local_ip, local_port, remote_ip, remote_port, incoming) = if outgoing {
        if target_port != PHOTON_PORT {
            return None;
        }
        (source_ip, source_port, target_ip, target_port, false)
    } else {
        if source_port != PHOTON_PORT {
            return None;
        }
        (target_ip, target_port, source_ip, source_port, true)
    };
    let payload = datagram.payload;
    if !packet::encrypted(payload) {
        return None;
    }
    let flow = flow_cache.verify_flow(pid, local_ip, local_port, remote_ip, remote_port)?;
    Some(Candidate {
        flow,
        incoming,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepts(frame: &[u8]) -> bool {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixDatagram;

        let (sender, receiver) = UnixDatagram::pair().unwrap();
        receiver.set_nonblocking(true).unwrap();
        let full = instructions();
        let full_program = libc::sock_fprog {
            len: full.len() as u16,
            filter: full.as_ptr().cast_mut(),
        };
        let attach_full = unsafe {
            libc::setsockopt(
                receiver.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_ATTACH_FILTER,
                (&full_program as *const libc::sock_fprog).cast(),
                std::mem::size_of_val(&full_program) as libc::socklen_t,
            )
        };
        assert_eq!(attach_full, 0, "{}", io::Error::last_os_error());

        // UnixDatagram has skb->protocol == 0, so run the IPv4+UDP+frag slice [2..].
        let mut filter = full[2..].to_vec();
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
        let mut fragment = ipv4_udp(5, 27002, 53000);
        fragment[6] = 0x00;
        fragment[7] = 0x01;
        assert!(!accepts(&fragment));
        // More-Fragments (MF) bit at offset 0 must also be rejected.
        let mut mf_first = ipv4_udp(5, 27002, 53000);
        mf_first[6] = 0x20;
        mf_first[7] = 0x00;
        assert!(!accepts(&mf_first));
        // Don't-Fragment (DF) bit is normal for UDP and must pass.
        let mut df = ipv4_udp(5, 27002, 53000);
        df[6] = 0x40;
        df[7] = 0x00;
        assert!(accepts(&df));
    }
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    fn packet(incoming: bool, local_port: u16) -> Vec<u8> {
        let mut payload = vec![0; 40];
        payload[2] = 1;
        payload[3..7].copy_from_slice(&[7, 8, 9, 10]);
        let mut frame = vec![0; 20];
        frame[0] = 0x45;
        frame[9] = 17;
        frame[12..16].copy_from_slice(&[127, 0, 0, 1]);
        frame[16..20].copy_from_slice(&[127, 0, 0, 1]);
        let (source, target): (u16, u16) = if incoming {
            (27002, local_port)
        } else {
            (local_port, 27002)
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
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let mut cache = procfs::FlowCache::default();
        let pid = std::process::id();
        let incoming_packet = packet(true, port);
        let incoming = parse(&incoming_packet, false, &mut cache, pid).unwrap();
        assert!(incoming.incoming);
        assert_eq!(incoming.flow.local_port, port);
        assert_eq!(&incoming.payload[3..7], &[7, 8, 9, 10]);
        assert!(parse(&packet(true, port), true, &mut cache, pid).is_none());
        assert!(parse(&packet(false, port.wrapping_add(1)), false, &mut cache, pid).is_none());
        assert!(
            !parse(&packet(false, port), true, &mut cache, pid)
                .unwrap()
                .incoming
        );
    }
}
