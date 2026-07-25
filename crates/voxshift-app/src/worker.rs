//! Assembles the Voice Coordinator plus the VRChat OSC and Discord RPC
//! adapters on a single dedicated background thread.
//!
//! §9.1/§9.2: the UI (Slint, Phase 3) must run its event loop on the main
//! thread and must never block on I/O — so all Tokio work lives on exactly
//! one other OS thread, running a `current_thread` runtime. This module is
//! the seam between the two: it returns a command sender, a snapshot
//! watch-receiver, and a `Handle` the caller can use to spawn further tasks
//! (e.g. a UI snapshot-bridge task) onto that same worker thread rather than
//! creating yet another one.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::mpsc as std_mpsc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use voxshift_core::command::{CoordinatorCommand, DiscordCommand, VrChatCommand};
use voxshift_core::coordinator::{Coordinator, CoordinatorConfig};
use voxshift_core::event::CoordinatorEvent;
use voxshift_core::state::{AppSnapshot, ConnectionState};
use voxshift_core::token::TokenStore;
use voxshift_discord::auth::{AuthConfig, DiscordOAuthClient};
use voxshift_platform_windows::credentials::WindowsCredentialStore;
use voxshift_storage::config::AppConfig;

/// §13.1 Discord reconnect backoff ladder, capped at 30s.
const DISCORD_BACKOFF_SECS: [u64; 6] = [1, 2, 5, 10, 15, 30];

/// Embedded at build time (see `.cargo/config.toml`) rather than read from
/// config.json — every VoxShift build talks to the same Discord
/// application, so the Client ID isn't a per-user runtime setting. It is
/// not secret (public client, §6.2).
const DISCORD_CLIENT_ID: &str = env!("VOXSHIFT_DISCORD_CLIENT_ID");

// `snapshot_rx` and `thread` aren't consumed yet — this phase has no GUI to
// subscribe to state snapshots, and the headless harness doesn't join the
// worker thread on shutdown. Both are part of the contract Phase 3's Slint
// glue relies on (integration note #1), so they stay on the struct now
// rather than being added later.
#[allow(dead_code)]
pub struct WorkerHandle {
    pub command_tx: mpsc::Sender<CoordinatorCommand>,
    pub snapshot_rx: watch::Receiver<AppSnapshot>,
    pub runtime: tokio::runtime::Handle,
    pub thread: std::thread::JoinHandle<()>,
    /// Send `()` to request Discord authorization right now (e.g. a
    /// Settings screen button) — see the module docs on why this is opt-in
    /// rather than automatic on first launch.
    pub authorize_tx: mpsc::Sender<()>,
}

struct WorkerReady {
    command_tx: mpsc::Sender<CoordinatorCommand>,
    snapshot_rx: watch::Receiver<AppSnapshot>,
    runtime_handle: tokio::runtime::Handle,
    authorize_tx: mpsc::Sender<()>,
}

/// Bundles what `ensure_authenticated`/the background refresh loop need.
#[derive(Clone)]
struct DiscordAuthContext {
    store: std::sync::Arc<dyn TokenStore>,
    oauth: DiscordOAuthClient,
    cfg: AuthConfig,
}

fn build_auth_context() -> DiscordAuthContext {
    let store: std::sync::Arc<dyn TokenStore> = std::sync::Arc::new(WindowsCredentialStore::new());
    let cfg = AuthConfig {
        client_id: DISCORD_CLIENT_ID.to_string(),
    };
    let oauth = DiscordOAuthClient::new();
    DiscordAuthContext { store, oauth, cfg }
}

pub fn spawn_worker(config: AppConfig) -> std::io::Result<WorkerHandle> {
    let (ready_tx, ready_rx) = std_mpsc::channel::<WorkerReady>();

    let thread = std::thread::Builder::new()
        .name("voxshift-worker".to_string())
        .spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("failed to build the voxshift worker tokio runtime");
            let runtime_handle = rt.handle().clone();

            rt.block_on(async move {
                let (command_tx, command_rx) = mpsc::channel::<CoordinatorCommand>(64);
                let (event_tx, event_rx) = mpsc::channel::<CoordinatorEvent>(64);
                let (vrchat_cmd_tx, vrchat_cmd_rx) = mpsc::channel::<VrChatCommand>(16);
                let (discord_cmd_tx, discord_cmd_rx) = mpsc::channel::<DiscordCommand>(16);
                let (authorize_tx, authorize_rx) = mpsc::channel::<()>(1);

                let coordinator_cfg = CoordinatorConfig {
                    link_mode: config.link_mode,
                    link_enabled: config.link_enabled,
                    startup_authority: config.startup_authority,
                    resume_policy: config.resume_policy,
                };
                let (coordinator, snapshot_rx) =
                    Coordinator::new(coordinator_cfg, vrchat_cmd_tx, discord_cmd_tx);

                let _ = ready_tx.send(WorkerReady {
                    command_tx: command_tx.clone(),
                    snapshot_rx: snapshot_rx.clone(),
                    runtime_handle: runtime_handle.clone(),
                    authorize_tx: authorize_tx.clone(),
                });
                // Drop this thread-local sender now that the caller has its
                // own clone — otherwise the command channel would never
                // fully close (and `run()`'s `commands.recv()` never return
                // `None`) even after the caller drops every `command_tx` it
                // was given.
                drop(command_tx);
                drop(authorize_tx);

                spawn_vrchat_adapters(&config, event_tx.clone(), vrchat_cmd_rx).await;

                let auth_ctx = build_auth_context();
                tokio::spawn(voxshift_discord::auth::run_refresh_loop(
                    auth_ctx.store.clone(),
                    auth_ctx.oauth.clone(),
                    auth_ctx.cfg.clone(),
                ));

                tokio::spawn(discord_supervisor(event_tx.clone(), discord_cmd_rx, authorize_rx, auth_ctx));

                tokio::spawn(log_snapshot_changes(snapshot_rx));

                coordinator.run(event_rx, command_rx).await;
            });
        })?;

    let ready = ready_rx
        .recv()
        .map_err(|_| std::io::Error::other("voxshift worker thread failed to start"))?;

    Ok(WorkerHandle {
        command_tx: ready.command_tx,
        snapshot_rx: ready.snapshot_rx,
        runtime: ready.runtime_handle,
        thread,
        authorize_tx: ready.authorize_tx,
    })
}

