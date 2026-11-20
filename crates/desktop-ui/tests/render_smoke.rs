//! Headless render smoke tests: build the UI against an `egui::Context` with a
//! fixed screen rect (no window) for both colour schemes and both languages.

use std::sync::Mutex;

use egui::epaint::Shape;
use egui::{Event, Modifiers, PointerButton, RawInput, Rect, Theme as EguiTheme, pos2, vec2};
use prxvc_desktop_ui::fonts;
use prxvc_desktop_ui::theme::{self, Scheme};
use prxvc_desktop_ui::tool_window::{self, ToolKind};
use prxvc_desktop_ui::widgets::{self, ButtonKind};
use prxvc_desktop_ui::{
    DesktopUiState, HistoryEntry, NetworkStatus, Theme, TranscriptLine, UiSettings,
    render_error_banner, render_settings_panel, settings_view,
};
use prxvc_i18n::Locale;

/// The i18n locale is global; tests that change it must not interleave.
static LOCALE_LOCK: Mutex<()> = Mutex::new(());

fn lock_locale() -> std::sync::MutexGuard<'static, ()> {
    LOCALE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The window size of the orb client while a dialog is open.
const SCREEN: egui::Vec2 = vec2(400.0, 600.0);

/// A context with the bundled fonts installed and active, as the application
/// has them. Every text style refers to a named weight family, so a context
/// without them cannot lay out text.
fn new_ctx() -> egui::Context {
    let ctx = egui::Context::default();
    fonts::install(&ctx, None);
    ctx.run_ui(RawInput::default(), |_| {}).drop_without_applying_deltas();
    ctx
}

fn raw_input(system: EguiTheme, time: f64, events: Vec<Event>) -> RawInput {
    raw_input_sized(system, SCREEN, time, events)
}

fn raw_input_sized(system: EguiTheme, size: egui::Vec2, time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), size)),
        system_theme: Some(system),
        time: Some(time),
        events,
        ..Default::default()
    }
}

fn collect_text(shape: &Shape, out: &mut String) {
    match shape {
        Shape::Text(t) => {
            out.push_str(t.galley.text());
            out.push('\n');
        }
        Shape::Vec(shapes) => shapes.iter().for_each(|s| collect_text(s, out)),
        _ => {}
    }
}

/// Run `frames` frames of `ui` and return (number of shapes in the last
/// frame, all text painted in the last frame).
fn run_frames(
    ctx: &egui::Context,
    system: EguiTheme,
    frames: usize,
    ui: impl FnMut(&mut egui::Ui),
) -> (usize, String) {
    run_frames_sized(ctx, system, SCREEN, frames, ui)
}

fn run_frames_sized(
    ctx: &egui::Context,
    system: EguiTheme,
    size: egui::Vec2,
    frames: usize,
    mut ui: impl FnMut(&mut egui::Ui),
) -> (usize, String) {
    let mut last = (0, String::new());
    for i in 0..frames {
        let output = ctx.run_ui(raw_input_sized(system, size, i as f64 * 0.016, vec![]), |u| ui(u));
        let mut text = String::new();
        for clipped in &output.shapes {
            collect_text(&clipped.shape, &mut text);
        }
        assert!(
            !text.to_lowercase().contains("clash"),
            "egui reported a widget id clash:\n{text}"
        );
        last = (output.shapes.len(), text);
        output.drop_without_applying_deltas();
    }
    last
}

