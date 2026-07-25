//! Discord RPC client — 設計書.md §6.1, §9.2, §11.3.
//!
//! Owns the named pipe connection end-to-end: handshake, generic
//! request/response commands (used for setup, before events start
//! flowing), and the main operational loop that dispatches every
//! subsequently-received frame (both events and delayed command replies)
//! while executing coordinator-issued Discord commands.

use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::NamedPipeClient;
use tokio::sync::mpsc;
use uuid::Uuid;

use voxshift_core::command::DiscordCommand;
use voxshift_core::event::CoordinatorEvent;
use voxshift_core::state::{ConnectionState, MuteState};

use crate::auth::AuthHandles;
use crate::error::DiscordError;
use crate::frame::{self, FrameReader, OpCode};
use crate::models::{Command, HandshakePayload, RpcMessage, VoiceSettingsArgs};
use crate::voice;

const READ_CHUNK: usize = 4096;

const SUBSCRIBED_EVENTS: [&str; 3] = [
    "VOICE_SETTINGS_UPDATE",
    "VOICE_CHANNEL_SELECT",
    "VOICE_CONNECTION_STATUS",
];

pub struct DiscordRpcClient {
    pipe: NamedPipeClient,
    reader: FrameReader,
}

impl DiscordRpcClient {
    /// Sends the RPC handshake and waits for the `READY` dispatch that
    /// confirms it (§6.1.1).
    pub async fn handshake(pipe: NamedPipeClient, client_id: &str) -> Result<Self, DiscordError> {
        let mut client = Self {
            pipe,
            reader: FrameReader::new(),
        };

        let payload = serde_json::to_vec(&HandshakePayload { v: 1, client_id })?;
        let frame = frame::encode(OpCode::Handshake, &payload)?;
        client.pipe.write_all(&frame).await?;

        let (opcode, payload) = client.read_frame().await?;
        if opcode != OpCode::Frame {
            return Err(DiscordError::UnexpectedHandshakeResponse);
        }
        let msg: RpcMessage = serde_json::from_slice(&payload)?;
        if msg.evt.as_deref() != Some("READY") {
            return Err(DiscordError::HandshakeRejected(
                msg.message.unwrap_or_else(|| "no READY event received".to_string()),
            ));
        }

        tracing::info!("discord rpc handshake complete");
        Ok(client)
    }

    /// AUTHENTICATE with a previously-obtained access token (Phase 2 seam —
    /// unused by Phase 1, since no valid token exists yet without the OAuth
    /// flow, but wired so Phase 2 can call it with no changes here).
    pub async fn authenticate(&mut self, access_token: &str) -> Result<(), DiscordError> {
        let resp = self
            .send_command("AUTHENTICATE", serde_json::json!({ "access_token": access_token }))
            .await?;
        if let Some(code) = resp.code {
            return Err(DiscordError::CommandFailed(format!(
                "AUTHENTICATE failed (code {code}): {}",
                resp.message.unwrap_or_default()
            )));
        }
        Ok(())
    }

    async fn read_frame(&mut self) -> Result<(OpCode, Vec<u8>), DiscordError> {
        loop {
            if let Some(frame) = self.reader.try_read_frame()? {
                return Ok(frame);
            }
            let mut buf = [0u8; READ_CHUNK];
            let n = self.pipe.read(&mut buf).await?;
            if n == 0 {
                return Err(DiscordError::PipeClosed);
            }
            self.reader.push(&buf[..n]);
        }
    }

    async fn write_command<T: Serialize>(
        &mut self,
        cmd: &str,
        args: T,
        evt: Option<&str>,
    ) -> Result<Uuid, DiscordError> {
        let nonce = Uuid::new_v4();
        let payload = serde_json::to_vec(&Command {
            cmd: cmd.to_string(),
            args,
            nonce: nonce.to_string(),
            evt: evt.map(str::to_string),
        })?;
        let frame = frame::encode(OpCode::Frame, &payload)?;
        self.pipe.write_all(&frame).await?;
        Ok(nonce)
    }

