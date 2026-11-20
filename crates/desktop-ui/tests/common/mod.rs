//! Helpers shared by the egui_kittest integration tests.

use std::sync::{Mutex, MutexGuard};

use egui_kittest::{Harness, HarnessBuilder};
use prxvc_desktop_ui::{fonts, theme};

/// The i18n locale is global; tests that change it must not interleave.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

pub fn lock_locale() -> MutexGuard<'static, ()> {
    LOCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A harness builder with the bundled fonts and the RF theme installed as the
/// application has them, and the given system scheme.
///
/// No system CJK face is passed, so the run is deterministic: the embedded
/// Noto Sans SC subset covers the UI's Chinese strings.
pub fn builder<S>(size: egui::Vec2, scheme: egui::Theme) -> HarnessBuilder<S> {
    Harness::builder()
        .with_size(size)
        .with_theme(scheme)
        .with_context_setup(|ctx| {
            fonts::install(ctx, None);
            theme::install(ctx);
        })
}
