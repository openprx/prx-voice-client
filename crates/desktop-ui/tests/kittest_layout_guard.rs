//! The layout guard (`common::layout_guard`) applied to the settings window sections and the orb
//! window: no two widgets overlap and none lies outside the window.

mod common;

use common::layout_guard;
use egui::{Rect, Theme, vec2};
use prxvc_desktop_ui::{DesktopUiState, SettingsSection};
use prxvc_i18n::Locale;

#[test]
fn the_settings_sections_have_no_overlapping_or_escaping_widgets() {
    let _guard = common::lock_locale();
    let mut failures = Vec::new();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        for width in [520.0, 760.0, 1200.0] {
            for section in SettingsSection::ALL {
                let mut h =
                    common::window_harness(width, Theme::Light, common::WindowState::new(section));
                h.run_steps(3);
                let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 2600.0));
                let found = layout_guard::problems(&h, window);
                if !found.is_empty() {
                    failures.push(format!(
                        "{section:?} {locale:?} {width}: {}",
                        found.join("\n  ")
                    ));
                }
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
    assert!(
        failures.is_empty(),
        "{} failing case(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
fn the_orb_window_has_no_overlapping_or_escaping_widgets() {
    let _guard = common::lock_locale();
    let mut failures = Vec::new();
    for locale in [Locale::En, Locale::ZhCn] {
        prxvc_i18n::set_locale(locale.clone());
        for (width, height) in [(200.0, 200.0), (320.0, 360.0)] {
            let mut state = DesktopUiState::new();
            state.idle_animation = false;
            let mut h = common::builder(vec2(width, height), Theme::Light).build_ui_state(
                |ui, st: &mut DesktopUiState| prxvc_desktop_ui::render_app(ui, st),
                state,
            );
            h.run_steps(3);
            let window = Rect::from_min_size(egui::Pos2::ZERO, vec2(width, height));
            let found = layout_guard::problems(&h, window);
            if !found.is_empty() {
                failures.push(format!(
                    "orb {locale:?} {width}x{height}: {}",
                    found.join("\n  ")
                ));
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
    assert!(
        failures.is_empty(),
        "{} failing case(s)\n{}",
        failures.len(),
        failures.join("\n")
    );
}
