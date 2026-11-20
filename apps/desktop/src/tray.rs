//! System tray icon and menu management.
//!
//! Wraps the `tray-icon` crate to show a status-area icon with a context
//! menu offering basic session control and window management actions.

use std::sync::mpsc::{Receiver, channel};

use tray_icon::{
    Icon, TrayIconBuilder,
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
};

/// Actions the user can trigger from the tray context menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrayAction {
    /// Bring the main window to the foreground.
    Show,
    /// Start a new voice session.
    StartSession,
    /// Stop the active voice session.
    StopSession,
    /// Open the settings page.
    Settings,
    /// Quit the application.
    Quit,
}

/// Manages the system tray icon and its context menu.
pub struct TrayManager {
    /// Prevent the tray icon from being dropped (which removes it).
    _tray: tray_icon::TrayIcon,
    show_item: MenuItem,
    start_item: MenuItem,
    stop_item: MenuItem,
    settings_item: MenuItem,
    quit_item: MenuItem,
    show_id: String,
    start_id: String,
    stop_id: String,
    settings_id: String,
    quit_id: String,
    /// Menu events forwarded by the global handler (see [`TrayManager::new`]).
    events: Receiver<MenuEvent>,
}

impl TrayManager {
    /// Build the tray icon with a context menu.
    ///
    /// Menu events do not wake egui, so an event handler forwards each one to
    /// a private channel and requests a repaint of `ctx`; that runs
    /// `App::logic` even while the window is hidden. Once a handler is set
    /// the crate's own `MenuEvent::receiver()` gets nothing.
    pub fn new(ctx: egui::Context) -> Result<Self, String> {
        let (tx, events) = channel();
        MenuEvent::set_event_handler(Some(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        }));

        let show_item = MenuItem::new(prxvc_i18n::t("tray.show"), true, None);
        let start_item = MenuItem::new(prxvc_i18n::t("session.start"), true, None);
        let stop_item = MenuItem::new(prxvc_i18n::t("session.stop"), true, None);
        let settings_item = MenuItem::new(prxvc_i18n::t("settings.title"), true, None);
        let quit_item = MenuItem::new(prxvc_i18n::t("tray.quit"), true, None);

        // Store IDs before items are moved into the menu.
        let show_id = show_item.id().0.clone();
        let start_id = start_item.id().0.clone();
        let stop_id = stop_item.id().0.clone();
        let settings_id = settings_item.id().0.clone();
        let quit_id = quit_item.id().0.clone();

        let menu = Menu::new();
        menu.append(&show_item)
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&start_item)
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&stop_item)
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&settings_item)
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&PredefinedMenuItem::separator())
            .map_err(|e| format!("menu append failed: {e}"))?;
        menu.append(&quit_item)
            .map_err(|e| format!("menu append failed: {e}"))?;

        let icon_rgba = create_prx_icon();
        let icon = Icon::from_rgba(icon_rgba, TRAY_ICON_SIZE, TRAY_ICON_SIZE)
            .map_err(|e| format!("failed to create icon: {e}"))?;

        let mut builder = TrayIconBuilder::new()
            .with_tooltip(prxvc_i18n::t("app.name"))
            .with_menu(Box::new(menu))
            .with_icon(icon);

        // On macOS, mark as template image so the system auto-adapts
        // to light/dark menu bar (black ↔ white).
        #[cfg(target_os = "macos")]
        {
            builder = builder.with_icon_as_template(true);
        }

        let tray = builder
            .build()
            .map_err(|e| format!("failed to create tray: {e}"))?;

        Ok(Self {
            _tray: tray,
            show_item,
            start_item,
            stop_item,
            settings_item,
            quit_item,
            show_id,
            start_id,
            stop_id,
            settings_id,
            quit_id,
            events,
        })
    }

    /// Update menu item labels to match current i18n locale.
    pub fn refresh_labels(&self) {
        self.show_item.set_text(prxvc_i18n::t("tray.show"));
        self.start_item.set_text(prxvc_i18n::t("session.start"));
        self.stop_item.set_text(prxvc_i18n::t("session.stop"));
        self.settings_item.set_text(prxvc_i18n::t("settings.title"));
        self.quit_item.set_text(prxvc_i18n::t("tray.quit"));
    }

    /// Poll for a tray menu action (non-blocking).
    pub fn poll_action(&self) -> Option<TrayAction> {
        if let Ok(event) = self.events.try_recv() {
            let id = &event.id.0;
            if id == &self.show_id {
                return Some(TrayAction::Show);
            }
            if id == &self.start_id {
                return Some(TrayAction::StartSession);
            }
            if id == &self.stop_id {
                return Some(TrayAction::StopSession);
            }
            if id == &self.settings_id {
                return Some(TrayAction::Settings);
            }
            if id == &self.quit_id {
                return Some(TrayAction::Quit);
            }
        }
        None
    }
}

/// Generate a 22x22 PRX hexagon logo as RGBA pixels.
///
/// Uses **template image style** for macOS compatibility:
/// pure black shape with varying alpha. macOS automatically inverts
/// the icon in dark mode (black → white). On other platforms the
/// black hexagon outline is clearly visible against any tray background.
fn create_prx_icon() -> Vec<u8> {
    let size: usize = 22;
    let mut pixels = vec![0u8; size * size * 4]; // RGBA, fully transparent

    let cx = size as f32 / 2.0;
    let cy = size as f32 / 2.0;
    let r: f32 = 9.5; // hexagon outer radius

    let stroke_outer = r;
    let stroke_inner = r - 1.8; // stroke width ~1.8px

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let ax = dx.abs();
            let ay = dy.abs();

            // Pointy-top hex containment: max(|dx|, |dx|*0.5 + |dy|*√3/2) ≤ r*√3/2
            let in_outer = ax <= stroke_outer * 0.866
                && (ax * 0.5 + ay * 0.866) <= stroke_outer * 0.866;
            let in_inner = ax <= stroke_inner * 0.866
                && (ax * 0.5 + ay * 0.866) <= stroke_inner * 0.866;

            let idx = (y * size + x) * 4;

            if in_outer && !in_inner {
                // Hexagon outline — black with full opacity (template style)
                pixels[idx] = 0;
                pixels[idx + 1] = 0;
                pixels[idx + 2] = 0;
                pixels[idx + 3] = 255; // full opacity for crisp rendering
            }
            // Interior and exterior remain fully transparent
        }
    }

    pixels
}

/// Icon size for the tray (matches the generated pixel buffer).
const TRAY_ICON_SIZE: u32 = 22;