async fn spawn_vrchat_adapters(
    config: &AppConfig,
    event_tx: mpsc::Sender<CoordinatorEvent>,
    vrchat_cmd_rx: mpsc::Receiver<VrChatCommand>,
) {
    if let Err(err) =
        voxshift_vrchat::osc_receiver::spawn(config.vrchat.receive_port, event_tx.clone()).await
    {
        tracing::error!(
            error = %err,
            port = config.vrchat.receive_port,
            "failed to bind vrchat OSC receiver; VRChat state will stay unknown this session"
        );
    }

    let host: IpAddr = config
        .vrchat
        .host
        .parse()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let target = SocketAddr::new(host, config.vrchat.send_port);

    match voxshift_vrchat::osc_sender::VrChatOscSender::connect(target).await {
        Ok(sender) => {
            tokio::spawn(voxshift_vrchat::osc_sender::run_command_loop(
                sender,
                vrchat_cmd_rx,
                event_tx,
            ));
        }
        Err(err) => {
            tracing::error!(error = %err, "failed to initialize vrchat OSC sender; vrchat commands will be dropped");
            tokio::spawn(drain_vrchat_commands(vrchat_cmd_rx));
        }
    }
}

async fn drain_vrchat_commands(mut rx: mpsc::Receiver<VrChatCommand>) {
    while rx.recv().await.is_some() {
        tracing::warn!("dropping vrchat command: OSC sender unavailable");
    }
}

/// Discover -> handshake -> run, forever, with the §13.1 backoff ladder
/// between attempts. Never returns — Discord being entirely unavailable is
/// a normal, tolerated degraded state, not a fatal one (VRChat-only sync
/// keeps working).
async fn discord_supervisor(
    event_tx: mpsc::Sender<CoordinatorEvent>,
    mut cmd_rx: mpsc::Receiver<DiscordCommand>,
    mut authorize_rx: mpsc::Receiver<()>,
    auth_ctx: DiscordAuthContext,
) {
    let mut attempt = 0usize;
    loop {
        let _ = event_tx
            .send(CoordinatorEvent::DiscordConnectionChanged(ConnectionState::Connecting))
            .await;

        match voxshift_discord::ipc::discover_and_connect().await {
            Ok(pipe) => {
                match voxshift_discord::client::DiscordRpcClient::handshake(pipe, DISCORD_CLIENT_ID).await
                {
                    Ok(mut client) => {
                        attempt = 0; // reset backoff after a successful handshake

                        // Silently restore/refresh a stored session only —
                        // never auto-triggers the AUTHORIZE consent dialog.
                        // On first run (nothing stored), Discord just stays
                        // connected-but-unauthorized until the user clicks
                        // "Authorize" in Settings.
                        match voxshift_discord::auth::try_restore_session(
                            &mut client,
                            auth_ctx.store.as_ref(),
                            &auth_ctx.oauth,
                            &auth_ctx.cfg,
                        )
                        .await
                        {
                            Ok(Some(_)) => tracing::info!("discord session restored from stored credentials"),
                            Ok(None) => tracing::info!("discord not yet authorized; waiting for user action"),
                            Err(err) => tracing::warn!(error = %err, "failed to restore discord session"),
                        }

                        let auth_handles = voxshift_discord::auth::AuthHandles {
                            store: auth_ctx.store.as_ref(),
                            oauth: &auth_ctx.oauth,
                            cfg: &auth_ctx.cfg,
                        };
                        let err = client
                            .run(event_tx.clone(), &mut cmd_rx, &mut authorize_rx, &auth_handles)
                            .await;
                        tracing::warn!(error = %err, "discord rpc session ended; will attempt to reconnect");
                    }
                    Err(err) => tracing::debug!(error = %err, "discord rpc handshake failed"),
                }
            }
            Err(err) => tracing::debug!(error = %err, "discord rpc pipe not available"),
        }

        let _ = event_tx
            .send(CoordinatorEvent::DiscordConnectionChanged(ConnectionState::Disconnected))
            .await;

        let secs = DISCORD_BACKOFF_SECS[attempt.min(DISCORD_BACKOFF_SECS.len() - 1)];
        attempt += 1;
        tokio::time::sleep(Duration::from_secs(secs)).await;
    }
}

/// Stand-in for the GUI (not wired in until Phase 3) — logs every state
/// transition so the app is observable without a window.
async fn log_snapshot_changes(mut rx: watch::Receiver<AppSnapshot>) {
    loop {
        if rx.changed().await.is_err() {
            return;
        }
        let snapshot = rx.borrow_and_update().clone();
        tracing::info!(?snapshot, "state updated");
    }
}
