//! A loopback stand-in for VRChat's OSC endpoint, used by integration tests
//! that exercise the real `voxshift-vrchat` sender/receiver.

use std::net::{Ipv4Addr, SocketAddr};

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;

pub struct MockVrChatPeer {
    /// Bound to the port VoxShift sends `/input/Voice` to (i.e. VoxShift's
    /// configured `send_port`) — this is VRChat's side of that exchange.
    inbound: UdpSocket,
    /// Used to send fake `MuteSelf`/`avatar/change` messages to VoxShift's
    /// configured `receive_port`.
    outbound: UdpSocket,
    voxshift_receive_addr: SocketAddr,
}

impl MockVrChatPeer {
    pub async fn bind(voxshift_send_port: u16, voxshift_receive_port: u16) -> std::io::Result<Self> {
        let inbound = UdpSocket::bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), voxshift_send_port)).await?;
        let outbound = UdpSocket::bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0)).await?;
        Ok(Self {
            inbound,
            outbound,
            voxshift_receive_addr: SocketAddr::new(Ipv4Addr::LOCALHOST.into(), voxshift_receive_port),
        })
    }

    pub async fn send_mute_self(&self, muted: bool) -> std::io::Result<()> {
        self.send_message("/avatar/parameters/MuteSelf", vec![OscType::Bool(muted)])
            .await
    }

    pub async fn send_avatar_change(&self) -> std::io::Result<()> {
        self.send_message("/avatar/change", vec![]).await
    }

    async fn send_message(&self, addr: &str, args: Vec<OscType>) -> std::io::Result<()> {
        let packet = OscPacket::Message(OscMessage {
            addr: addr.to_string(),
            args,
        });
        let bytes = rosc::encoder::encode(&packet)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:?}")))?;
        self.outbound.send_to(&bytes, self.voxshift_receive_addr).await?;
        Ok(())
    }

    /// Waits for the next `/input/Voice` value VoxShift sends. VRChat's
    /// "OSC as Input Controller" docs classify `/input/Voice` as a button
    /// input, which takes an `Int32` of `1`/`0` — not `Float`.
    pub async fn recv_input_voice(&self) -> std::io::Result<i32> {
        let mut buf = [0u8; 4096];
        loop {
            let (len, _src) = self.inbound.recv_from(&mut buf).await?;
            if let Ok((_, OscPacket::Message(msg))) = rosc::decoder::decode_udp(&buf[..len]) {
                if msg.addr == "/input/Voice" {
                    if let Some(OscType::Int(value)) = msg.args.first() {
                        return Ok(*value);
                    }
                }
            }
        }
    }
}