    /// Request/response helper for use before the main event loop is
    /// running (handshake, authenticate, initial subscribe) — at that point
    /// no unsolicited voice events are expected yet, so simply waiting for
    /// the matching nonce and logging anything else is safe. Once `run()`'s
    /// loop is active, all frames flow through [`Self::handle_incoming_frame`]
    /// instead so nothing is ever silently dropped.
    pub async fn send_command<T: Serialize>(
        &mut self,
        cmd: &str,
        args: T,
    ) -> Result<RpcMessage, DiscordError> {
        self.send_command_with_evt(cmd, args, None).await
    }

    /// As [`Self::send_command`], but also names the event being
    /// (un)subscribed to for `SUBSCRIBE`/`UNSUBSCRIBE` — the only two
    /// commands that carry an `evt` field.
    async fn send_command_with_evt<T: Serialize>(
        &mut self,
        cmd: &str,
        args: T,
        evt: Option<&str>,
    ) -> Result<RpcMessage, DiscordError> {
        let nonce = self.write_command(cmd, args, evt).await?;
        let nonce_str = nonce.to_string();
        loop {
            let (opcode, payload) = self.read_frame().await?;
            if opcode != OpCode::Frame {
                continue;
            }
            let msg: RpcMessage = serde_json::from_slice(&payload)?;
            if msg.nonce.as_deref() == Some(nonce_str.as_str()) {
                return Ok(msg);
            }
            tracing::debug!(?msg, "dropping unrelated frame before the main event loop is running");
        }
    }

    /// Subscribes to the voice-related events and fetches the current voice
    /// settings — used both right after a successful handshake (when a
    /// stored session was already restored) and again right after a
    /// just-completed [`crate::auth::authorize_now`] (since the very first
    /// attempt, made before any access token existed, could only have failed
    /// with a "not authenticated" error). Waits for each reply in turn
    /// (rather than firing-and-forgetting) so an `ERROR` frame — e.g. a
    /// missing scope or an unapproved RPC application — is actually
    /// noticed instead of silently vanishing (§ handle_discord_command_ack
    /// only recognizes acks for coordinator-tracked nonces, which these
    /// setup commands don't have).
    async fn subscribe_and_seed_state(
        &mut self,
        event_tx: &mpsc::Sender<CoordinatorEvent>,
    ) -> Result<(), DiscordError> {
        for evt in SUBSCRIBED_EVENTS {
            let resp = self
                .send_command_with_evt("SUBSCRIBE", serde_json::json!({}), Some(evt))
                .await?;
            report_command_error(&resp, event_tx, &format!("subscribing to {evt}")).await;
        }

        let resp = self
            .send_command_with_evt("GET_VOICE_SETTINGS", serde_json::json!({}), None)
            .await?;
        if resp.code.is_some() {
            report_command_error(&resp, event_tx, "fetching discord voice settings").await;
        } else {
            forward_voice_settings(&resp, event_tx).await;
        }
        Ok(())
    }

