//! The `Theme` colour presets: the RF theme and the legacy colour variants selectable beside it.

use egui::Color32;

use crate::theme::{self, Scheme, Tokens, mix};

/// Color theme for the UI.
///
/// The RF theme ([`Theme::rf`]) is the default. Its colours come from the
/// design tokens in [`theme`] and follow the system light/dark preference;
/// the other built-in themes are dark colour variants that stay selectable.
#[derive(Debug, Clone)]
pub struct Theme {
    /// Human-readable theme name.
    pub name: &'static str,
    /// Colour scheme in effect (always dark for the legacy colour themes).
    pub scheme: Scheme,
    /// Deep background color.
    pub bg_warm: Color32,
    /// Elevated surface color for cards and panels.
    pub bg_elevated: Color32,
    /// Primary text color.
    pub text_primary: Color32,
    /// Softer text color for secondary content.
    pub text_soft: Color32,
    /// Dim text color for hints and labels.
    pub text_dim: Color32,
    /// Accent / highlight color.
    pub accent: Color32,
    /// Orb color in idle state.
    pub orb_idle: Color32,
    /// Orb color when listening.
    pub orb_listening: Color32,
    /// Orb color when thinking.
    pub orb_thinking: Color32,
    /// Orb color when speaking.
    pub orb_speaking: Color32,
}

impl Theme {
    /// The RF theme (default) in the given scheme.
    ///
    /// The orb uses the standard's own colours: `primary` when idle, the
    /// semantic `success` when listening, `warning` when thinking and the
    /// first chart colour when speaking.
    pub fn rf(scheme: Scheme) -> Self {
        let tk = scheme.tokens();
        Self {
            name: theme::RF_THEME_NAME,
            scheme,
            bg_warm: tk.bg,
            bg_elevated: tk.surface_white,
            text_primary: tk.text,
            text_soft: tk.secondary,
            text_dim: tk.secondary,
            accent: tk.primary,
            orb_idle: tk.primary,
            orb_listening: tk.success,
            orb_thinking: tk.warning,
            orb_speaking: tk.chart_1,
        }
    }

    /// Whether this is the RF theme.
    pub fn is_rf(&self) -> bool {
        self.name == theme::RF_THEME_NAME
    }

    /// The design tokens this theme paints with.
    ///
    /// For the RF theme these are the standard's tokens of [`Self::scheme`];
    /// the legacy colour themes derive an approximate token set from their
    /// own colours so the same widgets work on every theme.
    pub fn tokens(&self) -> Tokens {
        if self.is_rf() {
            self.scheme.tokens()
        } else {
            self.legacy_tokens()
        }
    }

    fn legacy_tokens(&self) -> Tokens {
        let mut tk = theme::DARK;
        let (bg, raised, text) = (self.bg_warm, self.bg_elevated, self.text_primary);
        tk.bg = bg;
        tk.nav = mix(bg, Color32::BLACK, 0.25);
        tk.rail = mix(bg, Color32::BLACK, 0.45);
        tk.surface = raised;
        tk.surface_white = raised;
        tk.overlay = raised;
        tk.input = bg;
        tk.selected = mix(raised, text, 0.08);
        tk.overlay_border = mix(raised, text, 0.12);
        tk.line = mix(raised, text, 0.12);
        tk.line_strong = mix(raised, text, 0.22);
        tk.input_border = self.text_dim;
        tk.control = self.text_dim;
        tk.text = text;
        tk.secondary = self.text_soft;
        tk.primary = self.accent;
        tk.on_primary = bg;
        tk.focus = self.accent;
        tk.success = self.orb_listening;
        tk.success_bg = mix(raised, self.orb_listening, 0.15);
        tk.warning = self.orb_thinking;
        tk.warning_bg = mix(raised, self.orb_thinking, 0.15);
        tk.danger = self.orb_speaking;
        tk.danger_bg = mix(raised, self.orb_speaking, 0.15);
        tk.on_danger = bg;
        tk.chart_1 = self.orb_idle;
        tk
    }

