//! Voice Coordinator — 設計書.md §11 (state reconciliation algorithm).
//!
//! This is the single place that mutates VRChat/Discord mute state and issues
//! commands to the adapters. It depends only on the abstract event/command
//! types defined in this crate — never on voxshift-vrchat/voxshift-discord
//! directly — so it stays independently testable with fake channels.

use std::time::{Duration, SystemTime};

use tokio::sync::{mpsc, watch};
use tokio::time::Instant as TokioInstant;
use uuid::Uuid;

use crate::command::{CoordinatorCommand, DiscordCommand, VrChatCommand};
use crate::event::CoordinatorEvent;
use crate::state::{
    AppSnapshot, ConnectionState, LinkMode, LinkState, MuteState, ResumePolicy, StartupAuthority,
};

/// §11.5: VRChat toggle safety deadline — spec gives this one explicitly (1.5s).
const VRCHAT_COMMAND_TIMEOUT: Duration = Duration::from_millis(1500);
/// Discord round-trip deadline — §11 does not give an explicit number for
/// this side; 2.0s is a placeholder RTT budget, easily tunable later.
const DISCORD_COMMAND_TIMEOUT: Duration = Duration::from_millis(2000);

/// §11.4 — tracks a Discord `SET_VOICE_SETTINGS` command we issued, so the
/// matching `nonce` (or the resulting `VOICE_SETTINGS_UPDATE` echo) can be
/// recognized as our own operation rather than reprocessed as a user action.
#[derive(Debug, Clone)]
struct PendingDiscordCommand {
    nonce: Uuid,
    target: MuteState,
    #[allow(dead_code)]
    issued_at: TokioInstant,
    deadline: TokioInstant,
}

/// §11.4 — VRChat has no per-command id, so confirmation is inferred by
/// comparing the next observed `MuteSelf` value against what we expect.
#[derive(Debug, Clone)]
struct PendingVrChatCommand {
    #[allow(dead_code)]
    previous: MuteState,
    expected: MuteState, // compared against the observed MuteSelf on confirmation
    #[allow(dead_code)]
    issued_at: TokioInstant,
    deadline: TokioInstant,
}

#[derive(Debug, Clone)]
pub struct CoordinatorConfig {
    pub link_mode: LinkMode,
    pub link_enabled: bool,
    pub startup_authority: StartupAuthority,
    pub resume_policy: ResumePolicy,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        Self {
            link_mode: LinkMode::InverseBidirectional,
            link_enabled: true,
            startup_authority: StartupAuthority::Vrchat,
            resume_policy: ResumePolicy::SyncFromVrchat,
        }
    }
}

pub struct Coordinator {
    vrchat_state: MuteState,
    vrchat_connection: ConnectionState,

    discord_state: MuteState,
    discord_connection: ConnectionState,
    discord_in_voice: bool,

    link_mode: LinkMode,
    link_state: LinkState,

    resume_policy: ResumePolicy,
    startup_authority: StartupAuthority,
    initial_sync_done: bool,

    pending_discord: Option<PendingDiscordCommand>,
    pending_vrchat: Option<PendingVrChatCommand>,

    /// Monotonically increasing counter assigned to every processed event.
    /// §11.6 conflict resolution: a new external state observed after
    /// `last_command_sequence` always wins over a still-in-flight command
    /// we issued earlier — pending commands are discarded rather than
    /// fought.
    next_sequence: u64,
    last_command_sequence: u64,

    last_sync_at: Option<SystemTime>,
    last_error: Option<String>,

    vrchat_tx: mpsc::Sender<VrChatCommand>,
    discord_tx: mpsc::Sender<DiscordCommand>,
    snapshot_tx: watch::Sender<AppSnapshot>,
}

