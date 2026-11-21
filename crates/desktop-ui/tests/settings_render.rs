//! Headless render tests of every settings section: both colour schemes, both languages, and
//! windows of 480, 528 (the standard's 480 px content), 900 and 1200 px.
//!
//! For every combination the page completes, every interactive control has an accessible name
//! (`check_accessibility`), and no control reaches outside its column: below 900 px that is the
//! window less its padding, from 900 px the content column right of the section list.

mod common;

use common::{WindowState, window_harness};
use egui::{Role, Theme};
use egui_kittest::kittest::{By, NodeT as _, Queryable as _};
use prxvc_desktop_ui::settings_data::*;
use prxvc_desktop_ui::settings_window::{COLUMN_MAX, NAV_WIDTH, WIDE_MIN_WIDTH};
use prxvc_desktop_ui::tool_window::WINDOW_PADDING;
use prxvc_desktop_ui::{SettingsSection, UiSettings};
use prxvc_i18n::Locale;

const WIDTHS: [f32; 4] = [480.0, 528.0, 900.0, 1200.0];
const SCHEMES: [Theme; 2] = [Theme::Light, Theme::Dark];
const LOCALES: [Locale; 2] = [Locale::En, Locale::ZhCn];
const CONTROL_ROLES: [Role; 6] = [
    Role::Switch,
    Role::Button,
    Role::ComboBox,
    Role::RadioButton,
    Role::TextInput,
    Role::MenuItem,
];

/// The page as the user sees it first (`rich_data`), and a busy variant that opens every
/// optional part of it: the assistant's backend problem, offline mode with hosts, the driver with a failed
/// operation, a permission that waits, pipeline problems.
fn variants(section: SettingsSection) -> Vec<WindowState> {
    let plain = WindowState::new(section);
    let mut busy = WindowState::new(section);
    busy.settings = UiSettings {
        offline: true,
        allow_hosts: vec![
            "192.168.1.20".into(),
            "a-rather-long-host-name.example.internal".into(),
        ],
        input_device: "a-microphone-that-is-no-longer-connected-with-a-long-name".into(),
        models_endpoint: "https://a-very-long-mirror-address.example.org/huggingface".into(),
        call_self_language: "zh".into(),
        call_peer_language: "en".into(),
        dialogue_provider: "openprx".into(),
        assistant_voice: "clone".into(),
        assistant_language: "zh-CN".into(),
        hotkey_push_to_talk: "F5".into(),
        ..UiSettings::default()
    };
    busy.data.permissions.driver.state = DriverStateView::UpdateAvailable;
    busy.data.permissions.driver.operation = DriverOperationView::Failed {
        operation: DriverOperationKind::Install,
        code: "device_did_not_appear".into(),
    };
    busy.data.permissions.microphone = PermissionView::NotAllowed;
    busy.data.permissions.microphone_request = Some(RequestOutcome::NotInBundle);
    busy.data.permissions.outside_bundle = true;
    busy.data.audio.input_choice = Some(InputChoiceView::Chosen {
        name: "MacBook Pro Microphone".into(),
        reason: InputReason::ConfiguredMissing,
    });
    busy.data.voice_profile = None;
    busy.data.pipeline = PipelineData::Problems(vec![
        PipelineProblem {
            subject: Some((PipelineRole::Recognition, "zh".into())),
            fallback: false,
            code: "runtime_not_built_in".into(),
            args: vec!["qwen3-asr-0.6b".into(), "transcribe.cpp".into()],
        },
        PipelineProblem {
            subject: Some((PipelineRole::Translation, "zh>en".into())),
            fallback: true,
            code: "model_not_installed".into(),
            args: vec!["gemma-4-e4b-it-q4".into(), "4.6 GB".into()],
        },
    ]);
    busy.data.assistant.chain = PipelineData::Problems(vec![PipelineProblem {
        subject: Some((PipelineRole::Dialogue, String::new())),
        fallback: false,
        code: "not_available_yet".into(),
        args: vec!["openprx".into(), "OpenPRX agent".into()],
    }]);
    busy.data.notices = vec![
        SettingsNotice::UnreadableFileKept {
            kept_as: "config.toml.unreadable-1700000000".into(),
        },
        SettingsNotice::SaveFailed {
            detail: "permission denied".into(),
        },
    ];
    vec![plain, busy]
}

/// The horizontal range the controls of the page may occupy.
fn column(width: f32) -> (f32, f32) {
    if width >= WIDE_MIN_WIDTH {
        let left = WINDOW_PADDING + NAV_WIDTH + WINDOW_PADDING;
        (left, left + (width - left - WINDOW_PADDING).min(COLUMN_MAX))
    } else {
        (WINDOW_PADDING, width - WINDOW_PADDING)
    }
}

