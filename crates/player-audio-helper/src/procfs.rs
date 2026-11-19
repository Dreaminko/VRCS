use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

pub fn starttime(pid: u32) -> io::Result<u64> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let end = stat
        .rfind(')')
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid process status"))?;
    // Fields after comm start at state (field 3); starttime is field 22.
    let mut fields = stat[end + 1..].split_whitespace();
    if matches!(fields.next(), Some("Z" | "X" | "x")) {
        return Err(io::Error::new(io::ErrorKind::NotFound, "VRChat exited"));
    }
    fields
        .nth(18)
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid process start time"))
}

pub fn alive(pid: u32, expected: u64) -> bool {
    starttime(pid).is_ok_and(|actual| actual == expected)
}

/// Only a missing process or a different start time confirms session end.
pub fn exited(pid: u32, expected: u64) -> bool {
    match starttime(pid) {
        Ok(actual) => actual != expected,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

pub fn check_identity(pid: u32, expected: u64) -> io::Result<()> {
    if starttime(pid)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "The VRChat process restarted; start capture again",
        ));
    }
    Ok(())
}

pub fn uids(pid: u32) -> io::Result<[u32; 4]> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let values = status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Missing target UID"))?;
    let mut values = values.split_whitespace();
    let mut uids = [0; 4];
    for uid in &mut uids {
        *uid = values
            .next()
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid target UID"))?;
    }
    Ok(uids)
}

pub fn validate_target(pid: u32, expected: u64, uid: u32) -> io::Result<()> {
    check_identity(pid, expected)?;
    if uids(pid)? != [uid; 4] {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Target UID does not match pkexec caller",
        ));
    }
    let maps = fs::read_to_string(format!("/proc/{pid}/maps"))?;
    if !maps.to_ascii_lowercase().contains("gameassembly.dll") {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The selected process is not a VRChat IL2CPP process",
        ));
    }
    let cmdline = fs::read(format!("/proc/{pid}/cmdline"))?;
    let comm = fs::read(format!("/proc/{pid}/comm"))?;
    if !cmdline
        .split(|byte| *byte == 0)
        .chain(comm.split(|byte| *byte == b'\n'))
        .any(|part| {
            String::from_utf8_lossy(part)
                .to_ascii_lowercase()
                .contains("vrchat.exe")
        })
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "The selected process is not VRChat",
        ));
    }
    if fs::read_link(format!("/proc/{pid}/ns/net"))? != fs::read_link("/proc/self/ns/net")? {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "VRChat is in a different network namespace from VRCS",
        ));
    }
    check_identity(pid, expected)
}

/// Keep every IPv4 UDP socket so a competing inode makes attribution fail closed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Endpoint {
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub inode: u64,
    pub owned: bool,
}

impl Endpoint {
    fn matches(
        &self,
        local_ip: [u8; 4],
        local_port: u16,
        remote_ip: [u8; 4],
        remote_port: u16,
    ) -> bool {
        self.local_port == local_port
            && (self.local_ip == [0; 4] || self.local_ip == local_ip)
            && (self.remote_ip == [0; 4] || self.remote_ip == remote_ip)
            && (self.remote_port == 0 || self.remote_port == remote_port)
    }
}

pub fn flow_inode(
    endpoints: &[Endpoint],
    local_ip: [u8; 4],
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
) -> Option<u64> {
    let mut matches = endpoints
        .iter()
        .filter(|endpoint| endpoint.matches(local_ip, local_port, remote_ip, remote_port));
    let first = matches.next()?;
    (first.owned
        && first.inode != 0
        && matches.all(|endpoint| endpoint.owned && endpoint.inode == first.inode))
    .then_some(first.inode)
}

pub fn udp_endpoints(pid: u32) -> Vec<Endpoint> {
    read_udp_endpoints(pid).unwrap_or_default()
}