impl Coordinator {
    pub fn new(
        cfg: CoordinatorConfig,
        vrchat_tx: mpsc::Sender<VrChatCommand>,
        discord_tx: mpsc::Sender<DiscordCommand>,
    ) -> (Self, watch::Receiver<AppSnapshot>) {
        let initial = AppSnapshot {
            link_mode: cfg.link_mode,
            link_state: if cfg.link_enabled {
                LinkState::WaitingForState
            } else {
                LinkState::Paused
            },
            ..AppSnapshot::default()
        };
        let (snapshot_tx, snapshot_rx) = watch::channel(initial);
        let this = Self {
            vrchat_state: MuteState::Unknown,
            vrchat_connection: ConnectionState::Disconnected,
            discord_state: MuteState::Unknown,
            discord_connection: ConnectionState::Disconnected,
            discord_in_voice: false,
            link_mode: cfg.link_mode,
            link_state: if cfg.link_enabled {
                LinkState::WaitingForState
            } else {
                LinkState::Paused
            },
            resume_policy: cfg.resume_policy,
            startup_authority: cfg.startup_authority,
            initial_sync_done: false,
            pending_discord: None,
            pending_vrchat: None,
            next_sequence: 0,
            last_command_sequence: 0,
            last_sync_at: None,
            last_error: None,
            vrchat_tx,
            discord_tx,
            snapshot_tx,
        };
        (this, snapshot_rx)
    }

    /// Runs until the event channel closes or `CoordinatorCommand::Shutdown`
    /// is received. Never returns early on malformed/unexpected input (§16).
    pub async fn run(
        mut self,
        mut events: mpsc::Receiver<CoordinatorEvent>,
        mut commands: mpsc::Receiver<CoordinatorCommand>,
    ) {
        loop {
            let deadline = self.next_deadline();
            let timeout = async {
                match deadline {
                    Some(d) => tokio::time::sleep_until(d).await,
                    None => std::future::pending::<()>().await,
                }
            };

            tokio::select! {
                maybe_ev = events.recv() => {
                    match maybe_ev {
                        Some(ev) => self.handle_event(ev).await,
                        None => break,
                    }
                }
                maybe_cmd = commands.recv() => {
                    match maybe_cmd {
                        Some(CoordinatorCommand::Shutdown) | None => break,
                        Some(cmd) => self.handle_command(cmd).await,
                    }
                }
                _ = timeout => {
                    self.handle_pending_timeout();
                }
            }
        }
    }

    fn next_deadline(&self) -> Option<TokioInstant> {
        match (&self.pending_vrchat, &self.pending_discord) {
            (Some(v), Some(d)) => Some(v.deadline.min(d.deadline)),
            (Some(v), None) => Some(v.deadline),
            (None, Some(d)) => Some(d.deadline),
            (None, None) => None,
        }
    }

    fn next_seq(&mut self) -> u64 {
        let seq = self.next_sequence;
        self.next_sequence += 1;
        seq
    }

    async fn handle_event(&mut self, ev: CoordinatorEvent) {
        match ev {
            CoordinatorEvent::VrChatMuteSelf(state) => self.handle_vrchat_mute_self(state).await,
            CoordinatorEvent::VrChatAvatarChanged => self.handle_vrchat_avatar_changed(),
            CoordinatorEvent::VrChatConnectionChanged(cs) => {
                self.vrchat_connection = cs;
                self.publish_snapshot();
            }
            CoordinatorEvent::DiscordVoiceSettingsUpdate { mute } => {
                self.handle_discord_voice_settings_update(mute).await
            }
            CoordinatorEvent::DiscordVoiceChannelStatus { in_voice } => {
                self.discord_in_voice = in_voice;
                self.publish_snapshot();
            }
            CoordinatorEvent::DiscordConnectionChanged(cs) => {
                self.discord_connection = cs;
                if cs != ConnectionState::Connected {
                    // §13.1: connection lost -> state becomes unknown, do not
                    // touch VRChat.
                    self.discord_state = MuteState::Unknown;
                    self.pending_discord = None;
                }
                self.publish_snapshot();
            }
            CoordinatorEvent::DiscordCommandAck { nonce, result } => {
                self.handle_discord_command_ack(nonce, result)
            }
            CoordinatorEvent::DiscordVoiceStateUnavailable(message) => {
                self.last_error = Some(message);
                self.publish_snapshot();
            }
        }
    }

