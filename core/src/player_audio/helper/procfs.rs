use std::collections::{HashMap, HashSet};
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::time::{Duration, Instant};

const SOCK_DIAG_BY_FAMILY: u16 = 20;
const CACHE_TTL: Duration = Duration::from_secs(2);
const FLOW_CACHE_CAP: usize = 256;

pub(crate) fn starttime(pid: u32) -> io::Result<u64> {
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

#[cfg(test)]
fn alive(pid: u32, expected: u64) -> bool {
    starttime(pid).is_ok_and(|actual| actual == expected)
}

/// Only a missing process or a different start time confirms session end.
pub(crate) fn exited(pid: u32, expected: u64) -> bool {
    match starttime(pid) {
        Ok(actual) => actual != expected,
        Err(error) => error.kind() == io::ErrorKind::NotFound,
    }
}

pub(crate) fn check_identity(pid: u32, expected: u64) -> io::Result<()> {
    if starttime(pid)? != expected {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "The VRChat process restarted; start capture again",
        ));
    }
    Ok(())
}

pub(crate) fn uids(pid: u32) -> io::Result<[u32; 4]> {
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

pub(crate) fn validate_target(pid: u32, expected: u64, uid: u32) -> io::Result<()> {
    check_identity(pid, expected)?;
    if uids(pid)? != [uid; 4] {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Target UID does not match requesting user",
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
pub(crate) struct Endpoint {
    pub local_ip: [u8; 4],
    pub local_port: u16,
    pub remote_ip: [u8; 4],
    pub remote_port: u16,
    pub inode: u64,
    pub fd: Option<u32>,
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

pub(crate) fn flow_inode(
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

#[cfg(test)]
fn udp_endpoints(pid: u32) -> Vec<Endpoint> {
    read_udp_endpoints(pid).unwrap_or_default()
}

fn socket_inodes(pid: u32) -> io::Result<HashMap<u64, u32>> {
    let mut inodes = HashMap::new();
    for entry in fs::read_dir(format!("/proc/{pid}/fd"))? {
        let entry = entry?;
        let Some(fd_num) = entry
            .file_name()
            .to_str()
            .and_then(|name| name.parse::<u32>().ok())
        else {
            continue;
        };
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
            inodes.entry(inode).or_insert(fd_num);
        }
    }
    Ok(inodes)
}

#[repr(C)]
#[derive(Clone, Copy)]
struct InetDiagSockId {
    idiag_sport: [u8; 2],
    idiag_dport: [u8; 2],
    idiag_src: [[u8; 4]; 4],
    idiag_dst: [[u8; 4]; 4],
    idiag_if: u32,
    idiag_cookie: [u32; 2],
}

#[repr(C)]
struct InetDiagReqV2 {
    sdiag_family: u8,
    sdiag_protocol: u8,
    idiag_ext: u8,
    pad: u8,
    idiag_states: u32,
    id: InetDiagSockId,
}

#[repr(C)]
struct DiagRequest {
    hdr: libc::nlmsghdr,
    req: InetDiagReqV2,
}

#[repr(C)]
struct InetDiagMsg {
    idiag_family: u8,
    idiag_state: u8,
    idiag_timer: u8,
    idiag_retrans: u8,
    id: InetDiagSockId,
    idiag_expires: u32,
    idiag_rqueue: u32,
    idiag_wqueue: u32,
    idiag_uid: u32,
    idiag_inode: u32,
}

struct NetlinkFd(libc::c_int);
impl Drop for NetlinkFd {
    fn drop(&mut self) {
        unsafe { libc::close(self.0) };
    }
}

fn nlmsg_align(len: usize) -> usize {
    (len + 3) & !3
}

fn query_netlink_udp(family: u8, mut on_msg: impl FnMut(&InetDiagMsg)) -> io::Result<()> {
    let fd = unsafe {
        libc::socket(
            libc::AF_NETLINK,
            libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
            libc::NETLINK_SOCK_DIAG,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let sock = NetlinkFd(fd);
    let timeout = libc::timeval {
        tv_sec: 0,
        tv_usec: 500_000,
    };
    unsafe {
        libc::setsockopt(
            sock.0,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&timeout as *const libc::timeval).cast(),
            std::mem::size_of_val(&timeout) as libc::socklen_t,
        );
    }
    let mut local: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    local.nl_family = libc::AF_NETLINK as u16;
    if unsafe {
        libc::bind(
            sock.0,
            (&local as *const libc::sockaddr_nl).cast(),
            std::mem::size_of_val(&local) as libc::socklen_t,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }

    let req = DiagRequest {
        hdr: libc::nlmsghdr {
            nlmsg_len: std::mem::size_of::<DiagRequest>() as u32,
            nlmsg_type: SOCK_DIAG_BY_FAMILY,
            nlmsg_flags: (libc::NLM_F_REQUEST | libc::NLM_F_DUMP) as u16,
            nlmsg_seq: 1,
            nlmsg_pid: 0,
        },
        req: InetDiagReqV2 {
            sdiag_family: family,
            sdiag_protocol: libc::IPPROTO_UDP as u8,
            idiag_ext: 0,
            pad: 0,
            idiag_states: u32::MAX,
            id: InetDiagSockId {
                idiag_sport: [0; 2],
                idiag_dport: [0; 2],
                idiag_src: [[0; 4]; 4],
                idiag_dst: [[0; 4]; 4],
                idiag_if: 0,
                idiag_cookie: [u32::MAX; 2],
            },
        },
    };
    let mut kernel: libc::sockaddr_nl = unsafe { std::mem::zeroed() };
    kernel.nl_family = libc::AF_NETLINK as u16;
    let sent = unsafe {
        libc::sendto(
            sock.0,
            (&req as *const DiagRequest).cast(),
            std::mem::size_of::<DiagRequest>(),
            0,
            (&kernel as *const libc::sockaddr_nl).cast(),
            std::mem::size_of_val(&kernel) as libc::socklen_t,
        )
    };
    if sent < 0 {
        return Err(io::Error::last_os_error());
    }

    let mut buffer = [0u8; 8192];
    let hdr_len = std::mem::size_of::<libc::nlmsghdr>();
    let msg_len = std::mem::size_of::<InetDiagMsg>();
    loop {
        let count = unsafe { libc::recv(sock.0, buffer.as_mut_ptr().cast(), buffer.len(), 0) };
        if count < 0 {
            let err = io::Error::last_os_error();
            if err.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(err);
        }
        if count == 0 {
            break;
        }
        let mut offset = 0usize;
        let received = count as usize;
        while offset + hdr_len <= received {
            let hdr = unsafe {
                std::ptr::read_unaligned(buffer[offset..].as_ptr().cast::<libc::nlmsghdr>())
            };
            let len = hdr.nlmsg_len as usize;
            if len < hdr_len || offset + len > received {
                break;
            }
            match hdr.nlmsg_type as i32 {
                libc::NLMSG_DONE => return Ok(()),
                libc::NLMSG_ERROR => {
                    if len >= hdr_len + 4 {
                        let code = unsafe {
                            std::ptr::read_unaligned(
                                buffer[offset + hdr_len..].as_ptr().cast::<i32>(),
                            )
                        };
                        if code != 0 {
                            return Err(io::Error::from_raw_os_error(-code));
                        }
                    }
                }
                t if t == i32::from(SOCK_DIAG_BY_FAMILY) && len >= hdr_len + msg_len => {
                    let diag = unsafe {
                        std::ptr::read_unaligned(
                            buffer[offset + hdr_len..].as_ptr().cast::<InetDiagMsg>(),
                        )
                    };
                    on_msg(&diag);
                }
                _ => {}
            }
            let step = nlmsg_align(len);
            if step == 0 {
                break;
            }
            offset += step;
        }
    }
    Ok(())
}

fn read_udp_snapshot(
    pid: u32,
    inodes: &HashMap<u64, u32>,
) -> io::Result<(Vec<Endpoint>, HashSet<u16>)> {
    let mut endpoints = Vec::new();
    let mut ipv6_ports = HashSet::new();
    let v4_ok = query_netlink_udp(libc::AF_INET as u8, |msg| {
        let inode = u64::from(msg.idiag_inode);
        let fd = inodes.get(&inode).copied();
        endpoints.push(Endpoint {
            local_ip: msg.id.idiag_src[0],
            local_port: u16::from_be_bytes(msg.id.idiag_sport),
            remote_ip: msg.id.idiag_dst[0],
            remote_port: u16::from_be_bytes(msg.id.idiag_dport),
            inode,
            fd,
            owned: fd.is_some(),
        });
    });
    let v6_ok = query_netlink_udp(libc::AF_INET6 as u8, |msg| {
        ipv6_ports.insert(u16::from_be_bytes(msg.id.idiag_sport));
    });
    if v4_ok.is_ok() && (v6_ok.is_ok() || !Path::new("/proc/net/udp6").exists()) {
        return Ok((endpoints, ipv6_ports));
    }

    endpoints.clear();
    ipv6_ports.clear();
    let table = fs::read_to_string(format!("/proc/{pid}/net/udp"))?;
    for line in table.lines().skip(1) {
        endpoints.push(
            parse_udp_row(line, inodes).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "Invalid UDP socket row")
            })?,
        );
    }
    match fs::read_to_string(format!("/proc/{pid}/net/udp6")) {
        Ok(table6) => {
            for line in table6.lines().skip(1) {
                if let Some(port) = line
                    .split_whitespace()
                    .nth(1)
                    .and_then(|address| address.split_once(':'))
                    .and_then(|(_, port)| u16::from_str_radix(port, 16).ok())
                {
                    ipv6_ports.insert(port);
                }
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if Path::new("/proc/net/udp6").exists() {
                return Err(error);
            }
        }
        Err(error) => return Err(error),
    }
    Ok((endpoints, ipv6_ports))
}

#[cfg(test)]
fn read_udp_endpoints(pid: u32) -> io::Result<Vec<Endpoint>> {
    let inodes = socket_inodes(pid)?;
    let (endpoints, _) = read_udp_snapshot(pid, &inodes)?;
    Ok(endpoints)
}

#[derive(Clone, Copy, Hash, PartialEq, Eq)]
struct FlowKey {
    local_ip: [u8; 4],
    local_port: u16,
    remote_ip: [u8; 4],
    remote_port: u16,
}

struct CachedFlow {
    flow: Endpoint,
    verified_at: Instant,
}

#[derive(Default)]
pub(crate) struct FlowCache {
    snapshot_at: Option<Instant>,
    endpoints: Vec<Endpoint>,
    ipv6_ports: HashSet<u16>,
    flows: HashMap<FlowKey, CachedFlow>,
}

impl FlowCache {
    pub fn refresh_if_due(&mut self, pid: u32) -> io::Result<bool> {
        if self.snapshot_at.is_some_and(|at| at.elapsed() < CACHE_TTL) {
            return Ok(false);
        }
        self.refresh(pid)
    }

    fn refresh(&mut self, pid: u32) -> io::Result<bool> {
        let inodes = socket_inodes(pid)?;
        let (updated, ipv6_ports) = read_udp_snapshot(pid, &inodes)?;
        let changed = owned_keys(&self.endpoints) != owned_keys(&updated);
        self.endpoints = updated;
        self.ipv6_ports = ipv6_ports;
        self.snapshot_at = Some(Instant::now());
        if changed {
            self.flows.clear();
        }
        Ok(changed)
    }

    pub fn verify_flow(
        &mut self,
        pid: u32,
        local_ip: [u8; 4],
        local_port: u16,
        remote_ip: [u8; 4],
        remote_port: u16,
    ) -> Option<Endpoint> {
        let key = FlowKey {
            local_ip,
            local_port,
            remote_ip,
            remote_port,
        };
        if let Some(cached) = self.flows.get(&key) {
            if cached.verified_at.elapsed() < CACHE_TTL {
                return Some(cached.flow.clone());
            }
        }
        if self.snapshot_at.is_none_or(|at| at.elapsed() >= CACHE_TTL) && self.refresh(pid).is_err()
        {
            return None;
        }
        if self.ipv6_ports.contains(&local_port) {
            return None;
        }
        let inode = flow_inode(
            &self.endpoints,
            local_ip,
            local_port,
            remote_ip,
            remote_port,
        )?;
        let fd = self
            .endpoints
            .iter()
            .find(|endpoint| endpoint.owned && endpoint.inode == inode)
            .and_then(|endpoint| endpoint.fd);
        let flow = Endpoint {
            local_ip,
            local_port,
            remote_ip,
            remote_port,
            inode,
            fd,
            owned: true,
        };
        if self.flows.len() >= FLOW_CACHE_CAP {
            self.flows
                .retain(|_, entry| entry.verified_at.elapsed() < CACHE_TTL);
            if self.flows.len() >= FLOW_CACHE_CAP {
                if let Some(oldest) = self
                    .flows
                    .iter()
                    .min_by_key(|(_, entry)| entry.verified_at)
                    .map(|(k, _)| *k)
                {
                    self.flows.remove(&oldest);
                }
            }
        }
        self.flows.insert(
            key,
            CachedFlow {
                flow: flow.clone(),
                verified_at: Instant::now(),
            },
        );
        Some(flow)
    }
}

type OwnedEndpointKey = ([u8; 4], u16, [u8; 4], u16, u64);

fn owned_keys(endpoints: &[Endpoint]) -> Vec<OwnedEndpointKey> {
    let mut keys: Vec<_> = endpoints
        .iter()
        .filter(|e| e.owned)
        .map(|e| {
            (
                e.local_ip,
                e.local_port,
                e.remote_ip,
                e.remote_port,
                e.inode,
            )
        })
        .collect();
    keys.sort_unstable();
    keys
}

fn parse_udp_row(line: &str, inodes: &HashMap<u64, u32>) -> Option<Endpoint> {
    let mut fields = line.split_whitespace();
    let _slot = fields.next()?;
    let (local_ip, local_port) = parse_endpoint(fields.next()?)?;
    let (remote_ip, remote_port) = parse_endpoint(fields.next()?)?;
    let _state = fields.next()?;
    for _ in 0..5 {
        fields.next()?;
    }
    let inode = fields.next()?.parse().ok()?;
    let fd = inodes.get(&inode).copied();
    Some(Endpoint {
        local_ip,
        local_port,
        remote_ip,
        remote_port,
        inode,
        fd,
        owned: fd.is_some(),
    })
}

/// Recheck only candidate flows. The cached inode must still be the sole
/// matching socket and still be held by the same process. These /proc reads
/// narrow races; they cannot prove ownership at the instant a packet arrived.
#[cfg(test)]
fn confirms_flow(pid: u32, starttime: u64, flow: &Endpoint) -> bool {
    if !alive(pid, starttime) {
        return false;
    }
    let Ok(table) = fs::read_to_string(format!("/proc/{pid}/net/udp")) else {
        return false;
    };
    let mut found = false;
    let inodes = HashMap::new();
    for line in table.lines().skip(1) {
        let Some(endpoint) = parse_udp_row(line, &inodes) else {
            return false;
        };
        if endpoint.matches(
            flow.local_ip,
            flow.local_port,
            flow.remote_ip,
            flow.remote_port,
        ) {
            if endpoint.inode != flow.inode || flow.inode == 0 {
                return false;
            }
            found = true;
        }
    }
    // Inspect descriptors after the table: an inode closed since the cached
    // snapshot must not authorize traffic from a replacement socket.
    found
        && ipv6_port_is_clear(pid, starttime, flow.local_port)
        && holds_socket(pid, flow.inode, flow.fd).unwrap_or(false)
        && alive(pid, starttime)
}

#[cfg(test)]
fn holds_socket(pid: u32, inode: u64, fd_hint: Option<u32>) -> io::Result<bool> {
    let expected = format!("socket:[{inode}]");
    if let Some(fd) = fd_hint {
        if let Ok(path) = fs::read_link(format!("/proc/{pid}/fd/{fd}")) {
            if path.as_os_str() == std::ffi::OsStr::new(&expected) {
                return Ok(true);
            }
        }
    }
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

#[cfg(test)]
fn ipv6_port_is_clear(pid: u32, starttime: u64, local_port: u16) -> bool {
    let table = match fs::read_to_string(format!("/proc/{pid}/net/udp6")) {
        Ok(table) => table,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return !Path::new("/proc/net/udp6").exists() && alive(pid, starttime);
        }
        Err(_) => return false,
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

pub(crate) fn monotonic_us() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) } != 0 {
        return 0;
    }
    (ts.tv_sec as u64) * 1_000_000 + (ts.tv_nsec as u64) / 1_000
}

pub(crate) fn peer_credentials(
    stream: &std::os::unix::net::UnixStream,
) -> io::Result<(u32, u32, u32)> {
    use std::os::fd::AsRawFd;

    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let res = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if res != 0 {
        return Err(io::Error::last_os_error());
    }
    if cred.pid <= 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid peer process identity",
        ));
    }
    Ok((cred.pid as u32, cred.uid, cred.gid))
}

