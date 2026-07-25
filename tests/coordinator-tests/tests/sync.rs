//! End-to-end integration test: a real `Coordinator` wired to the real
//! `voxshift-vrchat` OSC adapters and the real `voxshift-discord` RPC
//! client, driven against `osc-mock`'s fake VRChat peer and
//! `discord-mock`'s scripted Discord named pipe server (§23.2: "疑似Discord
//! Named Pipe", "疑似VRChat OSC送受信", "DiscordとVRChatの同時操作" in
//! spirit — both directions of the sync are exercised).

use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;
use uuid::Uuid;

use voxshift_core::command::{CoordinatorCommand, DiscordCommand, VrChatCommand};
use voxshift_core::coordinator::{Coordinator, CoordinatorConfig};
use voxshift_core::error::CoreError;
use voxshift_core::event::CoordinatorEvent;
use voxshift_core::state::{LinkMode, MuteState, ResumePolicy, StartupAuthority};
use voxshift_core::token::{StoredTokenSet, TokenStore};
use voxshift_discord::auth::{AuthConfig, AuthHandles, DiscordOAuthClient};

const TEST_TIMEOUT: Duration = Duration::from_secs(5);
const VRCHAT_SEND_PORT: u16 = 29100; // voxshift -> mock vrchat (its "inbound")
const VRCHAT_RECEIVE_PORT: u16 = 29101; // mock vrchat -> voxshift

/// A no-op `TokenStore` — this test doesn't exercise Discord authorization,
/// only voice-state sync, so nothing is ever stored.
struct NullTokenStore;

impl TokenStore for NullTokenStore {
    fn load(&self) -> Result<Option<StoredTokenSet>, CoreError> {
        Ok(None)
    }
    fn save(&self, _tokens: &StoredTokenSet) -> Result<(), CoreError> {
        Ok(())
    }
    fn clear(&self) -> Result<(), CoreError> {
        Ok(())
    }
}

async fn with_timeout<T>(fut: impl std::future::Future<Output = std::io::Result<T>>) -> T {
    timeout(TEST_TIMEOUT, fut)
        .await
        .expect("operation timed out")
        .expect("operation failed")
}