fn socket_inodes(pid: u32) -> io::Result<HashSet<u64>> {
    let mut inodes = HashSet::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd"))? {
        let entry = entry?;
        let path = match fs::read_link(entry.path()) {
            Ok(path) => path,
            // File descriptors may close while the directory is traversed.
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if let Some(inode) = path
            .to_str()
            .and_then(|path| path.strip_prefix("socket:["))
            .and_then(|path| path.strip_suffix(']'))
            .and_then(|inode| inode.parse().ok())
        {
            inodes.insert(inode);
        }
    }
    Ok(inodes)
}

fn read_udp_endpoints(pid: u32) -> io::Result<Vec<Endpoint>> {
    let table = fs::read_to_string(format!("/proc/{pid}/net/udp"))?;
    let inodes = socket_inodes(pid)?;
    table
        .lines()
        .skip(1)
        .map(|line| {
            parse_udp_row(line, &inodes)
                .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Invalid UDP socket row"))
        })
        .collect()
}

fn parse_udp_row(line: &str, inodes: &HashSet<u64>) -> Option<Endpoint> {
    let mut fields = line.split_whitespace();
    let _slot = fields.next()?;
    let (local_ip, local_port) = parse_endpoint(fields.next()?)?;
    let (remote_ip, remote_port) = parse_endpoint(fields.next()?)?;
    let _state = fields.next()?;
    for _ in 0..5 {
        fields.next()?;
    }
    let inode = fields.next()?.parse().ok()?;
    Some(Endpoint {
        local_ip,
        local_port,
        remote_ip,
        remote_port,
        inode,
        owned: inodes.contains(&inode),
    })
}

/// Recheck only candidate flows. The cached inode must still be the sole
/// matching socket and still be held by the same process. These /proc reads
/// narrow races; they cannot prove ownership at the instant a packet arrived.
pub fn confirms_flow(
    pid: u32,
    starttime: u64,
    inode: u64,
    local_ip: [u8; 4],
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
) -> bool {
    if !alive(pid, starttime) {
        return false;
    }
    let Ok(table) = fs::read_to_string(format!("/proc/{pid}/net/udp")) else {
        return false;
    };
    let mut found = false;
    let inodes = HashSet::new();
    for line in table.lines().skip(1) {
        let Some(endpoint) = parse_udp_row(line, &inodes) else {
            return false;
        };
        if endpoint.matches(local_ip, local_port, remote_ip, remote_port) {
            if endpoint.inode != inode || inode == 0 {
                return false;
            }
            found = true;
        }
    }
    // Inspect descriptors after the table: an inode closed since the cached
    // snapshot must not authorize traffic from a replacement socket.
    found
        && ipv6_port_is_clear(pid, local_port)
        && holds_socket(pid, inode).unwrap_or(false)
        && alive(pid, starttime)
}

