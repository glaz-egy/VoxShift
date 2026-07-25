//! A scripted stand-in for the Discord desktop client's RPC named pipe, used
//! by integration tests that exercise the real `voxshift-discord` client
//! against something other than a live Discord install.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

use voxshift_discord::frame::{self, FrameReader, OpCode};

pub struct PendingMockDiscordServer {
    pipe: NamedPipeServer,
}

impl PendingMockDiscordServer {
    /// Blocks (async) until a client connects.
    pub async fn accept(self) -> std::io::Result<MockDiscordServer> {
        self.pipe.connect().await?;
        Ok(MockDiscordServer {
            pipe: self.pipe,
            reader: FrameReader::new(),
        })
    }
}

/// Creates (but does not accept a connection on) a named pipe server at
/// `pipe_name`. Use a unique per-test name (e.g. including a UUID) so
/// parallel tests — and a real Discord install, if one happens to be
/// running on the test machine — never collide.
pub fn create(pipe_name: &str) -> std::io::Result<PendingMockDiscordServer> {
    let pipe = ServerOptions::new()
        .first_pipe_instance(true)
        .create(pipe_name)?;
    Ok(PendingMockDiscordServer { pipe })
}

pub struct MockDiscordServer {
    pipe: NamedPipeServer,
    reader: FrameReader,
}

impl MockDiscordServer {
    async fn read_frame(&mut self) -> std::io::Result<(OpCode, Vec<u8>)> {
        loop {
            match self.reader.try_read_frame() {
                Ok(Some(frame)) => return Ok(frame),
                Ok(None) => {}
                Err(err) => {
                    return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()));
                }
            }
            let mut buf = [0u8; 4096];
            let n = self.pipe.read(&mut buf).await?;
            self.reader.push(&buf[..n]);
        }
    }

    async fn write_frame(&mut self, opcode: OpCode, payload: &serde_json::Value) -> std::io::Result<()> {
        let bytes = serde_json::to_vec(payload).expect("test payload always serializes");
        let frame = frame::encode(opcode, &bytes).expect("test payload within size limit");
        self.pipe.write_all(&frame).await
    }

    /// Reads the client's handshake frame and returns the `client_id` it
    /// sent.
    pub async fn expect_handshake(&mut self) -> std::io::Result<String> {
        let (opcode, payload) = self.read_frame().await?;
        assert_eq!(opcode, OpCode::Handshake, "expected a HANDSHAKE frame first");
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        Ok(value["client_id"].as_str().unwrap_or_default().to_string())
    }

    pub async fn send_ready(&mut self) -> std::io::Result<()> {
        self.write_frame(
            OpCode::Frame,
            &serde_json::json!({
                "cmd": null, "evt": "READY", "data": {}, "nonce": null, "code": null, "message": null
            }),
        )
        .await
    }

    /// Reads one command frame, returning `(cmd, nonce, args)`. Skips
    /// nothing — callers that expect a specific sequence (e.g. three
    /// `SUBSCRIBE`s then a `GET_VOICE_SETTINGS`) should call this once per
    /// expected command.
    pub async fn expect_command(&mut self) -> std::io::Result<(String, String, serde_json::Value)> {
        let (opcode, payload) = self.read_frame().await?;
        assert_eq!(opcode, OpCode::Frame);
        let value: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        let cmd = value["cmd"].as_str().unwrap_or_default().to_string();
        let nonce = value["nonce"].as_str().unwrap_or_default().to_string();
        Ok((cmd, nonce, value["args"].clone()))
    }

    /// Replies to any command (e.g. `SUBSCRIBE`) with a generic success —
    /// an empty `data` object and no error `code`.
    pub async fn reply_ok(&mut self, cmd: &str, nonce: &str) -> std::io::Result<()> {
        self.write_frame(
            OpCode::Frame,
            &serde_json::json!({
                "cmd": cmd, "evt": null, "nonce": nonce, "code": null, "message": null, "data": {}
            }),
        )
        .await
    }

    /// Replies to a `GET_VOICE_SETTINGS`/`SET_VOICE_SETTINGS` command with
    /// the given mute state.
    pub async fn reply_voice_settings(&mut self, cmd: &str, nonce: &str, mute: bool) -> std::io::Result<()> {
        self.write_frame(
            OpCode::Frame,
            &serde_json::json!({
                "cmd": cmd, "evt": null, "nonce": nonce, "code": null, "message": null,
                "data": { "mute": mute }
            }),
        )
        .await
    }

    /// Sends an unsolicited `VOICE_SETTINGS_UPDATE` event, as Discord would
    /// when the user (or another RPC client) changes the mute state.
    pub async fn send_voice_settings_update(&mut self, mute: bool) -> std::io::Result<()> {
        self.write_frame(
            OpCode::Frame,
            &serde_json::json!({
                "cmd": null, "evt": "VOICE_SETTINGS_UPDATE", "nonce": null, "code": null, "message": null,
                "data": { "mute": mute }
            }),
        )
        .await
    }
}