    /// Main operational loop. Subscribes to the voice-related events, then
    /// dispatches every subsequent frame (events and delayed command
    /// replies alike) and executes coordinator-issued `DiscordCommand`s.
    /// Returns (rather than panics) on any transport failure so the
    /// supervisor in voxshift-app can apply the §13.1 reconnect backoff.
    /// Borrows `cmd_rx` rather than owning it, so the same receiver survives
    /// across reconnect attempts in the voxshift-app supervisor loop — a
    /// channel receiver can't be recreated once dropped, and the coordinator
    /// only ever holds a single sender for it. `authorize_rx` carries
    /// on-demand authorization requests (e.g. a Settings screen button) —
    /// handled here, rather than in the supervisor, because servicing one
    /// requires exclusive `&mut self` access to the same connection this
    /// loop already owns.
    pub async fn run(
        mut self,
        event_tx: mpsc::Sender<CoordinatorEvent>,
        cmd_rx: &mut mpsc::Receiver<DiscordCommand>,
        authorize_rx: &mut mpsc::Receiver<()>,
        auth: &AuthHandles<'_>,
    ) -> DiscordError {
        // Seed initial Discord state (§12.1 "Discord状態取得"). If no session
        // has been restored yet (fresh install — try_restore_session
        // returned None), Discord will reject every one of these with a
        // "not authenticated" error; that's fine and expected here, and
        // report_command_error surfaces it rather than dropping it silently.
        // The same seeding runs again, successfully, once the user
        // authorizes via the `authorize_rx` branch below.
        if let Err(err) = self.subscribe_and_seed_state(&event_tx).await {
            return err;
        }

        let _ = event_tx
            .send(CoordinatorEvent::DiscordConnectionChanged(ConnectionState::Connected))
            .await;

        loop {
            tokio::select! {
                frame = self.read_frame() => {
                    match frame {
                        Ok((OpCode::Frame, payload)) => handle_incoming_frame(&payload, &event_tx).await,
                        Ok(_) => {} // handshake/close/ping/pong frames need no action here
                        Err(err) => return err,
                    }
                }
                maybe_cmd = cmd_rx.recv() => {
                    match maybe_cmd {
                        Some(cmd) => {
                            if let Err(err) = self.handle_discord_command(cmd).await {
                                return err;
                            }
                        }
                        None => return DiscordError::CommandChannelClosed,
                    }
                }
                maybe_authorize = authorize_rx.recv() => {
                    // A closed channel (`None`) just means the app is
                    // shutting down and no more requests will ever arrive —
                    // not fatal to this loop, so don't return/break here.
                    if maybe_authorize.is_some() {
                        tracing::info!("discord authorization requested");
                        match crate::auth::authorize_now(&mut self, auth.store, auth.oauth, auth.cfg).await {
                            Ok(_) => {
                                tracing::info!("discord authorization completed successfully");
                                // The very first SUBSCRIBE/GET_VOICE_SETTINGS
                                // attempts (sent at the top of this function)
                                // ran before authentication and so were
                                // rejected — redo them now so the coordinator
                                // both learns Discord's current mute state
                                // and actually receives future
                                // VOICE_SETTINGS_UPDATE events, instead of
                                // staying stuck at Unknown forever.
                                if let Err(err) = self.subscribe_and_seed_state(&event_tx).await {
                                    return err;
                                }
                            }
                            Err(err) => {
                                tracing::warn!(error = %err, "discord authorization failed");
                                let _ = event_tx
                                    .send(CoordinatorEvent::DiscordVoiceStateUnavailable(format!(
                                        "Discord authorization failed: {err}"
                                    )))
                                    .await;
                            }
                        }
                    }
                }
            }
        }
    }

    async fn handle_discord_command(&mut self, cmd: DiscordCommand) -> Result<(), DiscordError> {
        match cmd {
            DiscordCommand::SetMute { target, nonce } => {
                // Reuses the coordinator-issued nonce (rather than
                // generating a new one) so the resulting reply/echo can be
                // recognized by the coordinator's own pending-command
                // tracking (§11.4).
                let payload = serde_json::to_vec(&Command {
                    cmd: "SET_VOICE_SETTINGS".to_string(),
                    args: VoiceSettingsArgs {
                        mute: matches!(target, MuteState::Muted),
                    },
                    nonce: nonce.to_string(),
                    evt: None,
                })?;
                let frame = frame::encode(OpCode::Frame, &payload)?;
                self.pipe.write_all(&frame).await?;
                Ok(())
            }
        }
    }

}

/// Parses and routes one already-defragmented RPC frame payload. Free
/// function (no `self`) so it's directly unit-testable with a plain mpsc
/// channel and no live pipe. Never panics on malformed input (§16/§23.1
/// "不正JSONを安全に破棄できる").
async fn handle_incoming_frame(payload: &[u8], event_tx: &mpsc::Sender<CoordinatorEvent>) {
    let msg: RpcMessage = match serde_json::from_slice(payload) {
        Ok(m) => m,
        Err(err) => {
            tracing::warn!(error = %err, "dropping malformed discord rpc json");
            return;
        }
    };

    if let Some(code) = msg.code {
        tracing::warn!(code, message = ?msg.message, "discord rpc error frame");
        if let Some(nonce) = msg.nonce.as_deref().and_then(|s| Uuid::parse_str(s).ok()) {
            let _ = event_tx
                .send(CoordinatorEvent::DiscordCommandAck {
                    nonce,
                    result: Err(msg
                        .message
                        .clone()
                        .unwrap_or_else(|| "discord rpc error".to_string())),
                })
                .await;
        }
        return;
    }

    match msg.evt.as_deref() {
        Some("VOICE_SETTINGS_UPDATE") => forward_voice_settings(&msg, event_tx).await,
        Some("VOICE_CHANNEL_SELECT") => {
            let in_voice = msg
                .data
                .as_ref()
                .is_some_and(voice::parse_voice_channel_selected);
            let _ = event_tx
                .send(CoordinatorEvent::DiscordVoiceChannelStatus { in_voice })
                .await;
        }
        Some("VOICE_CONNECTION_STATUS") => {
            // Informational only for Phase 1 — the supervisor loop in
            // voxshift-app owns connection-level state transitions.
        }
        _ => {
            if matches!(
                msg.cmd.as_deref(),
                Some("GET_VOICE_SETTINGS") | Some("SET_VOICE_SETTINGS")
            ) {
                forward_voice_settings(&msg, event_tx).await;
            }
        }
    }
}