    // §11.2 — VRChat-origin state changes.
    async fn handle_vrchat_mute_self(&mut self, state: MuteState) {
        let seq = self.next_seq();

        // §11.4 self-operation identification: our own toggle confirmed (or
        // superseded by a genuinely new external value) either way clears
        // the pending marker — §11.6 gives priority to whatever external
        // state was most recently observed.
        if let Some(pending) = self.pending_vrchat.take() {
            if state != pending.expected {
                tracing::debug!(
                    ?state,
                    expected = ?pending.expected,
                    "vrchat state diverged from our pending toggle; treating as a new external action"
                );
            }
            self.last_command_sequence = seq;
        }

        self.vrchat_state = state;
        self.vrchat_connection = ConnectionState::Connected;
        self.publish_snapshot();

        if !self.initial_sync_done {
            self.try_initial_sync().await;
            return;
        }

        if self.link_state != LinkState::Active {
            return;
        }

        let target = match state {
            MuteState::Muted => MuteState::Unmuted,
            MuteState::Unmuted => MuteState::Muted,
            MuteState::Unknown => return,
        };

        if self.discord_state != target {
            self.set_discord_state(target).await;
        }
    }

    fn handle_vrchat_avatar_changed(&mut self) {
        // §6.3.5: avatar change invalidates the known mute state until a
        // fresh MuteSelf arrives.
        self.vrchat_state = MuteState::Unknown;
        self.pending_vrchat = None;
        self.publish_snapshot();
    }

    // §11.3 — Discord-origin state changes.
    async fn handle_discord_voice_settings_update(&mut self, mute: MuteState) {
        let seq = self.next_seq();

        if let Some(pending) = self.pending_discord.take() {
            if mute != pending.target {
                tracing::debug!(
                    ?mute,
                    target = ?pending.target,
                    "discord state diverged from our pending command; treating as a new external action"
                );
            }
            self.last_command_sequence = seq;
        }

        self.discord_state = mute;
        self.discord_connection = ConnectionState::Connected;
        self.publish_snapshot();

        if !self.initial_sync_done {
            self.try_initial_sync().await;
            return;
        }

        if self.link_state != LinkState::Active {
            return;
        }

        // §4.2: VRChat-priority mode never reflects Discord-origin changes
        // back onto VRChat.
        if self.link_mode == LinkMode::VrchatMaster {
            return;
        }

        let target = match mute {
            MuteState::Muted => MuteState::Unmuted,
            MuteState::Unmuted => MuteState::Muted,
            MuteState::Unknown => return,
        };

        if self.vrchat_state != target {
            self.toggle_vrchat(target).await;
        }
    }

    fn handle_discord_command_ack(&mut self, nonce: Uuid, result: Result<MuteState, String>) {
        let matches_pending = self
            .pending_discord
            .as_ref()
            .is_some_and(|p| p.nonce == nonce);
        if !matches_pending {
            return;
        }
        if let Err(message) = result {
            self.pending_discord = None;
            self.link_state = LinkState::Faulted;
            self.last_error = Some(message);
            self.publish_snapshot();
        }
        // Success is confirmed by the subsequent VOICE_SETTINGS_UPDATE echo,
        // handled in handle_discord_voice_settings_update.
    }

    async fn handle_command(&mut self, cmd: CoordinatorCommand) {
        match cmd {
            CoordinatorCommand::SetLinkMode(mode) => {
                self.link_mode = mode;
                self.publish_snapshot();
            }
            CoordinatorCommand::SetPaused(paused) => {
                if paused {
                    self.link_state = LinkState::Paused;
                    self.publish_snapshot();
                } else {
                    match self.resume_policy {
                        ResumePolicy::KeepState => {
                            self.link_state = LinkState::Active;
                            self.publish_snapshot();
                        }
                        ResumePolicy::SyncFromVrchat => {
                            self.link_state = LinkState::Active;
                            self.resync_from_vrchat().await;
                        }
                    }
                }
            }
            CoordinatorCommand::ManualResync => self.manual_resync().await,
            CoordinatorCommand::Shutdown => unreachable!("handled in run()"),
        }
    }

