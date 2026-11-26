use std::collections::HashSet;
use std::net::Ipv4Addr;

use super::helper::packet;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::*;

pub(super) struct Network {
    sockets: Vec<SOCKET>,
    events: Vec<WSAEVENT>,
    socket_ips: Vec<Ipv4Addr>,
    bound_ips: HashSet<Ipv4Addr>,
    pid: u32,
    ports: HashSet<u16>,
    buffer: Vec<u8>,
    last_packet_at: std::time::Instant,
}

unsafe fn open_interface_socket(address: Ipv4Addr) -> Option<(SOCKET, WSAEVENT)> {
    unsafe {
        let Ok(socket) = socket(AF_INET.0 as i32, SOCK_RAW, IPPROTO_IP.0) else {
            return None;
        };
        // Join/property snapshots arrive in bursts and are much larger
        // than voice frames. Preserve them while the scanner is active.
        let receive_buffer = (4 * 1024 * 1024i32).to_ne_bytes();
        let _ = setsockopt(socket, SOL_SOCKET, SO_RCVBUF, Some(&receive_buffer));
        let addr = SOCKADDR_IN {
            sin_family: AF_INET,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: u32::from_ne_bytes(address.octets()),
                },
            },
            ..Default::default()
        };
        let mode = 3u32; // RCVALL_IPLEVEL: receive local interface packets, no promiscuous mode.
        let mut returned = 0;
        let bound = bind(
            socket,
            (&addr as *const SOCKADDR_IN).cast(),
            std::mem::size_of_val(&addr) as i32,
        ) == 0;
        let enabled = bound
            && WSAIoctl(
                socket,
                SIO_RCVALL,
                Some((&mode as *const u32).cast()),
                4,
                None,
                0,
                &mut returned,
                None,
                None,
            ) == 0;
        let Ok(event) = WSACreateEvent() else {
            closesocket(socket);
            return None;
        };
        if enabled && WSAEventSelect(socket, Some(event), FD_READ as i32) == 0 {
            Some((socket, event))
        } else {
            let _ = WSACloseEvent(event);
            closesocket(socket);
            None
        }
    }
}

impl Network {
    pub(super) fn open(
        owner: crate::vrchat_process::ProcessIdentity,
        _status: super::SharedStatus,
        _stop: &std::sync::atomic::AtomicBool,
    ) -> Result<Self, String> {
        unsafe {
            let mut data = WSADATA::default();
            if WSAStartup(0x0202, &mut data) != 0 {
                return Err("Winsock startup failed".into());
            }
            let ports = match game_ports(owner.pid) {
                Ok(ports) => ports,
                Err(error) => {
                    WSACleanup();
                    return Err(error);
                }
            };
            let mut network = Self {
                sockets: Vec::new(),
                events: Vec::new(),
                socket_ips: Vec::new(),
                bound_ips: HashSet::new(),
                pid: owner.pid,
                ports,
                buffer: vec![0u8; 65_535],
                last_packet_at: std::time::Instant::now(),
            };
            let addresses = interfaces()?;
            for address in addresses {
                if let Some((socket, event)) = open_interface_socket(address) {
                    network.sockets.push(socket);
                    network.events.push(event);
                    network.socket_ips.push(address);
                    network.bound_ips.insert(address);
                }
            }
            if network.sockets.is_empty() {
                return Err("Player audio needs administrator access and an IPv4 interface supporting raw capture".into());
            }
            Ok(network)
        }
    }
    pub(super) fn refresh(&mut self) -> Result<(), String> {
        self.ports = game_ports(self.pid)?;
        if (self.sockets.is_empty()
            || self.last_packet_at.elapsed() >= std::time::Duration::from_secs(5))
            && let Ok(addresses) = interfaces()
        {
            let active: HashSet<Ipv4Addr> = addresses.iter().copied().collect();
            let mut index = 0;
            while index < self.socket_ips.len() {
                if active.contains(&self.socket_ips[index]) {
                    index += 1;
                    continue;
                }
                let ip = self.socket_ips.swap_remove(index);
                let socket = self.sockets.swap_remove(index);
                let event = self.events.swap_remove(index);
                self.bound_ips.remove(&ip);
                unsafe {
                    closesocket(socket);
                    let _ = WSACloseEvent(event);
                }
            }
            for address in addresses {
                if !self.bound_ips.contains(&address)
                    && let Some((socket, event)) = unsafe { open_interface_socket(address) }
                {
                    self.sockets.push(socket);
                    self.events.push(event);
                    self.socket_ips.push(address);
                    self.bound_ips.insert(address);
                }
            }
        }
        Ok(())
    }