#[test]
fn every_section_renders_in_every_scheme_language_and_width() {
    let _guard = common::lock_locale();
    let mut combinations = 0;
    for locale in LOCALES {
        prxvc_i18n::set_locale(locale.clone());
        for scheme in SCHEMES {
            for width in WIDTHS {
                for section in SettingsSection::ALL {
                    for (variant, state) in variants(section).into_iter().enumerate() {
                        let tag =
                            format!("{section:?}/{locale:?}/{scheme:?}/{width}/variant {variant}");
                        let mut harness = window_harness(width, scheme, state);
                        // `run` fails on a missing glyph; `check_accessibility` on an unnamed input.
                        harness.run();
                        harness.check_accessibility();
                        let (left, right) = column(width);
                        for role in CONTROL_ROLES {
                            for node in harness.query_all(By::new().role(role)) {
                                let rect = node.rect();
                                let name = node.accesskit_node().label().unwrap_or_default();
                                if role == Role::MenuItem && width >= WIDE_MIN_WIDTH {
                                    assert!(
                                        rect.left() >= WINDOW_PADDING - 0.5
                                            && rect.right() <= WINDOW_PADDING + NAV_WIDTH + 0.5,
                                        "{tag}: section list item {name:?} is at {rect:?}"
                                    );
                                    continue;
                                }
                                assert!(
                                    rect.left() >= left - 0.5 && rect.right() <= right + 0.5,
                                    "{tag}: {role:?} {name:?} at {rect:?} leaves its column {left}..{right}"
                                );
                            }
                        }
                        combinations += 1;
                    }
                }
            }
        }
    }
    assert_eq!(combinations, 2 * 2 * 4 * 11 * 2);
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn every_page_has_rows_and_a_heading() {
    let _guard = common::lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    for section in SettingsSection::ALL {
        let mut harness = window_harness(1200.0, Theme::Light, WindowState::new(section));
        harness.run();
        // The page heading is the section's name; the section list shows it too.
        assert!(
            harness.query_all_by_label(&section.label()).count() >= 2,
            "{section:?}: heading and list item"
        );
        let interactive = harness.query_all(By::new().role(Role::Switch)).count()
            + harness.query_all(By::new().role(Role::ComboBox)).count()
            + harness.query_all(By::new().role(Role::Button)).count()
            + harness.query_all(By::new().role(Role::TextInput)).count()
            + harness.query_all(By::new().role(Role::RadioButton)).count();
        assert!(
            interactive >= 3,
            "{section:?} has only {interactive} controls"
        );
    }
}

/// What the page paints, as the union of the bounds of every shape of its last frame.
fn painted_bounds(width: f32, scheme: Theme, mut state: WindowState) -> egui::Rect {
    use egui::{Event, RawInput, Rect, pos2, vec2};
    let ctx = egui::Context::default();
    prxvc_desktop_ui::fonts::install(&ctx, None);
    prxvc_desktop_ui::theme::install(&ctx);
    let mut painted = Rect::NOTHING;
    for frame in 0..3 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 2600.0))),
            system_theme: Some(scheme),
            time: Some(f64::from(frame) * 0.016),
            events: Vec::<Event>::new(),
            ..Default::default()
        };
        let output = ctx.run_pass(input, |ctx| {
            egui::Area::new(egui::Id::unique("painted"))
                .fixed_pos(pos2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.set_width(width);
                    let tk = prxvc_desktop_ui::theme::tokens_for(ui.ctx());
                    tool_window::window_frame(&tk).show(ui, |ui| {
                        settings_window::body(
                            ui,
                            &tk,
                            settings_window::BodyArgs {
                                settings: &mut state.settings,
                                data: &state.data,
                                commands: &mut state.commands,
                                view: &mut state.view,
                                current_theme: prxvc_desktop_ui::theme::RF_THEME_NAME,
                                pending_theme: &mut state.pending_theme,
                            },
                        );
                    });
                });
        });
        painted = Rect::NOTHING;
        for clipped in &output.shapes {
            let bounds = clipped.shape.visual_bounding_rect();
            // The window's background fill starts at the corner and includes the padding.
            if bounds.min.x > 0.5 || bounds.min.y > 0.5 {
                painted = painted.union(bounds);
            }
        }
        output.drop_without_applying_deltas();
    }
    painted
}

use prxvc_desktop_ui::{settings_window, tool_window};

/// Not only the controls: whatever a page paints (a long value in a select, a long host in a
/// chip, a long path) stays inside the window's padding.
#[test]
fn painted_content_never_runs_past_the_right_edge_of_its_column() {
    let _guard = common::lock_locale();
    for locale in LOCALES {
        prxvc_i18n::set_locale(locale.clone());
        for width in [480.0, 528.0, 1200.0] {
            for section in SettingsSection::ALL {
                for (variant, state) in variants(section).into_iter().enumerate() {
                    let painted = painted_bounds(width, Theme::Light, state);
                    let (_, right) = column(width);
                    assert!(
                        painted.right() <= right + 2.0,
                        "{section:?}/{locale:?}/{width}/variant {variant}: paints up to {} but the column ends at {right}",
                        painted.right()
                    );
                    assert!(
                        painted.left() >= WINDOW_PADDING - 3.0,
                        "{section:?}/{locale:?}/{width}/variant {variant}: paints from {}",
                        painted.left()
                    );
                }
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}