fn sample_state() -> DesktopUiState {
    let mut state = DesktopUiState::new();
    state.session_active = true;
    state.session_state = "AssistantSpeaking".to_owned();
    state.network_status = NetworkStatus::Reconnecting;
    state.reconnect_attempts = 2;
    state.transcript_lines = vec![
        TranscriptLine {
            role: "user".into(),
            text: "Hello there".into(),
            is_final: true,
        },
        TranscriptLine {
            role: "assistant".into(),
            text: "Hi, how can I help?".into(),
            is_final: false,
        },
    ];
    state.history_entries = vec![
        HistoryEntry {
            session_id: "s1".into(),
            title: "First session".into(),
            created_at: "2026-11-20 10:00".into(),
            duration: "3 min".into(),
            state: "Completed".into(),
        },
        HistoryEntry {
            session_id: "s2".into(),
            title: "Second session".into(),
            created_at: "2026-11-20 11:00".into(),
            duration: "1 min".into(),
            state: "Failed".into(),
        },
    ];
    state.debug_state_info = "AssistantSpeaking (session: s2)".into();
    state.debug_events = vec!["SessionCreated".into(), "SessionStarted".into()];
    state
}

const SCHEMES: [(Scheme, EguiTheme); 2] = [
    (Scheme::Light, EguiTheme::Light),
    (Scheme::Dark, EguiTheme::Dark),
];
const LOCALES: [Locale; 2] = [Locale::En, Locale::ZhCn];

#[test]
fn rf_is_the_default_theme() {
    assert!(Theme::default().is_rf());
    assert!(DesktopUiState::new().theme.is_rf());
    assert_eq!(Theme::all()[0].name, theme::RF_THEME_NAME);
    // The existing themes remain selectable.
    let names: Vec<_> = Theme::all().iter().map(|t| t.name).collect();
    for legacy in ["Midnight", "Ocean", "Rose", "Forest", "Ember"] {
        assert!(names.contains(&legacy), "{legacy} must stay selectable");
    }
}