    pub(super) fn wait(&self, timeout: std::time::Duration) {
        if timeout.is_zero() {
            return;
        }
        if self.events.is_empty() {
            std::thread::sleep(timeout);
            return;
        }
        let timeout_ms = timeout.as_millis().clamp(1, u32::MAX as u128 - 1) as u32;
        let count = self.events.len().min(64);
        let mut handles = [windows::Win32::Foundation::HANDLE::default(); 64];
        for (slot, event) in handles[..count].iter_mut().zip(&self.events[..count]) {
            *slot = windows::Win32::Foundation::HANDLE(event.0 as *mut std::ffi::c_void);
        }
        unsafe {
            let _ = WSAWaitForMultipleEvents(&handles[..count], false, timeout_ms, false);
        }
    }

    pub(super) fn receive(
        &mut self,
        mut callback: impl FnMut(super::connection::Datagram),
    ) -> Result<(), String> {
        for (socket, event) in self.sockets.iter().zip(&self.events) {
            let _ = unsafe { WSAResetEvent(*event) };
            let mut drained = false;
            // Bound work per interface so an active adapter cannot starve cancellation.
            for _ in 0..64 {
                let count = unsafe { recv(*socket, &mut self.buffer, SEND_RECV_FLAGS(0)) };
                if count == SOCKET_ERROR {
                    let error = unsafe { WSAGetLastError() };
                    if error == WSAEWOULDBLOCK {
                        drained = true;
                        break;
                    }
                    return Err(format!("Raw audio capture failed ({})", error.0));
                }
                if count <= 0 {
                    drained = true;
                    break;
                }
                self.last_packet_at = std::time::Instant::now();
                if let Some((connection, incoming, packet)) =
                    datagram(&self.buffer[..count as usize], &self.ports)
                {
                    callback(super::connection::Datagram {
                        connection,
                        incoming,
                        payload: packet.to_vec(),
                        at: std::time::Instant::now(),
                    });
                }
            }
            if !drained {
                let _ = unsafe { WSASetEvent(*event) };
            }
        }
        Ok(())
    }
}

impl Drop for Network {
    fn drop(&mut self) {
        unsafe {
            for socket in &self.sockets {
                closesocket(*socket);
            }
            for event in &self.events {
                let _ = WSACloseEvent(*event);
            }
            WSACleanup();
        }
    }
}

fn interfaces() -> Result<Vec<Ipv4Addr>, String> {
    unsafe {
        let mut length = 16_384u32;
        for _ in 0..3 {
            let mut data = vec![0u64; (length as usize).div_ceil(8)];
            let base = data.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
            let result = GetAdaptersAddresses(
                AF_INET.0 as u32,
                GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST | GAA_FLAG_SKIP_DNS_SERVER,
                None,
                Some(base),
                &mut length,
            );
            if result == ERROR_BUFFER_OVERFLOW.0 && length <= 4 * 1024 * 1024 {
                continue;
            }
            if result != 0 {
                return Err(format!("Cannot enumerate capture interfaces ({result})"));
            }
            let mut result = HashSet::new();
            let mut adapter = base;
            while !adapter.is_null() {
                let mut unicast = (*adapter).FirstUnicastAddress;
                while !unicast.is_null() {
                    let address = (*unicast).Address;
                    if !address.lpSockaddr.is_null()
                        && address.iSockaddrLength as usize >= std::mem::size_of::<SOCKADDR_IN>()
                        && (*address.lpSockaddr).sa_family == AF_INET
                    {
                        let address =
                            std::ptr::read_unaligned(address.lpSockaddr.cast::<SOCKADDR_IN>());
                        let ip = Ipv4Addr::from(address.sin_addr.S_un.S_addr.to_ne_bytes());
                        if !ip.is_loopback() && !ip.is_unspecified() {
                            result.insert(ip);
                        }
                    }
                    unicast = (*unicast).Next;
                }
                adapter = (*adapter).Next;
            }
            return Ok(result.into_iter().collect());
        }
    }
    Err("Capture interface list changed repeatedly".into())
}

