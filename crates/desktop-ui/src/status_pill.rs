//! The state pill under the orb: the state label and hint text on a floating surface.

use egui::{CornerRadius, FontId, StrokeKind};

use crate::fonts;
use crate::theme::{Tokens, hairline, radius, size, space};

/// The state pill under the orb: the state label (14) and the hint (12) on a
/// floating-layer surface so the text reads on any desktop behind the
/// transparent window.
pub(crate) struct StatusPill {
    pub(crate) rect: egui::Rect,
    label: std::sync::Arc<egui::Galley>,
    hint: Option<std::sync::Arc<egui::Galley>>,
}

impl StatusPill {
    const PAD_X: f32 = 12.0;
    const PAD_Y: f32 = 8.0;

    pub(crate) fn layout(
        painter: &egui::Painter,
        tk: &Tokens,
        top_center: egui::Pos2,
        label: &str,
        hint: &str,
        max_width: f32,
    ) -> Self {
        let wrap = (max_width - 2.0 * Self::PAD_X).max(60.0);
        let label = painter.layout(
            label.to_owned(),
            FontId::new(size::BUTTON, fonts::medium()),
            tk.text,
            wrap,
        );
        let hint = (!hint.is_empty()).then(|| {
            painter.layout(
                hint.to_owned(),
                FontId::proportional(size::META),
                tk.secondary,
                wrap,
            )
        });
        let inner_w = hint
            .as_ref()
            .map_or(label.size().x, |h| h.size().x.max(label.size().x));
        let inner_h = label.size().y + hint.as_ref().map_or(0.0, |h| h.size().y + space::S4);
        let rect = egui::Rect::from_min_size(
            egui::pos2(top_center.x - inner_w / 2.0 - Self::PAD_X, top_center.y),
            egui::vec2(inner_w + 2.0 * Self::PAD_X, inner_h + 2.0 * Self::PAD_Y),
        );
        Self { rect, label, hint }
    }

    pub(crate) fn paint(self, painter: &egui::Painter, tk: &Tokens) {
        painter.rect(
            self.rect,
            CornerRadius::same(radius::LG),
            tk.overlay,
            hairline(tk.overlay_border),
            StrokeKind::Inside,
        );
        let cx = self.rect.center().x;
        let mut y = self.rect.top() + Self::PAD_Y;
        let label_h = self.label.size().y;
        let label_w = self.label.size().x;
        painter.galley(egui::pos2(cx - label_w / 2.0, y), self.label, tk.text);
        y += label_h + space::S4;
        if let Some(hint) = self.hint {
            painter.galley(egui::pos2(cx - hint.size().x / 2.0, y), hint, tk.secondary);
        }
    }
}
