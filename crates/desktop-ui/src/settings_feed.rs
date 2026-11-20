//! The bridge that carries [`SettingsData`] from the app's probe threads to the settings window, and wakes the UI when it changes.
//!
//! Follows the shared-state pattern (copy-in under a short lock, a wake-up after the lock is
//! released, a copy-out once per frame). It is kept apart from `SharedAppState` because the
//! settings window is the only reader, and so that the settings work does not touch the call
//! parts of that file.

use std::sync::Arc;

use parking_lot::Mutex;

use crate::settings_data::SettingsData;
use crate::shared::RepaintSignal;

#[derive(Debug, Default)]
struct Inner {
    version: u64,
    data: SettingsData,
}

/// Shared slot for the settings window's data. Clones share one slot.
#[derive(Debug, Clone, Default)]
pub struct SettingsFeed {
    inner: Arc<Mutex<Inner>>,
    wake: RepaintSignal,
}

impl SettingsFeed {
    /// A feed that wakes the UI through `wake` (clone of the shared state's signal).
    pub fn new(wake: RepaintSignal) -> Self {
        Self {
            inner: Arc::default(),
            wake,
        }
    }

    /// Set the wake-up signal (the app does this once the shared state exists).
    pub fn set_wake(&mut self, wake: RepaintSignal) {
        self.wake = wake;
    }

    /// Change the data under the lock; wakes the UI only when `f` changed something.
    pub fn update(&self, f: impl FnOnce(&mut SettingsData)) {
        let changed = {
            let mut inner = self.inner.lock();
            let mut next = inner.data.clone();
            f(&mut next);
            let changed = next != inner.data;
            if changed {
                inner.data = next;
                inner.version += 1;
            }
            changed
        };
        if changed {
            self.wake.request();
        }
    }

    /// The data when it changed since `seen` (which is then updated).
    pub fn take_if_changed(&self, seen: &mut u64) -> Option<SettingsData> {
        let inner = self.inner.lock();
        if inner.version == *seen {
            return None;
        }
        *seen = inner.version;
        Some(inner.data.clone())
    }

    /// A copy of the current data.
    pub fn snapshot(&self) -> SettingsData {
        self.inner.lock().data.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_wakes_the_ui_once_and_is_seen_once() {
        let wake = RepaintSignal::default();
        let feed = SettingsFeed::new(wake.clone());
        let mut seen = 0;
        assert!(feed.take_if_changed(&mut seen).is_none());
        feed.update(|d| d.loaded = true);
        assert_eq!(wake.request_count(), 1);
        assert!(feed.take_if_changed(&mut seen).is_some_and(|d| d.loaded));
        assert!(feed.take_if_changed(&mut seen).is_none());
    }

    #[test]
    fn an_unchanged_update_does_not_wake_the_ui() {
        let wake = RepaintSignal::default();
        let feed = SettingsFeed::new(wake.clone());
        feed.update(|d| d.loaded = true);
        let before = wake.request_count();
        feed.update(|d| d.loaded = true);
        assert_eq!(wake.request_count(), before);
    }
}