    /// Midnight Purple (formerly the default theme).
    pub fn midnight() -> Self {
        Self {
            name: "Midnight",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(19, 17, 26),
            bg_elevated: Color32::from_rgb(34, 30, 46),
            text_primary: Color32::from_rgb(237, 233, 246),
            text_soft: Color32::from_rgb(168, 158, 194),
            text_dim: Color32::from_rgb(88, 79, 110),
            accent: Color32::from_rgb(167, 139, 250),
            orb_idle: Color32::from_rgb(167, 139, 250),
            orb_listening: Color32::from_rgb(94, 234, 212),
            orb_thinking: Color32::from_rgb(251, 191, 36),
            orb_speaking: Color32::from_rgb(248, 113, 113),
        }
    }

    /// Ocean Blue.
    pub fn ocean() -> Self {
        Self {
            name: "Ocean",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(13, 17, 28),
            bg_elevated: Color32::from_rgb(22, 30, 48),
            text_primary: Color32::from_rgb(220, 233, 250),
            text_soft: Color32::from_rgb(140, 165, 200),
            text_dim: Color32::from_rgb(70, 90, 120),
            accent: Color32::from_rgb(96, 165, 250),
            orb_idle: Color32::from_rgb(96, 165, 250),
            orb_listening: Color32::from_rgb(52, 211, 153),
            orb_thinking: Color32::from_rgb(251, 191, 36),
            orb_speaking: Color32::from_rgb(251, 146, 60),
        }
    }

    /// Rose Gold.
    pub fn rose() -> Self {
        Self {
            name: "Rose",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(24, 16, 20),
            bg_elevated: Color32::from_rgb(42, 28, 35),
            text_primary: Color32::from_rgb(248, 232, 238),
            text_soft: Color32::from_rgb(200, 160, 180),
            text_dim: Color32::from_rgb(120, 80, 100),
            accent: Color32::from_rgb(244, 114, 182),
            orb_idle: Color32::from_rgb(244, 114, 182),
            orb_listening: Color32::from_rgb(168, 230, 207),
            orb_thinking: Color32::from_rgb(252, 211, 77),
            orb_speaking: Color32::from_rgb(239, 68, 68),
        }
    }

    /// Forest Green.
    pub fn forest() -> Self {
        Self {
            name: "Forest",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(12, 20, 16),
            bg_elevated: Color32::from_rgb(24, 38, 30),
            text_primary: Color32::from_rgb(220, 243, 230),
            text_soft: Color32::from_rgb(150, 195, 170),
            text_dim: Color32::from_rgb(75, 110, 90),
            accent: Color32::from_rgb(74, 222, 128),
            orb_idle: Color32::from_rgb(74, 222, 128),
            orb_listening: Color32::from_rgb(147, 197, 253),
            orb_thinking: Color32::from_rgb(253, 224, 71),
            orb_speaking: Color32::from_rgb(248, 113, 113),
        }
    }

    /// Amber Ember.
    pub fn ember() -> Self {
        Self {
            name: "Ember",
            scheme: Scheme::Dark,
            bg_warm: Color32::from_rgb(22, 16, 10),
            bg_elevated: Color32::from_rgb(40, 30, 20),
            text_primary: Color32::from_rgb(250, 240, 225),
            text_soft: Color32::from_rgb(200, 175, 145),
            text_dim: Color32::from_rgb(120, 95, 70),
            accent: Color32::from_rgb(251, 146, 60),
            orb_idle: Color32::from_rgb(251, 146, 60),
            orb_listening: Color32::from_rgb(94, 234, 212),
            orb_thinking: Color32::from_rgb(253, 224, 71),
            orb_speaking: Color32::from_rgb(239, 68, 68),
        }
    }

    /// All available built-in themes. The RF theme comes first.
    pub fn all() -> Vec<Theme> {
        vec![
            Self::rf(Scheme::Light),
            Self::midnight(),
            Self::ocean(),
            Self::rose(),
            Self::forest(),
            Self::ember(),
        ]
    }
}

impl Default for Theme {
    /// The RF theme. Its scheme is resolved against the system preference on
    /// the first rendered frame.
    fn default() -> Self {
        Self::rf(Scheme::Light)
    }
}