fn holds_socket(pid: u32, inode: u64) -> io::Result<bool> {
    let expected = format!("socket:[{inode}]");
    for entry in fs::read_dir(format!("/proc/{pid}/fd"))? {
        match fs::read_link(entry?.path()) {
            Ok(path) if path.as_os_str() == std::ffi::OsStr::new(&expected) => return Ok(true),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn ipv6_port_is_clear(pid: u32, local_port: u16) -> bool {
    let Ok(table) = fs::read_to_string(format!("/proc/{pid}/net/udp6")) else {
        return false;
    };
    // A dual-stack UDP socket may receive IPv4 traffic. /proc does not expose
    // IPV6_V6ONLY here, so conservatively reject even IPv6-only sockets with
    // the same local port rather than guessing whether they can compete.
    table.lines().skip(1).all(|line| {
        line.split_whitespace()
            .nth(1)
            .and_then(|address| address.split_once(':'))
            .and_then(|(_, port)| u16::from_str_radix(port, 16).ok())
            .is_some_and(|port| port != local_port)
    })
}

fn parse_endpoint(value: &str) -> Option<([u8; 4], u16)> {
    let (ip, port) = value.split_once(':')?;
    Some((
        u32::from_str_radix(ip, 16).ok()?.to_ne_bytes(),
        u16::from_str_radix(port, 16).ok()?,
    ))
}

pub fn monotonic_us() -> u64 {
    let mut value = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut value) };
    (value.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(value.tv_nsec as u64 / 1_000)
}

pub fn peer_credentials(stream: &std::os::unix::net::UnixStream) -> io::Result<(u32, u32, u32)> {
    use std::os::fd::AsRawFd;
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut length = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut credentials as *mut libc::ucred).cast(),
            &mut length,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((credentials.pid as u32, credentials.uid, credentials.gid))
}

pub fn parent_is_private(path: &Path, expected_uid: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let parent = path
        .parent()
        .ok_or_else(|| "Socket has no parent directory".to_string())?;
    let metadata = fs::metadata(parent)
        .map_err(|error| format!("Cannot inspect socket directory: {error}"))?;
    if !metadata.is_dir() {
        return Err("Socket parent is not a directory".into());
    }
    if metadata.mode() & 0o077 != 0 {
        return Err("Socket directory is accessible by other users".into());
    }
    if let Some(uid) = expected_uid {
        if metadata.uid() != uid {
            return Err("Socket directory owner does not match the requesting user".into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_udp_socket_owned_by_the_target_process() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        assert!(super::udp_endpoints(std::process::id())
            .iter()
            .any(|endpoint| endpoint.owned
                && endpoint.local_ip == [127, 0, 0, 1]
                && endpoint.local_port == port));
    }

    #[test]
    fn cached_socket_does_not_authorize_a_closed_or_reused_port() {
        let pid = std::process::id();
        let starttime = super::starttime(pid).unwrap();
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = socket.local_addr().unwrap();
        let port = address.port();
        let cached = super::udp_endpoints(pid);
        let inode =
            super::flow_inode(&cached, [127, 0, 0, 1], port, [192, 0, 2, 1], 27002).unwrap();
        let confirmed = || {
            super::confirms_flow(
                pid,
                starttime,
                inode,
                [127, 0, 0, 1],
                port,
                [192, 0, 2, 1],
                27002,
            )
        };
        assert!(confirmed());
        // IPv6 loopback does not overlap this IPv4 binding, but procfs cannot
        // tell us whether an arbitrary IPv6 socket is IPv6-only.
        if let Ok(competitor) = std::net::UdpSocket::bind(("::1", port)) {
            assert!(!confirmed());
            drop(competitor);
            assert!(confirmed());
        }
        drop(socket);
        assert!(!confirmed());
        let _replacement = std::net::UdpSocket::bind(address).unwrap();
        assert!(super::flow_inode(&cached, [127, 0, 0, 1], port, [192, 0, 2, 1], 27002).is_some());
        assert!(!confirmed());
    }

    #[test]
    fn attribution_requires_addresses_remote_and_an_unambiguous_inode() {
        use super::{flow_inode, Endpoint};
        let game = Endpoint {
            local_ip: [127, 0, 0, 1],
            local_port: 53000,
            remote_ip: [192, 0, 2, 1],
            remote_port: 27002,
            inode: 1,
            owned: true,
        };
        let accepts = |endpoints: &[Endpoint], ip, remote| {
            flow_inode(endpoints, ip, 53000, remote, 27002).is_some()
        };
        let endpoints = std::slice::from_ref(&game);
        assert!(accepts(endpoints, [127, 0, 0, 1], [192, 0, 2, 1]));
        assert!(!accepts(endpoints, [127, 0, 0, 2], [192, 0, 2, 1]));
        assert!(!accepts(endpoints, [127, 0, 0, 1], [192, 0, 2, 2]));
        let competitor = Endpoint {
            local_ip: [0; 4],
            remote_ip: [0; 4],
            remote_port: 0,
            inode: 2,
            owned: false,
            ..game.clone()
        };
        assert!(!accepts(
            &[game, competitor],
            [127, 0, 0, 1],
            [192, 0, 2, 1]
        ));
    }

    #[test]
    fn process_identity_rejects_a_different_starttime() {
        let pid = std::process::id();
        let starttime = super::starttime(pid).unwrap();
        super::check_identity(pid, starttime).unwrap();
        assert!(!super::exited(pid, starttime));
        assert!(super::exited(pid, starttime.wrapping_add(1)));
        assert_eq!(
            super::check_identity(pid, starttime.wrapping_add(1))
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
    }
}
