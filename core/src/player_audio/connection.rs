use std::net::SocketAddrV4;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct Connection {
    pub local: SocketAddrV4,
    pub remote: SocketAddrV4,
    pub challenge: [u8; 4],
}

#[derive(Debug)]
pub(super) struct Datagram {
    pub connection: Connection,
    pub incoming: bool,
    pub payload: Vec<u8>,
    pub at: std::time::Instant,
}
