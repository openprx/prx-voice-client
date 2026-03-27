//! Global hotkey registration and polling.
//!
//! Wraps the `global-hotkey` crate to provide a simple interface for
//! registering a single toggle hotkey (Ctrl+Shift+Space) and polling
//! whether it was triggered.

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager,
    hotkey::{Code, HotKey, Modifiers},
};

/// Manages a single global hotkey registration.
pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
}

impl HotkeyManager {
    /// Create and register the default hotkey (Ctrl+Shift+Space).
    pub fn new() -> Result<Self, String> {
        let manager = GlobalHotKeyManager::new()
            .map_err(|e| format!("failed to create hotkey manager: {e}"))?;

        let hotkey = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::Space);

        manager
            .register(hotkey)
            .map_err(|e| format!("failed to register hotkey: {e}"))?;

        Ok(Self { manager, hotkey })
    }

    /// Check if the hotkey was triggered since the last call (non-blocking).
    pub fn was_triggered(&self) -> bool {
        if let Ok(event) = GlobalHotKeyEvent::receiver().try_recv() {
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
