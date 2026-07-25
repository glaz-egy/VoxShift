//! OSC avatar-config diagnostics/repair — 設計書.md §6.3.5.
//!
//! Explicitly out of scope for this phase: §25 lists "OSC設定診断" under
//! Phase 4 (stabilization), so this is a stub the future implementation
//! will fill in without changing the surrounding wiring.

pub struct AvatarConfigDiagnostics;

impl AvatarConfigDiagnostics {
    // TODO(phase4): §6.3.5 — detect the current avatar's OSC config file,
    // diagnose whether `MuteSelf` output is present, and (only on explicit
    // user action from the diagnostics screen) back up + patch it in.
}
