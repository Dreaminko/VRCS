use std::collections::HashSet;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use windows::Win32::Foundation::{CloseHandle, ERROR_BUFFER_OVERFLOW, ERROR_INSUFFICIENT_BUFFER};
use windows::Win32::NetworkManagement::IpHelper::*;
use windows::Win32::Networking::WinSock::*;
use windows::Win32::System::Diagnostics::ToolHelp::*;
use windows::Win32::System::Threading::{
    GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};

pub(super) fn process_identity(pid: u32) -> Result<super::cache::ProcessIdentity, String> {
    use windows::Win32::Foundation::FILETIME;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }
        .map_err(|e| e.to_string())?;
    let (mut created, mut exited, mut kernel, mut user) = (
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
        FILETIME::default(),
    );
    let result =
        unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    let _ = unsafe { CloseHandle(handle) };
    result.map_err(|e| e.to_string())?;
    if exited.dwLowDateTime != 0 || exited.dwHighDateTime != 0 {
        return Err("VRChat process exited".into());
    }
    Ok(super::cache::ProcessIdentity {
        pid,
        created: (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime),
    })
}

pub(super) fn game() -> Result<Option<(u32, PathBuf)>, String> {
    unsafe {
        let snapshot =
            CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| e.to_string())?;
        let mut entry = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        let mut found = None;
        let mut ok = Process32FirstW(snapshot, &mut entry).is_ok();
        while ok {
            let length = entry
                .szExeFile
                .iter()
                .position(|c| *c == 0)
                .unwrap_or(entry.szExeFile.len());
            if String::from_utf16_lossy(&entry.szExeFile[..length])
                .eq_ignore_ascii_case("VRChat.exe")
                && process_identity(entry.th32ProcessID).is_ok()
            {
                found = Some(entry.th32ProcessID);
                break;
            }
            ok = Process32NextW(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
        let Some(pid) = found else { return Ok(None) };
        // A process may exit between the snapshot and opening its image path.
        // Treat that as still waiting, rather than failing the armed capture.
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return Ok(None);
        };
        let mut path = vec![0u16; 32768];
        let mut length = path.len() as u32;
        let result = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(path.as_mut_ptr()),
            &mut length,
        );
        let _ = CloseHandle(process);
        if result.is_err() && process_identity(pid).is_err() {
            return Ok(None);
        }
        result.map_err(|e| e.to_string())?;
        Ok(Some((
            pid,
            PathBuf::from(String::from_utf16_lossy(&path[..length as usize])),
        )))
    }
}

pub(super) struct Network {
    sockets: Vec<SOCKET>,
}
impl Network {
    pub(super) fn open() -> Result<Self, String> {
        unsafe {
            let mut data = WSADATA::default();
            if WSAStartup(0x0202, &mut data) != 0 {
                return Err("Winsock startup failed".into());
            }
            let mut network = Self {
                sockets: Vec::new(),
            };
            let addresses = interfaces()?;
            for address in addresses {
                let Ok(socket) = socket(AF_INET.0 as i32, SOCK_RAW, IPPROTO_IP.0) else {
                    continue;
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
                let mut mode = 3u32; // RCVALL_IPLEVEL: receive local interface packets, no promiscuous mode.
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
                mode = 1;
                if enabled && ioctlsocket(socket, FIONBIO, &mut mode) == 0 {
                    network.sockets.push(socket);
                } else {
                    closesocket(socket);
                }
            }
            if network.sockets.is_empty() {
                return Err("Player audio needs administrator access and an IPv4 interface supporting raw capture".into());
            }
            Ok(network)
        }
    }
    pub(super) fn receive(
        &self,
        buffer: &mut [u8],
        mut callback: impl FnMut(&[u8]),
    ) -> Result<(), String> {
        for socket in &self.sockets {
            // Bound work per interface so an active adapter cannot starve cancellation.
            for _ in 0..64 {
                let count = unsafe { recv(*socket, buffer, SEND_RECV_FLAGS(0)) };
                if count == SOCKET_ERROR {
                    let error = unsafe { WSAGetLastError() };
                    if error == WSAEWOULDBLOCK {
                        break;
                    }
                    return Err(format!("Raw audio capture failed ({})", error.0));
                }
                if count <= 0 {
                    break;
                }
                callback(&buffer[..count as usize]);
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct Connection {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub challenge: [u8; 4],
}

pub(super) fn datagram<'a>(
    bytes: &'a [u8],
    ports: &HashSet<u16>,
) -> Option<(Connection, bool, &'a [u8])> {
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
    let src = SocketAddrV4::new(
        Ipv4Addr::new(bytes[12], bytes[13], bytes[14], bytes[15]),
        u16::from_be_bytes([udp[0], udp[1]]),
    );
    let dst = SocketAddrV4::new(
        Ipv4Addr::new(bytes[16], bytes[17], bytes[18], bytes[19]),
        u16::from_be_bytes([udp[2], udp[3]]),
    );
    let (local, remote, incoming) = if src.port() == 27002 && ports.contains(&dst.port()) {
        (dst, src, true)
    } else if dst.port() == 27002 && ports.contains(&src.port()) {
        (src, dst, false)
    } else {
        return None;
    };
    let packet = &udp[8..size];
    if !super::crypto::encrypted(packet) {
        return None;
    }
    Some((
        Connection {
            local,
            remote,
            challenge: packet[3..7].try_into().ok()?,
        },
        incoming,
        packet,
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