pub(crate) fn parent_is_private(path: &Path, expected_uid: Option<u32>) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;

    let Some(parent) = path.parent() else {
        return Err("missing socket parent".into());
    };
    let dir = CString::new(parent.as_os_str().as_bytes()).map_err(|_| "invalid socket path")?;
    let fd = unsafe {
        libc::open(
            dir.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(format!(
            "cannot inspect helper socket directory: {}",
            io::Error::last_os_error()
        ));
    }
    let file = unsafe { fs::File::from_raw_fd(fd) };
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    let mode = metadata.mode() & 0o777;
    if !metadata.file_type().is_dir() || mode != 0o700 {
        return Err("helper socket directory has unsafe permissions".into());
    }
    let expected = expected_uid.unwrap_or_else(|| unsafe { libc::getuid() });
    if metadata.uid() != expected {
        return Err("helper socket directory owner mismatch".into());
    }
    if path.exists() {
        let meta = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
        if !meta.file_type().is_socket() || meta.uid() != expected {
            return Err("helper socket path is not owned by the caller".into());
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
    fn flow_cache_attributes_owned_socket_and_bounds_entries() {
        let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let mut cache = super::FlowCache::default();
        let flow = cache
            .verify_flow(
                std::process::id(),
                [127, 0, 0, 1],
                port,
                [192, 0, 2, 1],
                27002,
            )
            .unwrap();
        assert!(flow.owned);
        assert_eq!(flow.local_port, port);
        assert!(cache
            .verify_flow(
                std::process::id(),
                [127, 0, 0, 1],
                port,
                [192, 0, 2, 1],
                27002
            )
            .is_some());
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
        let flow = super::Endpoint {
            local_ip: [127, 0, 0, 1],
            local_port: port,
            remote_ip: [192, 0, 2, 1],
            remote_port: 27002,
            inode,
            fd: None,
            owned: true,
        };
        let confirmed = || super::confirms_flow(pid, starttime, &flow);
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
            fd: None,
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