#[test]
fn main_view_renders_for_both_schemes_and_languages() {
    let _guard = lock_locale();
    for (scheme, system) in SCHEMES {
        for locale in LOCALES {
            prxvc_i18n::set_locale(locale.clone());
            let ctx = new_ctx();
            let mut state = sample_state();
            let (shapes, _) = run_frames(&ctx, system, 3, |ui| {
                prxvc_desktop_ui::render_app(ui, &mut state);
            });
            assert!(shapes > 0, "{scheme:?}/{locale:?}: no shapes painted");
            assert_eq!(
                state.theme.scheme, scheme,
                "the RF theme must follow the system scheme"
            );
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn popup_dialogs_render_for_both_schemes_and_languages() {
    let _guard = lock_locale();
    for (_, system) in SCHEMES {
        for locale in LOCALES {
            prxvc_i18n::set_locale(locale.clone());
            let ctx = new_ctx();
            let mut state = sample_state();
            state.windows.request_open(ToolKind::Settings);
            state.windows.request_open(ToolKind::History);
            state.windows.request_open(ToolKind::Debug);
            state.settings.use_mock = false; // also shows the server URL field
            let (shapes, text) = run_frames(&ctx, system, 3, |ui| {
                prxvc_desktop_ui::render_app(ui, &mut state);
            });
            assert!(shapes > 0);
            assert!(text.contains(&prxvc_i18n::t("settings.title")));
            assert!(text.contains(&prxvc_i18n::t("history.title")));
            assert!(text.contains(&prxvc_i18n::t("debug.title")));
            assert!(text.contains("First session"));
            assert!(text.contains("AssistantSpeaking (session: s2)"));
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn legacy_themes_still_render() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    for legacy in Theme::all().into_iter().filter(|t| !t.is_rf()) {
        let ctx = new_ctx();
        let mut state = sample_state();
        state.theme = legacy;
        state.windows.request_open(ToolKind::Settings);
        let (shapes, _) = run_frames(&ctx, EguiTheme::Light, 3, |ui| {
            prxvc_desktop_ui::render_app(ui, &mut state);
        });
        assert!(shapes > 0);
        // A legacy theme pins the dark scheme regardless of the system.
        assert_eq!(ctx.theme(), EguiTheme::Dark);
    }
}

#[test]
fn every_existing_setting_is_shown_in_both_languages() {
    let _guard = lock_locale();
    for (_, system) in SCHEMES {
        for locale in LOCALES {
            prxvc_i18n::set_locale(locale.clone());
            let ctx = new_ctx();
            theme::install(&ctx);
            let mut settings = UiSettings {
                use_mock: false,
                ..UiSettings::default()
            };
            let mut pending = None;
            // Tall screen so that no row is clipped away.
            let (shapes, text) = run_frames_sized(&ctx, system, vec2(400.0, 2400.0), 2, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let tk = theme::tokens_for(ui.ctx());
                    settings_view::render_settings_content(
                        ui,
                        &tk,
                        &mut settings,
                        theme::RF_THEME_NAME,
                        &mut pending,
                    );
                });
            });
            assert!(shapes > 0);
            for key in [
                "settings.theme",
                "settings.language",
                "settings.auto_play",
                "settings.save_history",
                "settings.show_transcript",
                "settings.use_mock",
                "settings.server_url",
                "settings.hotkey",
                "settings.appearance",
                "settings.section.language",
                "settings.section.audio",
                "settings.section.privacy",
                "settings.section.connection",
                "settings.section.system",
            ] {
                let label = prxvc_i18n::t(key);
                assert_ne!(label, key, "missing translation for {key}");
                assert!(text.contains(&label), "{locale:?}: {key} not shown");
            }
            assert!(text.contains(&prxvc_i18n::t("settings.language.en")));
            assert!(text.contains(&prxvc_i18n::t("settings.language.zh")));
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

#[test]
fn legacy_panels_and_banner_render() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let ctx = new_ctx();
    let mut state = sample_state();
    state.error_message = Some("Connection lost".into());
    state.error_recoverable = true;
    let mut settings = UiSettings::default();
    let (shapes, text) = run_frames(&ctx, EguiTheme::Light, 2, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            render_error_banner(ui, &mut state);
            render_settings_panel(ui, &mut settings);
            prxvc_desktop_ui::render_nav_bar(ui, &mut state);
            prxvc_desktop_ui::render_history_panel(ui, &state.history_entries.clone());
            prxvc_desktop_ui::render_debug_page(ui, &state);
            prxvc_desktop_ui::render_about_page(ui);
        });
    });
    assert!(shapes > 0);
    assert!(text.contains("Connection lost"));
    assert!(text.contains(&prxvc_i18n::t("common.retry")));
}

/// Press and release the pointer at `pos` over three frames and return
/// whatever `body` produced in the last frame.
fn click_at<R>(
    ctx: &egui::Context,
    pos: egui::Pos2,
    mut body: impl FnMut(&egui::Context) -> R,
) -> R {
    let button = |pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    // Two warm-up frames: a fresh area is laid out invisibly on its first frame.
    let frames = [
        vec![],
        vec![],
        vec![Event::PointerMoved(pos), button(true)],
        vec![button(false)],
    ];
    let mut result = None;
    for (i, events) in frames.into_iter().enumerate() {
        ctx.run_pass(raw_input(EguiTheme::Light, i as f64 * 0.05, events), |ctx| {
            result = Some(body(ctx));
        })
        .drop_without_applying_deltas();
    }
    result.expect("at least one frame ran")
}

#[test]
fn switch_toggles_on_click() {
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::LIGHT;
    let mut value = false;
    let changed = click_at(&ctx, pos2(60.0, 60.0), |ctx| {
        let mut changed = false;
        egui::Area::new(egui::Id::unique("switch_test"))
            .fixed_pos(pos2(50.0, 50.0))
            .show(ctx, |ui| {
                changed = widgets::switch(ui, &tk, &mut value).changed();
            });
        changed
    });
    assert!(changed, "the click must report a change");
    assert!(value, "the switch must turn on");
}

#[test]
fn disabled_button_does_not_react() {
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::LIGHT;
    let clicked = click_at(&ctx, pos2(60.0, 60.0), |ctx| {
        let mut clicked = false;
        egui::Area::new(egui::Id::unique("button_test"))
            .fixed_pos(pos2(50.0, 50.0))
            .show(ctx, |ui| {
                clicked = widgets::button_with(ui, &tk, ButtonKind::Primary, "Go", 32.0, false)
                    .clicked();
            });
        clicked
    });
    assert!(!clicked);
}

#[test]
fn enabled_button_reports_click() {
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::DARK;
    let clicked = click_at(&ctx, pos2(60.0, 60.0), |ctx| {
        let mut clicked = false;
        egui::Area::new(egui::Id::unique("button_test"))
            .fixed_pos(pos2(50.0, 50.0))
            .show(ctx, |ui| {
                clicked = widgets::button(ui, &tk, ButtonKind::Secondary, "Go").clicked();
            });
        clicked
    });
    assert!(clicked);
}

#[test]
fn segmented_click_selects_the_other_segment() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::LIGHT;
    let mut chosen = None;
    // The second segment of a two-segment control is right of the first; click
    // near the right end of the track.
    click_at(&ctx, pos2(50.0 + 100.0, 66.0), |ctx| {
        egui::Area::new(egui::Id::unique("segmented_test"))
            .fixed_pos(pos2(50.0, 50.0))
            .show(ctx, |ui| {
                if let Some(i) = widgets::segmented(ui, &tk, 0, &["English", "\u{4e2d}\u{6587}"]) {
                    chosen = Some(i);
                }
            });
    });
    assert_eq!(chosen, Some(1));
}

