//! Global hotkey registration and polling.
//!
//! Wraps the `global-hotkey` crate to provide a simple interface for
//! registering a single toggle hotkey (Ctrl+Shift+Space) and polling
//! whether it was triggered.

use std::sync::mpsc::{Receiver, channel};

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey, Modifiers},
};

/// Decide whether one hotkey event is a trigger.
///
/// The crate reports a `Pressed` and a `Released` event for every key press
/// (and, on some platforms, further `Pressed` events while the key is held).
/// Only the first `Pressed` after a release triggers; `held` carries that
/// state between calls. Events for other hotkey ids are ignored entirely.
pub fn is_trigger(hotkey_id: u32, event_id: u32, state: HotKeyState, held: &mut bool) -> bool {
    if event_id != hotkey_id {
        return false;
    }
    match state {
        HotKeyState::Pressed => !std::mem::replace(held, true),
        HotKeyState::Released => {
            *held = false;
            false
        }
    }
}

/// Manages a single global hotkey registration.
pub struct HotkeyManager {
    manager: GlobalHotKeyManager,
    hotkey: HotKey,
    /// Events forwarded by the global handler (see [`HotkeyManager::new`]).
    events: Receiver<GlobalHotKeyEvent>,
    /// The key is currently down (see [`is_trigger`]).
    held: bool,
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

        Ok(Self {
            manager,
            hotkey,
            events,
            held: false,
        })
    }

    /// Check if the hotkey was triggered since the last call (non-blocking).
    ///
    /// Drains every queued event, so a press and its release are consumed in
    /// the same call.
    pub fn was_triggered(&mut self) -> bool {
        let mut triggered = false;
        while let Ok(event) = self.events.try_recv() {
            triggered |= is_trigger(self.hotkey.id(), event.id, event.state, &mut self.held);
        }
        triggered
    }
}

impl Drop for HotkeyManager {
    fn drop(&mut self) {
        let _ = self.manager.unregister(self.hotkey);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: u32 = 7;

    fn run(events: &[(u32, HotKeyState)]) -> usize {
        let mut held = false;
        events
            .iter()
            .filter(|(id, st)| is_trigger(ID, *id, *st, &mut held))
            .count()
    }

    #[test]
    fn press_triggers_once() {
        assert_eq!(run(&[(ID, HotKeyState::Pressed)]), 1);
    }

    #[test]
    fn release_does_not_trigger() {
        assert_eq!(run(&[(ID, HotKeyState::Released)]), 0);
    }

    #[test]
    fn press_release_pair_triggers_once() {
        assert_eq!(
            run(&[(ID, HotKeyState::Pressed), (ID, HotKeyState::Released)]),
            1
        );
    }

    #[test]
    fn press_release_press_triggers_twice() {
        let events = [
            (ID, HotKeyState::Pressed),
            (ID, HotKeyState::Released),
            (ID, HotKeyState::Pressed),
        ];
        assert_eq!(run(&events), 2);
    }

    #[test]
    fn key_repeat_while_held_is_ignored() {
        let events = [
            (ID, HotKeyState::Pressed),
            (ID, HotKeyState::Pressed),
            (ID, HotKeyState::Pressed),
            (ID, HotKeyState::Released),
        ];
        assert_eq!(run(&events), 1);
    }

    #[test]
    fn other_hotkey_id_does_not_trigger() {
        assert_eq!(
            run(&[
                (ID + 1, HotKeyState::Pressed),
                (ID + 1, HotKeyState::Released)
            ]),
            0
        );
    }
}