#[tokio::test]
async fn bidirectional_sync_works_end_to_end_through_real_adapters() {
    // --- Mock Discord named pipe server ---
    let pipe_name = format!(r"\\.\pipe\voxshift-test-{}", Uuid::new_v4());
    let pending_server = discord_mock::create(&pipe_name).expect("failed to create mock discord pipe");

    // --- Mock VRChat OSC peer ---
    let mock_vrchat = osc_mock::MockVrChatPeer::bind(VRCHAT_SEND_PORT, VRCHAT_RECEIVE_PORT)
        .await
        .expect("failed to bind mock vrchat peer");

    // --- Real Coordinator ---
    let (vrchat_cmd_tx, vrchat_cmd_rx) = mpsc::channel::<VrChatCommand>(16);
    let (discord_cmd_tx, discord_cmd_rx) = mpsc::channel::<DiscordCommand>(16);
    let (event_tx, event_rx) = mpsc::channel::<CoordinatorEvent>(64);
    let (_command_tx, command_rx) = mpsc::channel::<CoordinatorCommand>(8);

    let coordinator_cfg = CoordinatorConfig {
        link_mode: LinkMode::InverseBidirectional,
        link_enabled: true,
        startup_authority: StartupAuthority::None, // don't auto-act on the bootstrap sync
        resume_policy: ResumePolicy::SyncFromVrchat,
    };
    let (coordinator, mut snapshot_rx) = Coordinator::new(coordinator_cfg, vrchat_cmd_tx, discord_cmd_tx);
    tokio::spawn(coordinator.run(event_rx, command_rx));

    // --- Real VRChat OSC adapters, pointed at the mock peer's ports ---
    voxshift_vrchat::osc_receiver::spawn(VRCHAT_RECEIVE_PORT, event_tx.clone())
        .await
        .expect("failed to bind vrchat osc receiver");
    let vrchat_sender = voxshift_vrchat::osc_sender::VrChatOscSender::connect(SocketAddr::new(
        Ipv4Addr::LOCALHOST.into(),
        VRCHAT_SEND_PORT,
    ))
    .await
    .expect("failed to connect vrchat osc sender");
    tokio::spawn(voxshift_vrchat::osc_sender::run_command_loop(
        vrchat_sender,
        vrchat_cmd_rx,
        event_tx.clone(),
    ));

    // --- Real Discord RPC client, pointed at the mock pipe ---
    let mut discord_cmd_rx = discord_cmd_rx;
    let discord_event_tx = event_tx.clone();
    tokio::spawn(async move {
        let (_authorize_tx, mut authorize_rx) = mpsc::channel::<()>(1);
        let store = NullTokenStore;
        let oauth = DiscordOAuthClient::new();
        let cfg = AuthConfig { client_id: "test-client-id".to_string() };
        let auth = AuthHandles { store: &store, oauth: &oauth, cfg: &cfg };

        let pipe = voxshift_discord::ipc::connect_to(&pipe_name).expect("mock pipe should accept a client");
        let client = voxshift_discord::client::DiscordRpcClient::handshake(pipe, "test-client-id")
            .await
            .expect("handshake with mock discord server should succeed");
        let _ = client.run(discord_event_tx, &mut discord_cmd_rx, &mut authorize_rx, &auth).await;
    });

    // --- Drive the mock Discord server through the startup handshake ---
    let mut server = with_timeout(pending_server.accept()).await;
    let client_id = timeout(TEST_TIMEOUT, server.expect_handshake())
        .await
        .expect("timed out waiting for handshake")
        .expect("failed to read handshake");
    assert_eq!(client_id, "test-client-id");
    server.send_ready().await.expect("failed to send READY");

    for _ in 0..3 {
        let (cmd, nonce, _args) = timeout(TEST_TIMEOUT, server.expect_command())
            .await
            .expect("timed out waiting for SUBSCRIBE")
            .expect("failed to read SUBSCRIBE");
        assert_eq!(cmd, "SUBSCRIBE");
        // The real client now waits for each SUBSCRIBE's reply (so it can
        // notice an ERROR frame) before sending the next command — ack it,
        // or the client would block here forever.
        server
            .reply_ok(&cmd, &nonce)
            .await
            .expect("failed to ack SUBSCRIBE");
    }

    let (cmd, nonce, _args) = timeout(TEST_TIMEOUT, server.expect_command())
        .await
        .expect("timed out waiting for GET_VOICE_SETTINGS")
        .expect("failed to read GET_VOICE_SETTINGS");
    assert_eq!(cmd, "GET_VOICE_SETTINGS");
    server
        .reply_voice_settings(&cmd, &nonce, false) // Discord starts out unmuted
        .await
        .expect("failed to reply to GET_VOICE_SETTINGS");

    // --- Bootstrap: first VRChat state observation completes initial sync
    // (StartupAuthority::None means it does not itself trigger a command).
    mock_vrchat
        .send_mute_self(true) // mic off
        .await
        .expect("failed to send bootstrap MuteSelf");

    // Wait for both sides to be known before proceeding.
    loop {
        snapshot_rx.changed().await.expect("coordinator task died");
        let snap = snapshot_rx.borrow().clone();
        if snap.vrchat_mute != MuteState::Unknown && snap.discord_mute != MuteState::Unknown {
            break;
        }
    }

    // === Direction 1: VRChat -> Discord ===
    mock_vrchat
        .send_mute_self(false) // mic turns ON
        .await
        .expect("failed to send MuteSelf(false)");

    let (cmd, nonce, args) = timeout(TEST_TIMEOUT, server.expect_command())
        .await
        .expect("timed out waiting for SET_VOICE_SETTINGS")
        .expect("failed to read SET_VOICE_SETTINGS");
    assert_eq!(cmd, "SET_VOICE_SETTINGS");
    assert_eq!(args["mute"], true, "mic ON must mute discord");

    server
        .reply_voice_settings(&cmd, &nonce, true)
        .await
        .expect("failed to ack SET_VOICE_SETTINGS");

    // === Direction 2: Discord -> VRChat ===
    // A different RPC client (or the user) unmutes Discord directly.
    server
        .send_voice_settings_update(false)
        .await
        .expect("failed to send VOICE_SETTINGS_UPDATE");

    let first = timeout(TEST_TIMEOUT, mock_vrchat.recv_input_voice())
        .await
        .expect("timed out waiting for /input/Voice (rising edge)")
        .expect("failed to receive /input/Voice");
    assert_eq!(first, 1);
    let second = timeout(TEST_TIMEOUT, mock_vrchat.recv_input_voice())
        .await
        .expect("timed out waiting for /input/Voice (falling edge)")
        .expect("failed to receive /input/Voice");
    assert_eq!(second, 0);

    // VRChat confirms the toggle: mic goes off, matching the expected
    // target for "discord unmuted -> vrchat mic off" under inverse mode.
    mock_vrchat
        .send_mute_self(true)
        .await
        .expect("failed to send confirming MuteSelf");

    loop {
        snapshot_rx.changed().await.expect("coordinator task died");
        let snap = snapshot_rx.borrow().clone();
        if snap.vrchat_mute == MuteState::Muted && snap.discord_mute == MuteState::Unmuted {
            break;
        }
    }
}
