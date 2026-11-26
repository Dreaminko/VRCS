use std::net::{Ipv4Addr, SocketAddrV4, UdpSocket as StdUdpSocket};
use std::sync::OnceLock;

use rosc::{OscMessage, OscPacket, OscType, encoder};
use tokio::net::UdpSocket;

pub(super) const CHATBOX_ADDRESS: &str = "/chatbox/input";
pub(super) const TYPING_ADDRESS: &str = "/chatbox/typing";

static TYPING_SOCKET: OnceLock<StdUdpSocket> = OnceLock::new();

fn typing_socket() -> Result<&'static StdUdpSocket, String> {
    if let Some(socket) = TYPING_SOCKET.get() {
        return Ok(socket);
    }
    let socket =
        StdUdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).map_err(|e| e.to_string())?;
    let _ = socket.set_nonblocking(true);
    Ok(TYPING_SOCKET.get_or_init(|| socket))
}

pub(super) async fn send_chatbox(socket: &UdpSocket, port: u16, text: &str) -> Result<(), String> {
    let packet = OscPacket::Message(OscMessage {
        addr: CHATBOX_ADDRESS.into(),
        args: vec![
            OscType::String(text.into()),
            OscType::Bool(true),
            OscType::Bool(false),
        ],
    });
    let bytes = encoder::encode(&packet).map_err(|error| error.to_string())?;
    socket
        .send_to(&bytes, SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(crate) async fn send_typing(port: u16, typing: bool) -> Result<(), String> {
    let socket = typing_socket()?;
    let packet = OscPacket::Message(OscMessage {
        addr: TYPING_ADDRESS.into(),
        args: vec![OscType::Bool(typing)],
    });
    let bytes = encoder::encode(&packet).map_err(|error| error.to_string())?;
    socket
        .send_to(&bytes, SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .map(|_| ())
        .map_err(|error| error.to_string())
}