/// Reports an `ERROR` frame (non-null `code`) received in reply to a setup
/// command (`SUBSCRIBE`/`GET_VOICE_SETTINGS`) that has no coordinator-tracked
/// nonce to match against `DiscordCommandAck`. Without this, e.g. a missing
/// scope or an unapproved RPC application would only ever produce a
/// `tracing::warn!` invisible to the user.
async fn report_command_error(resp: &RpcMessage, event_tx: &mpsc::Sender<CoordinatorEvent>, context: &str) {
    let Some(code) = resp.code else { return };
    let message = resp.message.clone().unwrap_or_default();
    tracing::warn!(code, %message, context, "discord rejected an rpc setup command");
    let _ = event_tx
        .send(CoordinatorEvent::DiscordVoiceStateUnavailable(format!(
            "Discord error {context} (code {code}): {message}"
        )))
        .await;
}

async fn forward_voice_settings(msg: &RpcMessage, event_tx: &mpsc::Sender<CoordinatorEvent>) {
    let Some(data) = &msg.data else {
        tracing::warn!(cmd = ?msg.cmd, evt = ?msg.evt, "voice settings reply/event had no `data` field at all");
        return;
    };
    match voice::parse_voice_settings_mute(data) {
        Ok(mute) => {
            let _ = event_tx
                .send(CoordinatorEvent::DiscordVoiceSettingsUpdate { mute })
                .await;
        }
        Err(err) => tracing::warn!(error = %err, %data, "malformed voice settings payload"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn malformed_json_frame_is_safely_dropped() {
        let (tx, mut rx) = mpsc::channel(4);
        handle_incoming_frame(b"{ not valid json", &tx).await;
        assert!(rx.try_recv().is_err(), "no event should be emitted for malformed json");
    }

    #[tokio::test]
    async fn voice_settings_update_event_forwards_mute_state() {
        let (tx, mut rx) = mpsc::channel(4);
        let payload = serde_json::to_vec(&serde_json::json!({
            "cmd": null, "evt": "VOICE_SETTINGS_UPDATE", "nonce": null, "code": null, "message": null,
            "data": { "mute": true }
        }))
        .unwrap();
        handle_incoming_frame(&payload, &tx).await;
        match rx.try_recv().unwrap() {
            CoordinatorEvent::DiscordVoiceSettingsUpdate { mute } => assert_eq!(mute, MuteState::Muted),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_frame_with_nonce_emits_command_ack_failure() {
        let (tx, mut rx) = mpsc::channel(4);
        let nonce = Uuid::new_v4();
        let payload = serde_json::to_vec(&serde_json::json!({
            "cmd": null, "evt": null, "data": null,
            "nonce": nonce.to_string(), "code": 4006, "message": "not authenticated"
        }))
        .unwrap();
        handle_incoming_frame(&payload, &tx).await;
        match rx.try_recv().unwrap() {
            CoordinatorEvent::DiscordCommandAck { nonce: got, result } => {
                assert_eq!(got, nonce);
                assert!(result.is_err());
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_voice_settings_reply_forwards_mute_state() {
        let (tx, mut rx) = mpsc::channel(4);
        let payload = serde_json::to_vec(&serde_json::json!({
            "cmd": "GET_VOICE_SETTINGS", "evt": null, "nonce": Uuid::new_v4().to_string(),
            "code": null, "message": null, "data": { "mute": false }
        }))
        .unwrap();
        handle_incoming_frame(&payload, &tx).await;
        match rx.try_recv().unwrap() {
            CoordinatorEvent::DiscordVoiceSettingsUpdate { mute } => assert_eq!(mute, MuteState::Unmuted),
            other => panic!("unexpected event: {other:?}"),
        }
    }
}
