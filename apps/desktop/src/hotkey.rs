//! Global hotkey registration and polling.
//!
//! Wraps the `global-hotkey` crate to provide a simple interface for
//! registering a single toggle hotkey (Ctrl+Shift+Space) and polling
//! whether it was triggered.

use std::sync::mpsc::{Receiver, channel};

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};

/// Manages a single global hotkey registration.
pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    /// Events forwarded by the global handler (see [`HotkeyManager::new`]).
    events: Receiver<GlobalHotKeyEvent>,
}

impl HotkeyManager {
    /// Create and register the default hotkey (Ctrl+Shift+Space).
    ///
    /// Events arrive on a platform thread and do not wake egui, so an event
    /// handler forwards each one to a private channel and requests a repaint
    /// of `ctx`; that runs `App::logic` even while the window is hidden. Once
    /// a handler is set the crate's own `receiver()` gets nothing.
    pub fn new(ctx: egui::Context) -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| format!("failed to create hotkey manager: {e}"))?;

        let (tx, events) = channel();
        GlobalHotKeyEvent::set_event_handler(Some(move |event| {
            let _ = tx.send(event);
            ctx.request_repaint();
        }));

        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);

        manager
            .register(hotkey)
            .map_err(|e| format!("failed to register hotkey: {e}"))?;

        Ok(Self { manager, hotkey, events })
    }

    /// Check if the hotkey was triggered since the last call (non-blocking).
    pub fn was_triggered(&self) -> bool {
        if let Ok(event) = self.events.try_recv() {
            if event.id == self.hotkey.id() {
                return true;
            }
        }
        false
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.hotkey);
    }
}
