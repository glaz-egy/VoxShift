//! VRChat OSC receiver — 設計書.md §6.3.1/§6.3.2/§6.3.5.

use std::net::{Ipv4Addr, SocketAddr};

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use voxshift_core::event::CoordinatorEvent;
use voxshift_core::state::MuteState;

/// §16: reject/ignore OSC packets larger than this.
const MAX_PACKET_SIZE: usize = 64 * 1024;

const ADDR_MUTE_SELF: &str = "/avatar/parameters/MuteSelf";
const ADDR_AVATAR_CHANGE: &str = "/avatar/change";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedOscEvent {
    MuteSelf(MuteState),
    AvatarChanged,
}

/// Pure parsing/interpretation, kept free of I/O so it's unit-testable
/// without a live socket.
fn interpret_message(msg: &OscMessage) -> Option<ParsedOscEvent> {
    match msg.addr.as_str() {
        ADDR_MUTE_SELF => match msg.args.first() {
            Some(OscType::Bool(true)) => Some(ParsedOscEvent::MuteSelf(MuteState::Muted)),
            Some(OscType::Bool(false)) => Some(ParsedOscEvent::MuteSelf(MuteState::Unmuted)),
            other => {
                tracing::warn!(?other, "MuteSelf received with unexpected argument shape");
                None
            }
        },
        ADDR_AVATAR_CHANGE => Some(ParsedOscEvent::AvatarChanged),
        _ => None,
    }
}

fn flatten_messages(packet: OscPacket, out: &mut Vec<OscMessage>) {
    match packet {
        OscPacket::Message(msg) => out.push(msg),
        OscPacket::Bundle(bundle) => {
            for inner in bundle.content {
                flatten_messages(inner, out);
            }
        }
    }
}

/// Binds `127.0.0.1:{port}` (§6.3.1 — loopback only) and forwards decoded
/// VRChat state to the coordinator via `event_tx`. Never panics or exits on
/// malformed input (§16/§23.1 "不正OSCを安全に破棄できる").
pub async fn spawn(
    port: u16,
    event_tx: mpsc::Sender<CoordinatorEvent>,
) -> std::io::Result<tokio::task::JoinHandle<()>> {
    let addr = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), port);
    let socket = UdpSocket::bind(addr).await?;
    tracing::info!(%addr, "vrchat OSC receiver bound");
    Ok(tokio::spawn(receive_loop(socket, event_tx)))
}

async fn receive_loop(socket: UdpSocket, event_tx: mpsc::Sender<CoordinatorEvent>) {
    let mut buf = vec![0u8; MAX_PACKET_SIZE];
    loop {
        let (len, src) = match socket.recv_from(&mut buf).await {
            Ok(v) => v,
            Err(err) => {
                tracing::warn!(error = %err, "osc recv_from failed; continuing");
                continue;
            }
        };

        if !src.ip().is_loopback() {
            tracing::warn!(%src, "dropping OSC packet from a non-loopback address");
            continue;
        }

        let packet = match rosc::decoder::decode_udp(&buf[..len]) {
            Ok((_, packet)) => packet,
            Err(err) => {
                tracing::warn!(error = ?err, "dropping malformed OSC packet");
                continue;
            }
        };

        let mut messages = Vec::new();
        flatten_messages(packet, &mut messages);
        for msg in &messages {
            if let Some(parsed) = interpret_message(msg) {
                let event = match parsed {
                    ParsedOscEvent::MuteSelf(state) => CoordinatorEvent::VrChatMuteSelf(state),
                    ParsedOscEvent::AvatarChanged => CoordinatorEvent::VrChatAvatarChanged,
                };
                if event_tx.send(event).await.is_err() {
                    return; // coordinator shut down
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn msg(addr: &str, args: Vec<OscType>) -> OscMessage {
        OscMessage {
            addr: addr.to_string(),
            args,
        }
    }

    #[test]
    fn mute_self_true_means_vrchat_mic_off() {
        let m = msg(ADDR_MUTE_SELF, vec![OscType::Bool(true)]);
        assert_eq!(
            interpret_message(&m),
            Some(ParsedOscEvent::MuteSelf(MuteState::Muted))
        );
    }

    #[test]
    fn mute_self_false_means_vrchat_mic_on() {
        let m = msg(ADDR_MUTE_SELF, vec![OscType::Bool(false)]);
        assert_eq!(
            interpret_message(&m),
            Some(ParsedOscEvent::MuteSelf(MuteState::Unmuted))
        );
    }

    #[test]
    fn avatar_change_is_recognized() {
        let m = msg(ADDR_AVATAR_CHANGE, vec![]);
        assert_eq!(interpret_message(&m), Some(ParsedOscEvent::AvatarChanged));
    }

    #[test]
    fn malformed_mute_self_argument_is_safely_ignored() {
        let m = msg(ADDR_MUTE_SELF, vec![OscType::Int(1)]);
        assert_eq!(interpret_message(&m), None);
    }

    #[test]
    fn missing_mute_self_argument_is_safely_ignored() {
        let m = msg(ADDR_MUTE_SELF, vec![]);
        assert_eq!(interpret_message(&m), None);
    }

    #[test]
    fn unknown_address_is_ignored() {
        let m = msg("/some/other/address", vec![OscType::Bool(true)]);
        assert_eq!(interpret_message(&m), None);
    }

    #[test]
    fn bundle_is_flattened_into_its_messages() {
        let bundle = OscPacket::Bundle(rosc::OscBundle {
            timetag: rosc::OscTime { seconds: 0, fractional: 0 },
            content: vec![
                OscPacket::Message(msg(ADDR_MUTE_SELF, vec![OscType::Bool(true)])),
                OscPacket::Message(msg(ADDR_AVATAR_CHANGE, vec![])),
            ],
        });
        let mut out = Vec::new();
        flatten_messages(bundle, &mut out);
        assert_eq!(out.len(), 2);
    }
}