    // §12.2 — initial sync, once both sides have a known state.
    async fn try_initial_sync(&mut self) {
        if self.initial_sync_done {
            return;
        }
        if self.vrchat_state == MuteState::Unknown || self.discord_state == MuteState::Unknown {
            return;
        }
        self.initial_sync_done = true;
        if self.link_state == LinkState::WaitingForState {
            self.link_state = LinkState::Active;
        }

        // §12.2: VRChat-priority mode always favors VRChat at startup,
        // regardless of the configured startup authority.
        let effective_authority = if self.link_mode == LinkMode::VrchatMaster {
            StartupAuthority::Vrchat
        } else {
            self.startup_authority
        };

        match effective_authority {
            StartupAuthority::Vrchat => {
                if let Some(target) = self.discord_target_for(self.vrchat_state) {
                    if self.discord_state != target {
                        self.set_discord_state(target).await;
                    }
                }
            }
            StartupAuthority::Discord => {
                if let Some(target) = self.vrchat_target_for(self.discord_state) {
                    if self.vrchat_state != target {
                        self.toggle_vrchat(target).await;
                    }
                }
            }
            StartupAuthority::None => {}
        }

        self.last_sync_at = Some(SystemTime::now());
        self.publish_snapshot();
    }

    // §12.3 — manual resync.
    async fn manual_resync(&mut self) {
        self.pending_discord = None;
        self.pending_vrchat = None;
        self.resync_from_vrchat().await;
    }

    async fn resync_from_vrchat(&mut self) {
        if self.vrchat_state == MuteState::Unknown {
            self.last_error =
                Some("VRChat state unknown; toggle the mic once to sync".to_string());
            self.publish_snapshot();
            return;
        }
        if let Some(target) = self.discord_target_for(self.vrchat_state) {
            if self.discord_state != target {
                self.set_discord_state(target).await;
            }
        }
        self.last_sync_at = Some(SystemTime::now());
        self.publish_snapshot();
    }

    fn discord_target_for(&self, vrchat: MuteState) -> Option<MuteState> {
        match vrchat {
            MuteState::Muted => Some(MuteState::Unmuted),
            MuteState::Unmuted => Some(MuteState::Muted),
            MuteState::Unknown => None,
        }
    }

    fn vrchat_target_for(&self, discord: MuteState) -> Option<MuteState> {
        match discord {
            MuteState::Muted => Some(MuteState::Unmuted),
            MuteState::Unmuted => Some(MuteState::Muted),
            MuteState::Unknown => None,
        }
    }

    async fn set_discord_state(&mut self, target: MuteState) {
        // Symmetric to `toggle_vrchat`'s Unknown guard (§11.5): if we don't
        // actually know Discord's current state (e.g. GET_VOICE_SETTINGS
        // hasn't completed yet — not authenticated, or a fetch is still
        // pending), don't blindly issue a command; it would never receive
        // a recognizable confirmation and would just time out.
        if self.discord_state == MuteState::Unknown {
            return;
        }
        // §23.1: never re-issue a command to a state we've already reached
        // or already have a matching command in flight for.
        if self.discord_state == target {
            return;
        }
        if let Some(p) = &self.pending_discord {
            if p.target == target {
                return;
            }
        }
        let nonce = Uuid::new_v4();
        let now = TokioInstant::now();
        self.pending_discord = Some(PendingDiscordCommand {
            nonce,
            target,
            issued_at: now,
            deadline: now + DISCORD_COMMAND_TIMEOUT,
        });
        let _ = self
            .discord_tx
            .send(DiscordCommand::SetMute { target, nonce })
            .await;
    }

    async fn toggle_vrchat(&mut self, expected: MuteState) {
        // §11.5: refuse to act on an unknown current state, and never issue
        // a second toggle while one is already in flight (retrying blind
        // could undo an operation that actually succeeded).
        if self.vrchat_state == MuteState::Unknown {
            return;
        }
        if self.pending_vrchat.is_some() {
            return;
        }
        let now = TokioInstant::now();
        self.pending_vrchat = Some(PendingVrChatCommand {
            previous: self.vrchat_state,
            expected,
            issued_at: now,
            deadline: now + VRCHAT_COMMAND_TIMEOUT,
        });
        let _ = self
            .vrchat_tx
            .send(VrChatCommand::ToggleVoice {
                expected_after: expected,
            })
            .await;
    }