/// Settings content laid out at the standard's 480 px width, in every scheme
/// and language: nothing the content paints reaches outside its rectangle.
#[test]
fn settings_content_fits_480_px_in_both_schemes_and_languages() {
    let _guard = lock_locale();
    let content_width = tool_window::SETTINGS_CONTENT_WIDTH;
    assert_eq!(content_width, 480.0);
    for (scheme, system) in SCHEMES {
        for locale in LOCALES {
            for use_mock in [true, false] {
                prxvc_i18n::set_locale(locale.clone());
                let ctx = new_ctx();
                theme::install(&ctx);
                let mut settings = UiSettings { use_mock, ..UiSettings::default() };
                let mut pending = None;
                let content =
                    Rect::from_min_size(pos2(tool_window::WINDOW_PADDING, 20.0), vec2(content_width, 4000.0));
                let mut used = Rect::NOTHING;
                let mut painted = Rect::NOTHING;
                for i in 0..3 {
                    let input = raw_input_sized(system, vec2(content_width + 48.0, 4100.0), i as f64 * 0.016, vec![]);
                    let output = ctx.run_pass(input, |ctx| {
                        egui::Area::new(egui::Id::unique("fit"))
                            .fixed_pos(content.min)
                            .show(ctx, |ui| {
                                ui.set_width(content_width);
                                let tk = theme::tokens_for(ui.ctx());
                                settings_view::render_settings_content(
                                    ui,
                                    &tk,
                                    &mut settings,
                                    theme::RF_THEME_NAME,
                                    &mut pending,
                                );
                                used = ui.min_rect();
                            });
                    });
                    painted = Rect::NOTHING;
                    for clipped in &output.shapes {
                        painted = painted.union(clipped.shape.visual_bounding_rect());
                    }
                    output.drop_without_applying_deltas();
                }
                let tag = format!("{scheme:?}/{locale:?}/mock={use_mock}");
                assert!(
                    used.width() <= content_width + 0.5,
                    "{tag}: content is {} wide, the content rect is {content_width}",
                    used.width()
                );
                assert!(
                    painted.left() >= content.left() - 2.0 && painted.right() <= content.right() + 2.0,
                    "{tag}: painted {painted:?} leaves the content rect {content:?}"
                );
                assert!(used.height() > 400.0, "{tag}: all sections should be laid out");
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}

/// The window title comes from the catalogue in both languages.
#[test]
fn tool_window_titles_are_translated() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let en = ToolKind::Settings.title();
    prxvc_i18n::set_locale(Locale::ZhCn);
    let zh = ToolKind::Settings.title();
    prxvc_i18n::set_locale(Locale::En);
    assert!(!en.is_empty() && !zh.is_empty());
    assert_ne!(en, zh);
    assert_ne!(en, "settings.title", "the key must exist in the catalogue");
}

/// Opening the settings window does not touch the orb window: no viewport
/// command resizes or moves the root viewport, in either direction.
#[test]
fn opening_and_closing_settings_never_resizes_or_moves_the_orb_window() {
    let _guard = lock_locale();
    prxvc_i18n::set_locale(Locale::En);
    let ctx = new_ctx();
    let mut state = sample_state();
    let mut commands = Vec::new();
    for frame in 0..6 {
        if frame == 1 {
            state.windows.request_open(ToolKind::Settings);
        }
        if frame == 4 {
            state.windows.get_mut(ToolKind::Settings).close();
        }
        let output = ctx.run_ui(raw_input(EguiTheme::Light, frame as f64 * 0.016, vec![]), |ui| {
            prxvc_desktop_ui::render_app(ui, &mut state);
        });
        if let Some(root) = output.viewport_output.get(&egui::ViewportId::ROOT) {
            commands.extend(root.commands.iter().cloned());
        }
        output.drop_without_applying_deltas();
    }
    assert!(
        !commands.iter().any(|c| matches!(
            c,
            egui::ViewportCommand::InnerSize(_)
                | egui::ViewportCommand::OuterPosition(_)
                | egui::ViewportCommand::MinInnerSize(_)
                | egui::ViewportCommand::MaxInnerSize(_)
        )),
        "the orb window must not be resized or moved: {commands:?}"
    );
    assert!(!state.windows.settings.is_open());
}

#[test]
fn setting_row_puts_the_control_right_and_centred() {
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::LIGHT;
    let mut value = false;
    let mut control = Rect::NOTHING;
    let mut row = Rect::NOTHING;
    run_frames_sized(&ctx, EguiTheme::Light, vec2(400.0, 600.0), 3, |ui| {
        egui::Area::new(egui::Id::unique("row"))
            .fixed_pos(pos2(20.0, 20.0))
            .show(ui.ctx(), |ui| {
                ui.set_width(320.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                let top = ui.cursor().top();
                widgets::setting_row(
                    ui,
                    &tk,
                    "Label",
                    Some("A description that is long enough to wrap onto a second line here."),
                    theme::layout::SWITCH_WIDTH,
                    true,
                    |ui| {
                        control = widgets::switch(ui, &tk, &mut value).rect;
                    },
                );
                row = Rect::from_min_max(pos2(20.0, top), pos2(340.0, ui.cursor().top()));
            });
    });
    assert!((control.right() - 340.0).abs() < 0.5, "control must sit at the right edge");
    let mid = (control.center().y - row.top()) - (row.bottom() - control.center().y);
    assert!(mid.abs() < 1.0, "control must be vertically centred in the row, off by {mid}");
    assert!(row.height() >= 56.0, "row is at least padding + 32 + padding");
}

// ---------------------------------------------------------------------------
// Bundled fonts
// ---------------------------------------------------------------------------

/// Sample strings every text style must draw without the replacement glyph.
const GLYPH_SAMPLES: &[(&str, &str)] = &[
    ("latin", "The quick brown fox jumps over the lazy dog. PRX voice"),
    ("digits", "0123456789 12:30 3.14 100%"),
    ("ascii punctuation", ".,;:!?()[]{}<>-_/\\@#&*+=~^'\"|`$"),
    ("chinese", "你好，世界！语音助手已连接，请开始说话。"),
    ("chinese mixed", "连接 Gmail 账号后，可在 2026 年同步"),
    ("cjk punctuation", "、。「」『』《》【】〈〉…—·"),
    ("full-width forms", "（１２３）：；？！ＡＢＣ％"),
];

#[test]
fn every_text_style_draws_every_sample_without_the_replacement_glyph() {
    let ctx = new_ctx();
    theme::install(&ctx);
    ctx.run_ui(RawInput::default(), |_| {}).drop_without_applying_deltas();
    for (scheme, _) in SCHEMES {
        let style = theme::style_for(&scheme.tokens(), matches!(scheme, Scheme::Dark));
        for (text_style, font_id) in &style.text_styles {
            for (what, sample) in GLYPH_SAMPLES {
                let missing: Vec<char> = sample
                    .chars()
                    .filter(|c| !ctx.fonts_mut(|f| f.has_glyph(font_id, *c)))
                    .collect();
                assert!(
                    missing.is_empty(),
                    "{text_style:?} ({:?}): {what} sample is missing glyphs {missing:?}",
                    font_id.family
                );
            }
        }
    }
}

#[test]
fn every_weight_family_draws_the_samples() {
    let ctx = new_ctx();
    for family in fonts::families() {
        let id = egui::FontId::new(15.0, family.clone());
        for (what, sample) in GLYPH_SAMPLES {
            assert!(
                ctx.fonts_mut(|f| f.has_glyphs(&id, sample)),
                "{family:?}: {what} sample is not fully covered"
            );
        }
    }
}

#[test]
fn glyph_check_detects_a_missing_glyph() {
    // A rare hanzi outside the embedded subset and without any system font in
    // the test context: the check must say so, or the tests above prove nothing.
    let ctx = new_ctx();
    let id = egui::FontId::new(15.0, egui::FontFamily::Proportional);
    assert!(!ctx.fonts_mut(|f| f.has_glyph(&id, '\u{20000}')));
}

#[test]
fn text_styles_use_the_standard_weights() {
    let style = theme::style_for(&theme::LIGHT, false);
    let family = |s: egui::TextStyle| style.text_styles[&s].family.clone();
    assert_eq!(family(egui::TextStyle::Body), egui::FontFamily::Proportional);
    assert_eq!(family(egui::TextStyle::Small), egui::FontFamily::Proportional);
    assert_eq!(family(egui::TextStyle::Button), fonts::medium());
    assert_eq!(family(egui::TextStyle::Heading), fonts::semibold());
    assert_eq!(family(egui::TextStyle::Monospace), egui::FontFamily::Monospace);
}

#[test]
fn heading_text_is_semibold_and_body_text_is_regular() {
    let ctx = new_ctx();
    theme::install(&ctx);
    let tk = theme::LIGHT;
    let mut seen = Vec::new();
    ctx.run_ui(RawInput::default(), |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            let items = [
                widgets::text("Title", theme::size::TITLE, tk.text),
                widgets::text("Section", theme::size::SUBHEADING, tk.text),
                widgets::text("Body", theme::size::BODY, tk.text),
                widgets::text_medium("Row", theme::size::BODY, tk.text),
            ];
            seen = items
                .into_iter()
                .map(|t| {
                    let galley = egui::WidgetText::from(t).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Extend),
                        200.0,
                        egui::TextStyle::Body,
                    );
                    galley.job.sections[0].format.font_id.family.clone()
                })
                .collect();
        });
    })
    .drop_without_applying_deltas();
    assert_eq!(
        seen,
        vec![
            fonts::semibold(),
            fonts::semibold(),
            egui::FontFamily::Proportional,
            fonts::medium()
        ]
    );
}

