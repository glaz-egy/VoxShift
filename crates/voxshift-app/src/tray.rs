//! System tray icon and menu — 設計書.md §7.6/§7.7.
//!
//! Kept intentionally simple for this pass: five static menu items (rather
//! than a single label that toggles "Pause"/"Resume") since mutating a
//! `tray-icon` menu must happen on the same (main) thread it was created
//! on, and the live app state lives on the worker thread. A follow-up could
//! thread a request back to the main thread to relabel the pause item.

use crossbeam_channel::Receiver;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder, TrayIconEvent};

pub struct TrayHandles {
    // Never read directly, but must stay alive: dropping a `TrayIcon`
    // removes it from the system tray.
    #[allow(dead_code)]
    pub icon: TrayIcon,
    pub show_id: MenuId,
    pub pause_id: MenuId,
    pub resume_id: MenuId,
    pub resync_id: MenuId,
    pub quit_id: MenuId,
}

fn placeholder_icon() -> Icon {
    // A flat teal 16x16 square — a real multi-resolution .ico per §7.7's
    // state table is a follow-up; this keeps the tray icon present and
    // buildable without bundled asset files.
    const SIZE: u32 = 16;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.extend_from_slice(&[0x35, 0xD8, 0xC6, 0xFF]);
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid placeholder icon buffer")
}

pub fn build() -> Result<TrayHandles, tray_icon::Error> {
    let menu = Menu::new();
    let show_item = MenuItem::new("Show VoxShift", true, None);
    let pause_item = MenuItem::new("Pause", true, None);
    let resume_item = MenuItem::new("Resume", true, None);
    let resync_item = MenuItem::new("Resync", true, None);
    let quit_item = MenuItem::new("Quit", true, None);

    menu.append(&show_item).ok();
    menu.append(&pause_item).ok();
    menu.append(&resume_item).ok();
    menu.append(&resync_item).ok();
    menu.append(&quit_item).ok();

    let show_id = show_item.id().clone();
    let pause_id = pause_item.id().clone();
    let resume_id = resume_item.id().clone();
    let resync_id = resync_item.id().clone();
    let quit_id = quit_item.id().clone();

    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("VoxShift")
        .with_icon(placeholder_icon())
        .build()?;

    Ok(TrayHandles {
        icon,
        show_id,
        pause_id,
        resume_id,
        resync_id,
        quit_id,
    })
}

pub fn menu_event_receiver() -> &'static Receiver<MenuEvent> {
    MenuEvent::receiver()
}

pub fn tray_event_receiver() -> &'static Receiver<TrayIconEvent> {
    TrayIconEvent::receiver()
}
