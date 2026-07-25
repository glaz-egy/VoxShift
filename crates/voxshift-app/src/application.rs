//! Application entry point — GUI wiring (Phase 3). §12.1 startup order,
//! minus the platform-specific pieces still deferred (Mica backdrop, real
//! OS theme detection, autostart).

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use raw_window_handle::HasWindowHandle;
use slint::ComponentHandle;
use tokio::sync::watch;
use tray_icon::TrayIconEvent;

use voxshift_core::command::CoordinatorCommand;
use voxshift_core::state::{AppSnapshot, LinkMode as CoreLinkMode};
use voxshift_storage::config::Theme as ConfigTheme;
use voxshift_ui::theme::{ConfiguredTheme, OsTheme};
use voxshift_ui::{AppLinkMode, AppState, MainWindow, Tokens};

pub fn run(background: bool) {
    let config = voxshift_storage::config::load();
    let _log_ring =
        voxshift_storage::logging::init(&config.logging.level, config.logging.file_logging);

    tracing::info!(config_path = ?voxshift_storage::config::config_path(), "VoxShift starting");

    let worker = match crate::worker::spawn_worker(config.clone()) {
        Ok(w) => w,
        Err(err) => {
            tracing::error!(error = %err, "failed to start the voxshift worker thread");
            return;
        }
    };

    let ui = match MainWindow::new() {
        Ok(ui) => ui,
        Err(err) => {
            tracing::error!(error = %err, "failed to create the main window");
            return;
        }
    };

    apply_initial_theme(&ui, &config);
    apply_initial_language(&ui, &config);
    wire_callbacks(&ui, worker.command_tx.clone(), worker.authorize_tx.clone());

    worker
        .runtime
        .spawn(bridge_snapshot_to_ui(worker.snapshot_rx.clone(), ui.as_weak()));

    let tray = match crate::tray::build() {
        Ok(t) => Some(t),
        Err(err) => {
            tracing::warn!(error = %err, "failed to create tray icon; continuing without one");
            None
        }
    };

    let ui_weak_for_close = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = ui_weak_for_close.upgrade() {
            ui.hide().ok();
            ui.global::<AppState>().set_window_visible(false);
        }
        slint::CloseRequestResponse::HideWindow
    });

    // tray-icon's hidden message-only window is serviced by the same
    // native message loop Slint's winit backend already pumps on this
    // thread — polling its event channels from a Slint timer keeps
    // everything on one thread without a second native event loop.
    let _tray_poll_timer = spawn_tray_event_poller(&ui, worker.command_tx.clone(), tray);

    if !background {
        show_window(&ui.as_weak());
    }

    // Not `ui.run()`: its generated default quits the whole event loop once
    // the last window closes, which — since `on_close_requested` above only
    // hides the window rather than destroying it — would still tear down
    // the tray-resident app the moment the user clicks the window's close
    // button. `run_event_loop_until_quit` keeps running with zero visible
    // windows; only the tray menu's "Quit" (`std::process::exit`) ends it.
    slint::run_event_loop_until_quit().ok();
}

fn apply_initial_theme(ui: &MainWindow, config: &voxshift_storage::config::AppConfig) {
    let configured = match config.theme {
        ConfigTheme::Dark => ConfiguredTheme::Dark,
        ConfigTheme::Light => ConfiguredTheme::Light,
        ConfigTheme::System => ConfiguredTheme::System,
    };
    // TODO(phase4): real OS theme detection + live-change watching via
    // voxshift-platform-windows::dwm; dark is a reasonable default until
    // then since the app's own palette is dark-first.
    let resolved = voxshift_ui::theme::resolve(configured, OsTheme::Dark);
    voxshift_ui::theme::apply(&ui.global::<Tokens>(), resolved, config.accessibility.text_scale);
    ui.global::<AppState>().set_reduced_motion(config.reduced_motion);
}

fn apply_initial_language(ui: &MainWindow, config: &voxshift_storage::config::AppConfig) {
    let table = voxshift_ui::i18n::load_language(&config.language);
    voxshift_ui::i18n::apply(&ui.global::<voxshift_ui::Strings>(), &table);

    let languages = voxshift_ui::i18n::available_languages();
    let codes: Vec<slint::SharedString> = languages.iter().map(|(code, _)| code.as_str().into()).collect();
    let names: Vec<slint::SharedString> = languages.iter().map(|(_, name)| name.as_str().into()).collect();

    let app_state = ui.global::<AppState>();
    app_state.set_current_language_code(config.language.clone().into());
    app_state.set_available_language_codes(slint::ModelRc::new(slint::VecModel::from(codes)));
    app_state.set_available_language_names(slint::ModelRc::new(slint::VecModel::from(names)));
}

