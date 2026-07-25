//! VRChat OSC sender — 設計書.md §6.3.3 (`/input/Voice` toggle sequence).

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use rosc::{OscMessage, OscPacket, OscType};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use voxshift_core::command::VrChatCommand;
use voxshift_core::event::CoordinatorEvent;
use voxshift_core::state::ConnectionState;

const ADDR_INPUT_VOICE: &str = "/input/Voice";
/// §6.3.3: hold `/input/Voice` at 1 for 60ms before returning it to 0.
const TOGGLE_HOLD: Duration = Duration::from_millis(60);

pub struct VrChatOscSender {
    socket: UdpSocket,
}

impl VrChatOscSender {
    pub async fn connect(target: SocketAddr) -> std::io::Result<Self> {
        let socket = UdpSocket::bind(SocketAddr::new(Ipv4Addr::LOCALHOST.into(), 0)).await?;
        socket.connect(target).await?;
        Ok(Self { socket })
    }

    /// §6.3.3 sequence: `/input/Voice 1`, wait 60ms, `/input/Voice 0`.
    ///
    /// VRChat's "OSC as Input Controller" docs classify `/input/Voice` as a
    /// *button* input (not an axis), and button inputs take an `Int32` of
    /// `1`/`0` for pressed/released — not the `Float32` this previously
    /// sent, which VRChat's OSC parser silently ignored (wrong argument
    /// type), so the toggle never actually reached the avatar.
    pub async fn send_toggle_voice(&self) -> std::io::Result<()> {
        self.send_voice_value(1).await?;
        tokio::time::sleep(TOGGLE_HOLD).await;
        self.send_voice_value(0).await?;
        Ok(())
    }

    async fn send_voice_value(&self, value: i32) -> std::io::Result<()> {
        let packet = OscPacket::Message(OscMessage {
            addr: ADDR_INPUT_VOICE.to_string(),
            args: vec![OscType::Int(value)],
        });
        let bytes = rosc::encoder::encode(&packet)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{e:?}")))?;
        self.socket.send(&bytes).await?;
        Ok(())
    }
}

/// Drains `VrChatCommand`s and executes them. Toggle *confirmation* is not
/// this loop's job — the coordinator infers success from the next observed
/// `MuteSelf` value (§11.4/§11.5); a hard send failure here is reported back
/// as a connection degradation instead.
pub async fn run_command_loop(
    sender: VrChatOscSender,
    mut commands: mpsc::Receiver<VrChatCommand>,
    event_tx: mpsc::Sender<CoordinatorEvent>,
) {
    while let Some(cmd) = commands.recv().await {
        match cmd {
            VrChatCommand::ToggleVoice { .. } => {
                if let Err(err) = sender.send_toggle_voice().await {
                    tracing::warn!(error = %err, "failed to send vrchat toggle");
                    let _ = event_tx
                        .send(CoordinatorEvent::VrChatConnectionChanged(
                            ConnectionState::Degraded,
                        ))
                        .await;
                }
            }
        }
    }
}
