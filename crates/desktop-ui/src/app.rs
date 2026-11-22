//! Top-level composition of the floating orb UI: `render_app` and the per-area steps it runs each frame.

use egui::Color32;

use crate::assistant_view::issue_card;
use crate::call_window_view::{self, CallWindowAction};
use crate::context_menu::orb_context_menu;
use crate::debug_view::debug_body;
use crate::history_view::history_rows;
use crate::orb_view::{orb_animation, paint_orb};
use crate::repaint;
use crate::settings_window;
use crate::state::DesktopUiState;
use crate::status_hint::{state_hint, translate_state_human};
use crate::status_pill::StatusPill;
use crate::theme::{self, Scheme, Tokens, layout, space};
use crate::theme_presets::Theme;
use crate::tool_window::{self, ToolKind};
use crate::transcript_view::transcript_card;
use crate::widgets::{self, ButtonKind};

/// Render the floating orb UI (frameless transparent mode).
///
/// The entire window is transparent. Only the orb, state label, transcript,
/// and context-menu popups are visible. The orb is draggable to move the window.
pub fn render_app(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    let ctx = &ui.ctx().clone();
    let time = ctx.input(|i| i.time);
    // Animation is driven by elapsed time, so the lower idle frame rate looks
    // the same; a still orb (idle animation off) is drawn at time zero.
    let time = if state.idle_animation || repaint::is_active_state(&state.session_state) {
        time
    } else {
        0.0
    };
    state.anim_time = time;

    // Cmd+, (macOS) or Ctrl+, opens the settings window while the orb window is focused.
    if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::Comma)) {
        state.open_settings(None);
    }

    let tk = resolve_tokens(ctx, state);

    // Transparent background for central panel
    let frame = egui::Frame::NONE.fill(Color32::TRANSPARENT);

    egui::CentralPanel::default()
        .frame(frame)
        .show(ui, |ui| render_stage(ui, state, &tk, time));

    show_tool_windows(ctx, state, &tk);
    schedule_repaint(ctx, state);
}

/// Install the style, resolve the RF scheme against what egui now uses (the
/// system light/dark preference for the RF theme) and return the tokens.
fn resolve_tokens(ctx: &egui::Context, state: &mut DesktopUiState) -> Tokens {
    // Style first, then resolve the RF scheme against what egui now uses
    // (the system light/dark preference for the RF theme).
    theme::ensure_installed(ctx, &state.theme);
    if state.theme.is_rf() {
        let scheme = Scheme::from_egui(ctx.theme());
        if state.theme.scheme != scheme {
            state.theme = Theme::rf(scheme);
        }
    }
    state.theme.tokens()
}

/// The central stage: orb, state pill, end button, transcript card, and the
/// interactions on the orb.
fn render_stage(ui: &mut egui::Ui, state: &mut DesktopUiState, tk: &Tokens, time: f64) {
    let rect = ui.max_rect();
    let center = egui::pos2(rect.center().x, rect.top() + 90.0);

    // === ORB ANIMATION STATE ===
    let base_radius = 40.0;
    let (orb_color, scale) = orb_animation(state, time);
    let radius = base_radius * scale;
    let label_pos = egui::pos2(center.x, center.y + radius + 16.0);
    let label_text = translate_state_human(&state.session_state);
    let hint = state_hint(&state.session_state);

    let pill = StatusPill::layout(
        ui.painter(),
        tk,
        label_pos,
        &label_text,
        &hint,
        rect.width() - 2.0 * space::S8,
    );
    let below_pill = pill.rect.bottom() + space::S8;

    // === ALLOCATE INTERACTIVE AREAS (requires &mut ui) ===
    let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.5, radius * 2.5));
    let orb_response = ui.allocate_rect(orb_rect, egui::Sense::click_and_drag());

    // End button (only when session active): a quiet destructive action.
    let stop_clicked = end_button(ui, state, tk, rect, below_pill);
    let controls_h = if state.session_active {
        layout::CONTROL_HEIGHT_SMALL + space::S8
    } else {
        0.0
    };

    // The assistant's issue, when there is one, above the transcript.
    let issue_h = issue_card(ui, state, tk, rect, below_pill + controls_h);

    // Transcript card (must be created before painting)
    if !state.transcript_lines.is_empty() {
        transcript_card(ui, state, tk, rect, below_pill + controls_h + issue_h);
    }

    // === PAINT EVERYTHING (only needs &ui / painter, no more mutations) ===
    let painter = ui.painter();
    paint_orb(
        painter,
        tk,
        center,
        radius,
        orb_color,
        time,
        orb_response.hovered(),
    );
    if orb_response.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    pill.paint(painter, tk);

    // === HANDLE INTERACTIONS (no borrow conflicts here) ===
    handle_orb_interactions(ui, state, &orb_response);
    orb_context_menu(&orb_response, state, tk);

    // End button click
    if stop_clicked {
        state.pending_commands.push("StopSession".into());
    }
}