fn wire_callbacks(
    ui: &MainWindow,
    cmd_tx: tokio::sync::mpsc::Sender<CoordinatorCommand>,
    authorize_tx: tokio::sync::mpsc::Sender<()>,
) {
    let app_state = ui.global::<AppState>();

    let tx = cmd_tx.clone();
    app_state.on_pause_clicked(move || {
        let _ = tx.try_send(CoordinatorCommand::SetPaused(true));
    });

    let tx = cmd_tx.clone();
    app_state.on_resume_clicked(move || {
        let _ = tx.try_send(CoordinatorCommand::SetPaused(false));
    });

    let tx = cmd_tx.clone();
    app_state.on_set_link_mode(move |mode| {
        let core_mode = match mode {
            AppLinkMode::InverseBidirectional => CoreLinkMode::InverseBidirectional,
            AppLinkMode::VrchatMaster => CoreLinkMode::VrchatMaster,
        };
        let _ = tx.try_send(CoordinatorCommand::SetLinkMode(core_mode));
    });

    let tx = cmd_tx.clone();
    app_state.on_manual_resync(move || {
        let _ = tx.try_send(CoordinatorCommand::ManualResync);
    });

    let ui_weak_for_auth = ui.as_weak();
    app_state.on_reauthorize_discord(move || {
        let message = match authorize_tx.try_send(()) {
            Ok(()) => "Requested — check Discord for the authorization prompt.",
            Err(_) => "Discord isn't connected right now; try again once it is.",
        };
        if let Some(ui) = ui_weak_for_auth.upgrade() {
            ui.global::<AppState>().set_discord_auth_message(message.into());
        }
    });

    app_state.on_copy_diagnostics(move || {
        tracing::info!("copy-diagnostics requested (clipboard export is a follow-up)");
    });

    let ui_weak_for_lang = ui.as_weak();
    app_state.on_set_language(move |code| {
        let Some(ui) = ui_weak_for_lang.upgrade() else { return };
        let table = voxshift_ui::i18n::load_language(&code);
        voxshift_ui::i18n::apply(&ui.global::<voxshift_ui::Strings>(), &table);
        ui.global::<AppState>().set_current_language_code(code.clone());

        let mut config = voxshift_storage::config::load();
        config.language = code.to_string();
        if let Err(err) = voxshift_storage::config::save(&config) {
            tracing::error!(error = %err, "failed to save language setting");
        }
    });
}

async fn bridge_snapshot_to_ui(mut rx: watch::Receiver<AppSnapshot>, ui_weak: slint::Weak<MainWindow>) {
    loop {
        if rx.changed().await.is_err() {
            return;
        }
        let snapshot = rx.borrow_and_update().clone();
        let ui_weak = ui_weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui_weak.upgrade() {
                apply_snapshot(&ui, &snapshot);
            }
        });
    }
}

fn apply_snapshot(ui: &MainWindow, snapshot: &AppSnapshot) {
    let ui_snap = voxshift_ui::view_model::from_snapshot(snapshot);
    let app_state = ui.global::<AppState>();
    app_state.set_vrchat_connection(ui_snap.vrchat_connection);
    app_state.set_vrchat_mute(ui_snap.vrchat_mute);
    app_state.set_discord_connection(ui_snap.discord_connection);
    app_state.set_discord_mute(ui_snap.discord_mute);
    app_state.set_discord_in_voice_channel(ui_snap.discord_in_voice_channel);
    app_state.set_link_mode(ui_snap.link_mode);
    app_state.set_link_state(ui_snap.link_state);
    app_state.set_last_sync_label(ui_snap.last_sync_label.into());
    app_state.set_last_error(ui_snap.last_error.into());
}

fn spawn_tray_event_poller(
    ui: &MainWindow,
    cmd_tx: tokio::sync::mpsc::Sender<CoordinatorCommand>,
    tray: Option<crate::tray::TrayHandles>,
) -> slint::Timer {
    let poll_ui_weak = ui.as_weak();
    let timer = slint::Timer::default();
    timer.start(slint::TimerMode::Repeated, Duration::from_millis(100), move || {
        while let Ok(event) = crate::tray::tray_event_receiver().try_recv() {
            if let TrayIconEvent::Click { button: tray_icon::MouseButton::Left, .. } = event {
                show_window(&poll_ui_weak);
            }
        }

        let Some(tray) = &tray else { return };
        while let Ok(event) = crate::tray::menu_event_receiver().try_recv() {
            if event.id == tray.show_id {
                show_window(&poll_ui_weak);
            } else if event.id == tray.pause_id {
                let _ = cmd_tx.try_send(CoordinatorCommand::SetPaused(true));
            } else if event.id == tray.resume_id {
                let _ = cmd_tx.try_send(CoordinatorCommand::SetPaused(false));
            } else if event.id == tray.resync_id {
                let _ = cmd_tx.try_send(CoordinatorCommand::ManualResync);
            } else if event.id == tray.quit_id {
                let _ = cmd_tx.try_send(CoordinatorCommand::Shutdown);
                std::process::exit(0);
            }
        }
    });
    timer
}

fn show_window(ui_weak: &slint::Weak<MainWindow>) {
    if let Some(ui) = ui_weak.upgrade() {
        ui.show().ok();
        ui.global::<AppState>().set_window_visible(true);
        schedule_maximize_button_removal(ui_weak.clone());
    }
}

/// Removes the window's maximize button — a native Win32 title-bar tweak
/// with no `.slint` markup equivalent, so it needs the raw HWND. Only
/// applied once (it survives hide/show, since hiding doesn't destroy the
/// native window).
static MAXIMIZE_BUTTON_REMOVED: AtomicBool = AtomicBool::new(false);

fn schedule_maximize_button_removal(ui_weak: slint::Weak<MainWindow>) {
    if MAXIMIZE_BUTTON_REMOVED.load(Ordering::Relaxed) {
        return;
    }
    // The native HWND doesn't exist yet the instant `show()` returns — per
    // Slint's docs it's only created by the window manager during a
    // subsequent event-loop iteration — so this is deferred rather than
    // called inline right after `show()`.
    slint::Timer::single_shot(Duration::from_millis(0), move || {
        let Some(ui) = ui_weak.upgrade() else { return };
        let slint_window_handle = ui.window().window_handle();
        let Ok(handle) = slint_window_handle.window_handle() else {
            return;
        };
        if let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() {
            voxshift_platform_windows::window_style::disable_maximize_button(win32.hwnd.get());
            MAXIMIZE_BUTTON_REMOVED.store(true, Ordering::Relaxed);
        }
    });
}
