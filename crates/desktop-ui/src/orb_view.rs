//! The orb view: its painting, its state-driven colour and breathing animation, and the standalone `render_orb`.

use egui::Color32;

use crate::state::DesktopUiState;
use crate::theme::Tokens;

/// Render the animated orb in the center of the stage area.
///
/// The orb changes color and animation based on session state. Returns a
/// clickable response so callers can handle click-to-start/interrupt.
pub fn render_orb(ui: &mut egui::Ui, state: &DesktopUiState, time: f64) -> egui::Response {
    let available = ui.available_size();
    let center = egui::pos2(available.x / 2.0, available.y * 0.4);
    let base_radius: f32 = 80.0;
    let tk = state.theme.tokens();

    let (orb_color, scale_anim) = orb_animation(state, time);
    let radius = base_radius * scale_anim;

    // Allocate the clickable area first (requires &mut ui), then paint over it.
    let orb_rect = egui::Rect::from_center_size(center, egui::vec2(radius * 2.0, radius * 2.0));
    let response = ui.allocate_rect(orb_rect, egui::Sense::click());

    let painter = ui.painter();
    paint_orb(painter, &tk, center, radius, orb_color, time, response.hovered());
    response
}

/// Paint the orb: glow, two breathing rings, body, shimmer and hover ring.
///
/// Rings, shimmer and hover ring use the `text` token with a low alpha so
/// they read on both schemes.
pub(crate) fn paint_orb(
    painter: &egui::Painter,
    tk: &Tokens,
    center: egui::Pos2,
    radius: f32,
    orb_color: Color32,
    time: f64,
    hovered: bool,
) {
    let with_alpha = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
    let ring_alpha = (20.0 * (1.0 + (time * 0.8).sin() as f32) / 2.0) as u8;

    painter.circle_filled(center, radius * 2.2, with_alpha(orb_color, 20));
    painter.circle_stroke(
        center,
        radius * 1.35,
        egui::Stroke::new(1.0_f32, with_alpha(tk.text, ring_alpha)),
    );
    painter.circle_stroke(
        center,
        radius * 1.6,
        egui::Stroke::new(0.5_f32, with_alpha(tk.text, ring_alpha / 2)),
    );
    painter.circle_filled(center, radius, orb_color);

    // Highlight shimmer (top-left).
    let hl = egui::pos2(center.x - radius * 0.2, center.y - radius * 0.2);
    painter.circle_filled(hl, radius * 0.35, with_alpha(Color32::WHITE, 35));

    if hovered {
        painter.circle_stroke(
            center,
            radius + 3.0,
            egui::Stroke::new(1.5_f32, with_alpha(tk.text, 60)),
        );
    }
}

/// Calculate orb color and breathing scale for the current session state.
pub(crate) fn orb_animation(state: &DesktopUiState, time: f64) -> (Color32, f32) {
    let theme = &state.theme;
    match state.session_state.as_str() {
        "Listening" | "UserSpeaking" => {
            let s = 1.0 + 0.06 * (time * 2.5).sin() as f32;
            (theme.orb_listening, s)
        }
        "Recognizing" | "Thinking" => {
            let s = 1.0 + 0.03 * (time * 1.5).sin() as f32 + 0.02 * (time * 2.3).cos() as f32;
            (theme.orb_thinking, s)
        }
        "AssistantSpeaking" => {
            let s = 1.0 + 0.08 * (time * 4.0).sin() as f32;
            (theme.orb_speaking, s)
        }
        _ => {
            let s = 1.0 + 0.03 * (time * 1.0).sin() as f32;
            (theme.orb_idle, s)
        }
    }
}
