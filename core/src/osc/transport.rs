use std::net::{Ipv4Addr, SocketAddrV4};

use rosc::{encoder, OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;

pub(super) const CHATBOX_ADDRESS: &str = "/chatbox/input";

pub(super) async fn send_chatbox(socket: &UdpSocket, port: u16, text: &str) -> Result<(), String> {
    let bytes = encode_chatbox(text)?;
    socket
        .send_to(&bytes, SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))
        .await
        .map(|_| ())
        .map_err(|error| error.to_string())
}

pub(super) fn try_send_chatbox(socket: &UdpSocket, port: u16, text: &str) -> std::io::Result<()> {
    let bytes = encode_chatbox(text).map_err(std::io::Error::other)?;
    socket
        .try_send_to(&bytes, SocketAddrV4::new(Ipv4Addr::LOCALHOST, port).into())
        .map(|_| ())
}

fn encode_chatbox(text: &str) -> Result<Vec<u8>, String> {
    let packet = OscPacket::Message(OscMessage {
        addr: CHATBOX_ADDRESS.into(),
        args: vec![
            OscType::String(text.into()),
            OscType::Bool(true),
            OscType::Bool(false),
        ],
    });
    encoder::encode(&packet).map_err(|error| error.to_string())
}