    fn handle_pending_timeout(&mut self) {
        let now = TokioInstant::now();
        if let Some(p) = &self.pending_vrchat {
            if p.deadline <= now {
                // §11.5: do not resend — the first toggle may have actually
                // succeeded. Degrade instead and require manual resync.
                self.pending_vrchat = None;
                self.vrchat_state = MuteState::Unknown;
                self.link_state = LinkState::Faulted;
                self.last_error =
                    Some("VRChat toggle not confirmed; manual resync required".to_string());
                self.publish_snapshot();
            }
        }
        if let Some(p) = &self.pending_discord {
            if p.deadline <= now {
                self.pending_discord = None;
                self.discord_state = MuteState::Unknown;
                self.discord_connection = ConnectionState::Degraded;
                self.last_error = Some("Discord command not confirmed".to_string());
                self.publish_snapshot();
            }
        }
    }

    fn publish_snapshot(&self) {
        let snapshot = AppSnapshot {
            vrchat_connection: self.vrchat_connection,
            vrchat_mute: self.vrchat_state,
            discord_connection: self.discord_connection,
            discord_mute: self.discord_state,
            discord_in_voice_channel: self.discord_in_voice,
            link_mode: self.link_mode,
            link_state: self.link_state,
            last_sync_at: self.last_sync_at,
            last_error: self.last_error.clone(),
        };
        // A closed receiver just means nobody is listening yet/anymore.
        let _ = self.snapshot_tx.send(snapshot);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_active_coordinator(
        link_mode: LinkMode,
    ) -> (Coordinator, mpsc::Receiver<VrChatCommand>, mpsc::Receiver<DiscordCommand>) {
        let (vrchat_tx, vrchat_rx) = mpsc::channel(8);
        let (discord_tx, discord_rx) = mpsc::channel(8);
        let cfg = CoordinatorConfig {
            link_mode,
            link_enabled: true,
            startup_authority: StartupAuthority::None,
            resume_policy: ResumePolicy::SyncFromVrchat,
        };
        let (mut coordinator, _snapshot_rx) = Coordinator::new(cfg, vrchat_tx, discord_tx);
        coordinator.link_state = LinkState::Active;
        coordinator.initial_sync_done = true;
        (coordinator, vrchat_rx, discord_rx)
    }

    #[tokio::test]
    async fn vrchat_mic_on_mutes_discord() {
        let (mut c, _vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.discord_state = MuteState::Unmuted; // known, currently unmuted
        c.handle_vrchat_mute_self(MuteState::Unmuted).await; // mic ON
        let cmd = drx.try_recv().expect("expected a discord command");
        match cmd {
            DiscordCommand::SetMute { target, .. } => assert_eq!(target, MuteState::Muted),
        }
    }

    #[tokio::test]
    async fn vrchat_mic_off_unmutes_discord() {
        let (mut c, _vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.discord_state = MuteState::Muted; // known, currently muted
        c.handle_vrchat_mute_self(MuteState::Muted).await; // mic OFF
        let cmd = drx.try_recv().expect("expected a discord command");
        match cmd {
            DiscordCommand::SetMute { target, .. } => assert_eq!(target, MuteState::Unmuted),
        }
    }

    #[tokio::test]
    async fn discord_mute_turns_vrchat_mic_on() {
        let (mut c, mut vrx, _drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.vrchat_state = MuteState::Muted; // known, mic currently off
        c.handle_discord_voice_settings_update(MuteState::Muted).await;
        let cmd = vrx.try_recv().expect("expected a vrchat toggle");
        match cmd {
            VrChatCommand::ToggleVoice { expected_after } => {
                assert_eq!(expected_after, MuteState::Unmuted)
            }
        }
    }

    #[tokio::test]
    async fn discord_unmute_turns_vrchat_mic_off() {
        let (mut c, mut vrx, _drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.vrchat_state = MuteState::Unmuted; // known, mic currently on
        c.handle_discord_voice_settings_update(MuteState::Unmuted).await;
        let cmd = vrx.try_recv().expect("expected a vrchat toggle");
        match cmd {
            VrChatCommand::ToggleVoice { expected_after } => {
                assert_eq!(expected_after, MuteState::Muted)
            }
        }
    }

    #[tokio::test]
    async fn vrchat_master_mode_ignores_discord_originated_changes() {
        let (mut c, mut vrx, _drx) = make_active_coordinator(LinkMode::VrchatMaster);
        c.vrchat_state = MuteState::Muted;
        c.handle_discord_voice_settings_update(MuteState::Muted).await;
        assert!(vrx.try_recv().is_err(), "no VRChat command should be issued in VRChat-priority mode");
    }

    #[tokio::test]
    async fn unknown_vrchat_state_never_gets_toggled() {
        let (mut c, mut vrx, _drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        // vrchat_state defaults to Unknown.
        c.handle_discord_voice_settings_update(MuteState::Muted).await;
        assert!(vrx.try_recv().is_err(), "must never blindly toggle from an unknown state");
    }

    #[tokio::test]
    async fn unknown_discord_state_never_gets_a_set_command() {
        let (mut c, _vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        // discord_state defaults to Unknown (e.g. GET_VOICE_SETTINGS hasn't
        // completed yet — not authenticated).
        c.handle_vrchat_mute_self(MuteState::Unmuted).await;
        assert!(
            drx.try_recv().is_err(),
            "must never blindly command discord from an unknown discord state"
        );
    }

    #[tokio::test]
    async fn duplicate_command_to_same_target_is_suppressed() {
        let (mut c, _vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.discord_state = MuteState::Unmuted; // known, currently unmuted
        c.set_discord_state(MuteState::Muted).await;
        drx.try_recv().expect("first command should be sent");
        // Same target requested again while the first is still pending.
        c.set_discord_state(MuteState::Muted).await;
        assert!(drx.try_recv().is_err(), "duplicate command to the same state must be suppressed");
    }

    #[tokio::test]
    async fn self_issued_discord_echo_is_not_reprocessed_as_user_action() {
        let (mut c, mut vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.vrchat_state = MuteState::Unmuted; // mic on
        c.discord_state = MuteState::Unmuted; // known, currently unmuted
        c.set_discord_state(MuteState::Muted).await;
        drx.try_recv().expect("command sent");
        // Discord echoes back exactly the state we asked for.
        c.handle_discord_voice_settings_update(MuteState::Muted).await;
        assert!(c.pending_discord.is_none(), "echo of our own command must clear the pending marker");
        assert!(vrx.try_recv().is_err(), "the echo of our own command must not trigger a new VRChat toggle");
    }

    #[tokio::test]
    async fn conflicting_external_state_wins_over_in_flight_command() {
        let (mut c, mut vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.vrchat_state = MuteState::Unmuted;
        c.discord_state = MuteState::Unmuted; // known, currently unmuted
        c.set_discord_state(MuteState::Muted).await;
        drx.try_recv().expect("command sent");
        // Before our command is confirmed, Discord reports a different
        // (user-driven) state — this must win, clearing the stale pending
        // marker and reconciling from the new state instead.
        c.handle_discord_voice_settings_update(MuteState::Unmuted).await;
        assert!(c.pending_discord.is_none());
        let cmd = vrx.try_recv().expect("should reconcile vrchat towards the new external state");
        match cmd {
            VrChatCommand::ToggleVoice { expected_after } => {
                assert_eq!(expected_after, MuteState::Muted)
            }
        }
    }

    #[tokio::test]
    async fn paused_link_state_ignores_state_changes() {
        let (mut c, _vrx, mut drx) = make_active_coordinator(LinkMode::InverseBidirectional);
        c.link_state = LinkState::Paused;
        c.handle_vrchat_mute_self(MuteState::Unmuted).await;
        assert!(drx.try_recv().is_err(), "paused link must not issue commands");
    }
}