pub(super) fn game_ports(pid: u32) -> Result<HashSet<u16>, String> {
    unsafe {
        let mut length = 0;
        let result = GetExtendedUdpTable(
            None,
            &mut length,
            false,
            AF_INET.0 as u32,
            UDP_TABLE_OWNER_PID,
            0,
        );
        if result != ERROR_INSUFFICIENT_BUFFER.0 || length > 4 * 1024 * 1024 {
            return Err("Cannot enumerate game UDP endpoints".into());
        }
        for _ in 0..3 {
            let mut buffer = vec![0u32; (length as usize).div_ceil(4)];
            let result = GetExtendedUdpTable(
                Some(buffer.as_mut_ptr().cast()),
                &mut length,
                false,
                AF_INET.0 as u32,
                UDP_TABLE_OWNER_PID,
                0,
            );
            if result == ERROR_INSUFFICIENT_BUFFER.0 && length <= 4 * 1024 * 1024 {
                continue;
            }
            if result != 0 {
                return Err(format!("Cannot read game UDP endpoints ({result})"));
            }
            let table = &*buffer.as_ptr().cast::<MIB_UDPTABLE_OWNER_PID>();
            let count = table.dwNumEntries as usize;
            if count
                > (buffer.len() * 4).saturating_sub(4) / std::mem::size_of::<MIB_UDPROW_OWNER_PID>()
            {
                return Err("Invalid UDP endpoint table".into());
            }
            let rows = std::slice::from_raw_parts(table.table.as_ptr(), count);
            return Ok(rows
                .iter()
                .filter(|r| r.dwOwningPid == pid)
                .map(|r| u16::from_be(r.dwLocalPort as u16))
                .collect());
        }
    }
    Err("Game UDP endpoints changed repeatedly".into())
}

pub(super) use super::connection::Connection;

pub(super) fn datagram<'a>(
    bytes: &'a [u8],
    ports: &HashSet<u16>,
) -> Option<(Connection, bool, &'a [u8])> {
    let datagram = packet::ipv4_udp(bytes)?;
    let (source, destination) = (datagram.source, datagram.destination);
    let (local, remote, incoming) =
        if source.port() == packet::PHOTON_PORT && ports.contains(&destination.port()) {
            (destination, source, true)
        } else if destination.port() == packet::PHOTON_PORT && ports.contains(&source.port()) {
            (source, destination, false)
        } else {
            return None;
        };
    let payload = datagram.payload;
    if !packet::encrypted(payload) {
        return None;
    }
    Some((
        Connection {
            local,
            remote,
            challenge: payload[3..7].try_into().ok()?,
        },
        incoming,
        payload,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capture_filter_accepts_only_current_process_voice_endpoints() {
        let payload = super::super::crypto::seal(&[1; 32], 1, b"synthetic voice");
        let mut ip = vec![0; 28];
        ip[0] = 0x45;
        ip[9] = 17;
        ip[12..16].copy_from_slice(&[1, 2, 3, 4]);
        ip[16..20].copy_from_slice(&[192, 168, 0, 2]);
        ip[20..22].copy_from_slice(&27002u16.to_be_bytes());
        ip[22..24].copy_from_slice(&53000u16.to_be_bytes());
        ip[24..26].copy_from_slice(&((payload.len() + 8) as u16).to_be_bytes());
        ip.extend(payload);
        let total = ip.len() as u16;
        ip[2..4].copy_from_slice(&total.to_be_bytes());
        let ports = HashSet::from([53000]);
        assert!(datagram(&ip, &ports).unwrap().1);
        assert!(datagram(&ip, &HashSet::from([53001])).is_none());
        ip[6] = 0x20;
        assert!(datagram(&ip, &ports).is_none());
    }
}
