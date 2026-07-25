// Without this, Windows builds a console-subsystem executable, which pops
// up a command-prompt window alongside the tray app on every launch. This
// doesn't affect logging: redirected stdout/stderr (e.g. `Start-Process
// -RedirectStandardOutput`) are still delivered regardless of subsystem —
// only automatic console-window allocation on a plain double-click is
// suppressed.
#![windows_subsystem = "windows"]

mod application;
mod tray;
mod worker;

fn main() {
    // §20: refuse to start a second instance. The Named Pipe "show existing
    // window" control channel is a follow-up — for now a second launch
    // just exits.
    let Some(_single_instance_guard) = voxshift_platform_windows::single_instance::acquire() else {
        eprintln!("VoxShift is already running.");
        return;
    };

    let background = std::env::args().any(|arg| arg == "--background");
    application::run(background);
}