/// Draw the end button below the pill while a session is active; true when
/// it was clicked this frame.
fn end_button(
    ui: &mut egui::Ui,
    state: &DesktopUiState,
    tk: &Tokens,
    rect: egui::Rect,
    below_pill: f32,
) -> bool {
    let mut stop_clicked = false;
    if state.session_active {
        let btn_rect = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - 80.0, below_pill),
            egui::vec2(160.0, layout::CONTROL_HEIGHT_SMALL),
        );
        ui.scope_builder(
            egui::UiBuilder::new()
                .max_rect(btn_rect)
                .layout(egui::Layout::top_down(egui::Align::Center)),
            |ui| {
                stop_clicked = widgets::button_with(
                    ui,
                    tk,
                    ButtonKind::DangerQuiet,
                    &prxvc_i18n::t("session.stop"),
                    layout::CONTROL_HEIGHT_SMALL,
                    true,
                )
                .clicked();
            },
        );
    }
    stop_clicked
}

/// Orb click (start, interrupt while the assistant speaks, otherwise end) and drag (move the frameless window).
fn handle_orb_interactions(
    ui: &mut egui::Ui,
    state: &mut DesktopUiState,
    orb_response: &egui::Response,
) {
    // Click to start/stop/interrupt
    if orb_response.clicked() {
        tracing::info!(
            session_active = state.session_active,
            session_state = %state.session_state,
            "orb clicked"
        );
        if state.session_state == "Starting" {
            // The engines are loading: this click is ignored, not queued behind the start.
        } else if !state.session_active {
            state.pending_commands.push("CreateSession".into());
            state.pending_commands.push("StartSession".into());
        } else if state.session_state == "AssistantSpeaking" {
            state.pending_commands.push("InterruptPlayback".into());
        } else {
            // Listening or thinking: a second click ends the session, as the end button does.
            state.pending_commands.push("StopSession".into());
        }
    }

    // Drag to move the frameless window
    if orb_response.dragged() {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

/// The Settings, History and Debug windows.
///
/// Each is a native window of its own (see `tool_window`); the orb window
/// is not resized or moved by them.
fn show_tool_windows(ctx: &egui::Context, state: &mut DesktopUiState, tk: &Tokens) {
    let placement = tool_window::Placement::from_root(ctx);

    let current_theme = state.theme.name;
    {
        let DesktopUiState {
            windows,
            settings,
            pending_theme,
            settings_view,
            settings_data,
            settings_commands,
            ..
        } = &mut *state;
        tool_window::show_raw(
            ctx,
            tk,
            ToolKind::Settings,
            &mut windows.settings,
            placement,
            |ui| {
                settings_window::body(
                    ui,
                    tk,
                    settings_window::BodyArgs {
                        settings: &mut *settings,
                        data: settings_data,
                        commands: &mut *settings_commands,
                        view: &mut *settings_view,
                        current_theme,
                        pending_theme: &mut *pending_theme,
                    },
                );
            },
        );
    }

    let history_entries = state
        .windows
        .history
        .is_open()
        .then(|| state.history_entries.clone());
    if let Some(entries) = history_entries {
        tool_window::show(
            ctx,
            tk,
            ToolKind::History,
            &mut state.windows.history,
            placement,
            |ui| {
                history_rows(ui, tk, &entries);
            },
        );
    }

    let debug_snapshot = state.windows.debug.is_open().then(|| {
        (
            state.debug_state_info.clone(),
            state.debug_events.clone(),
            state.call.clone(),
        )
    });
    if let Some((info, events, call)) = debug_snapshot {
        let mut open_call_window = false;
        {
            let DesktopUiState { windows, .. } = &mut *state;
            tool_window::show(
                ctx,
                tk,
                ToolKind::Debug,
                &mut windows.debug,
                placement,
                |ui| {
                    debug_body(ui, tk, &info, &events, &call, &mut open_call_window);
                },
            );
        }
        if open_call_window {
            state.windows.request_open(ToolKind::Call);
        }
    }

    show_call_window(ctx, state, tk, placement);
}

/// The Call translation window: draws the call panel and applies what was clicked after the frame.
fn show_call_window(
    ctx: &egui::Context,
    state: &mut DesktopUiState,
    tk: &Tokens,
    placement: tool_window::Placement,
) {
    if !state.windows.call.is_open() {
        return;
    }
    let call = state.call.clone();
    let mut actions = Vec::new();
    {
        let DesktopUiState {
            windows,
            call_translate,
            settings,
            call_window,
            ..
        } = &mut *state;
        tool_window::show_raw(
            ctx,
            tk,
            ToolKind::Call,
            &mut windows.call,
            placement,
            |ui| {
                call_window_view::body(
                    ui,
                    tk,
                    &call,
                    call_translate,
                    settings,
                    call_window,
                    &mut actions,
                );
            },
        );
    }
    apply_call_actions(state, actions);
}

/// Apply what was clicked in the Call translation window after the frame.
fn apply_call_actions(state: &mut DesktopUiState, actions: Vec<CallWindowAction>) {
    for action in actions {
        match action {
            CallWindowAction::Command(cmd) => state.pending_commands.push(cmd.to_owned()),
            CallWindowAction::OpenSettings(section) => state.open_settings(Some(section)),
        }
    }
}

/// Draw the content of the Call translation window into `ui`, in the padded window frame, and
/// apply what was clicked. The window itself draws the same content; this entry exists so that
/// tests and snapshots can show it without the orb around it.
pub fn render_call_window(ui: &mut egui::Ui, state: &mut DesktopUiState) {
    let tk = crate::theme::tokens_for(ui.ctx());
    let call = state.call.clone();
    let mut actions = Vec::new();
    egui::CentralPanel::default()
        .frame(tool_window::window_frame(&tk))
        .show(ui, |ui| {
            let DesktopUiState {
                call_translate,
                settings,
                call_window,
                ..
            } = &mut *state;
            call_window_view::body(
                ui,
                &tk,
                &call,
                call_translate,
                settings,
                call_window,
                &mut actions,
            );
        });
    apply_call_actions(state, actions);
}

/// Ask egui for the next repaint according to the state-based policy.
fn schedule_repaint(ctx: &egui::Context, state: &DesktopUiState) {
    repaint::schedule(
        ctx,
        repaint::decide(&repaint::RepaintInputs {
            session_state: &state.session_state,
            orb_hidden: ctx.input(|i| i.viewport().visible()) == Some(false),
            tool_window_open: ToolKind::ALL
                .iter()
                .any(|k| state.windows.get(*k).is_open()),
            idle_animation: state.idle_animation,
        }),
    );
}