#[test]
fn chinese_and_latin_text_render_in_every_style() {
    let _guard = lock_locale();
    let ctx = new_ctx();
    theme::install(&ctx);
    let (shapes, text) = run_frames(&ctx, EguiTheme::Light, 2, |ui| {
        egui::CentralPanel::default().show(ui, |ui| {
            for style in [
                egui::TextStyle::Small,
                egui::TextStyle::Body,
                egui::TextStyle::Button,
                egui::TextStyle::Heading,
                egui::TextStyle::Monospace,
            ] {
                for (_, sample) in GLYPH_SAMPLES {
                    ui.label(egui::RichText::new(*sample).text_style(style.clone()));
                }
            }
        });
    });
    assert!(shapes > 0);
    assert!(text.contains("你好，世界"));
    assert!(text.contains("quick brown fox"));
}

/// The call-mode control inside the debug window, in both languages, light and dark,
/// with an idle panel and with a running panel that carries issues.
#[test]
fn call_control_renders_in_both_languages_with_issues() {
    use prxvc_desktop_ui::call_view::{
        CaptionView, ClauseStatus, CountersView, LatencyView, PartialView,
    };
    use prxvc_desktop_ui::{
        ActivityView, CallPanel, CallPhase, Heard, IssueSeverity, IssueView, LaneView,
    };
    let _guard = lock_locale();
    let running = CallPanel {
        translate_enabled: false,
        phase: CallPhase::AudioReady,
        in_call_mode: true,
        default_input_ours: false,
        displaced_to: "AirPods".into(),
        microphone: "Built-in Mic".into(),
        virtual_sink: "PRX Voice Microphone (Internal Sink)".into(),
        call_app: "net.whatsapp.WhatsApp".into(),
        activity: ActivityView::OnOtherMicrophone,
        activity_devices: "AirPods".into(),
        outgoing: LaneView { running: true, hears: Heard::Original, device: "Built-in Mic".into(), frames: 120, rms: 0.1, peak: 0.4, ..LaneView::default() },
        incoming: LaneView { running: true, hears: Heard::Original, device: "process tap".into(), frames: 90, ..LaneView::default() },
        issues: vec![IssueView {
            key: "call.issue.default_input_displaced".into(),
            severity: IssueSeverity::Degraded,
            detail: "the default input is now AirPods".into(),
        }],
        last_error: Some(IssueView {
            key: "call.issue.driver_missing".into(),
            severity: IssueSeverity::Blocking,
            detail: String::new(),
        }),
    };
    // A translating call: captions with a stable prefix, counters, latency, level and the gate.
    let lane = LaneView {
        running: true,
        translating: true,
        languages: Some(("zh".into(), "en".into())),
        hears: Heard::Translation,
        device: "Built-in Mic".into(),
        frames: 500,
        partial: Some(PartialView { text: "今天天气很好我们去".into(), stable_len: 18 }),
        clauses: vec![
            CaptionView { source: "你好".into(), translation: "Hello.".into(), status: ClauseStatus::Played },
            CaptionView { source: "我们开会".into(), translation: String::new(), status: ClauseStatus::Translating },
        ],
        counters: Some(CountersView { late_dropped: 1, underruns: 2, ..CountersView::default() }),
        latency: Some(LatencyView {
            speech_start_to_first_audio_ms: Some(2500.0),
            pause_to_first_audio_ms: Some(900.0),
            stages: vec![("mt_first_token".into(), 90.0)],
        }),
        translated_peak: 0.3,
        translated_rms: 0.05,
        input_gated: true,
        ..LaneView::default()
    };
    let translating = CallPanel {
        translate_enabled: true,
        phase: CallPhase::Translating,
        in_call_mode: true,
        default_input_ours: true,
        outgoing: lane.clone(),
        incoming: LaneView { hears: Heard::Translation, ..lane },
        issues: vec![IssueView {
            key: "call.issue.speaker_output".into(),
            severity: IssueSeverity::Degraded,
            detail: "local output: MacBook Pro Speakers".into(),
        }],
        ..CallPanel::default()
    };
    for (_, system) in SCHEMES {
        for locale in LOCALES {
            prxvc_i18n::set_locale(locale.clone());
            for panel in [CallPanel::default(), running.clone(), translating.clone()] {
                let ctx = new_ctx();
                let mut state = sample_state();
                state.call = panel;
                state.windows.request_open(ToolKind::Debug);
                let (shapes, text) = run_frames(&ctx, system, 3, |ui| {
                    prxvc_desktop_ui::render_app(ui, &mut state);
                });
                assert!(shapes > 0);
                assert!(text.contains(&prxvc_i18n::t("call.title")), "{text}");
                assert!(text.contains(&prxvc_i18n::t("call.start")));
            }
        }
    }
    prxvc_i18n::set_locale(Locale::En);
}
