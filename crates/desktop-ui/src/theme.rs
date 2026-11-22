//! RF design tokens and the egui theme built from them.
//!
//! Every value in this module is transcribed from the RF design standard
//! (colour system, typography, spacing, radii, borders, shadows and density).
//! Nothing is read from external files at build time or at runtime. Each
//! colour row names the standard's token (`rf-<name>`) next to its light and
//! dark value; values marked "adopted" are the standard's own adopted
//! deviations from its design file (field boundary, success green), taken as
//! the standard states them.
//!
//! The module exposes:
//! * [`Tokens`] with a [`LIGHT`] and a [`DARK`] constant, for custom-painted
//!   pieces and the widgets in [`crate::widgets`];
//! * size, radius, spacing and layout constants ([`size`], [`radius`],
//!   [`space`], [`layout`], [`border`]);
//! * [`apply`] / [`ensure_installed`], which set the stock egui widgets'
//!   look (visuals, text styles, spacing) for both schemes at once and let
//!   egui follow the system light/dark preference.

use crate::fonts;
use egui::{
    Color32, Context, CornerRadius, FontFamily, FontId, Margin, Stroke, TextStyle,
    Theme as EguiTheme, ThemePreference, epaint::Shadow,
};

// ---------------------------------------------------------------------------
// Colour tokens
// ---------------------------------------------------------------------------

/// Parse `#RRGGBB` or `#RRGGBBAA` at compile time.
///
/// Translucent colours are returned premultiplied. Only black is translucent
/// in the standard (scrim, shadow), so premultiplying is exact.
const fn hex(s: &str) -> Color32 {
    const fn digit(b: u8) -> u8 {
        match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => panic!("invalid hex digit in colour literal"),
        }
    }
    const fn byte(b: &[u8], i: usize) -> u8 {
        digit(b[i]) * 16 + digit(b[i + 1])
    }
    let b = s.as_bytes();
    let (r, g, bl) = (byte(b, 1), byte(b, 3), byte(b, 5));
    if b.len() == 9 {
        let a = byte(b, 7) as u16;
        Color32::from_rgba_premultiplied(
            (r as u16 * a / 255) as u8,
            (g as u16 * a / 255) as u8,
            (bl as u16 * a / 255) as u8,
            a as u8,
        )
    } else {
        Color32::from_rgb(r, g, bl)
    }
}

macro_rules! tokens {
    ($( $(#[$doc:meta])* $field:ident = $name:literal : $light:literal / $dark:literal; )*) => {
        /// The 50 themed colour tokens of the RF standard for one scheme.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct Tokens {
            $( $(#[$doc])* pub $field: Color32, )*
        }

        /// Light scheme.
        pub const LIGHT: Tokens = Tokens { $( $field: hex($light), )* };
        /// Dark scheme.
        pub const DARK: Tokens = Tokens { $( $field: hex($dark), )* };

        impl Tokens {
            /// Token names exactly as the standard spells them (without the `rf-` prefix).
            pub const NAMES: &'static [&'static str] = &[ $( $name, )* ];

            /// All tokens as `(standard name, colour)` pairs, in declaration order.
            pub fn entries(&self) -> Vec<(&'static str, Color32)> {
                vec![ $( ($name, self.$field), )* ]
            }
        }
    };
}

tokens! {
    // Surfaces
    /// App canvas, main content ground.
    bg = "bg": "#FCFBF9" / "#171716";
    /// Secondary list column, settings navigation.
    nav = "nav": "#F5F4F1" / "#121211";
    /// Primary navigation rail.
    rail = "rail": "#EDECE8" / "#0D0D0C";
    /// Reserved for the active or hovered rail item.
    rail_active = "rail-active": "#E2E1DE" / "#222221";
    /// Filled neutral controls: chip, tip bar, segmented track, disabled button.
    surface = "surface": "#EFEFEF" / "#212121";
    /// Raised card, secondary button fill, file tile.
    surface_white = "surface-white": "#FFFFFF" / "#1E1E1E";
    /// Selected row, selected navigation item.
    selected = "selected": "#E6E5E2" / "#222221";
    /// Floating layers: menu, popover, dialog.
    overlay = "overlay": "#FFFFFF" / "#1E1E1E";
    /// 1px outline of floating layers.
    overlay_border = "overlay-border": "#E2E2E2" / "#303030";
    /// Text field ground.
    input = "input": "#FFFFFF" / "#1C1C1C";
    /// 1px outline of a text field (adopted: takes the `control` value).
    input_border = "input-border": "#8E8E8E" / "#7A7A7A";
    // Text and icons
    /// Primary text and default icons.
    text = "text": "#161616" / "#EBEBEB";
    /// Supporting text, meta, placeholder.
    secondary = "secondary": "#585858" / "#9E9E9E";
    /// Functional boundary and disabled foreground. Never readable text.
    control = "control": "#8E8E8E" / "#7A7A7A";
    // Borders and dividers
    /// Dividers, card outlines.
    line = "line": "#E6E6E6" / "#262626";
    /// Stronger outline: switch off-track, select-like boxes.
    line_strong = "line-strong": "#D4D4D4" / "#3A3A3A";
    /// Reserved, unused by the standard.
    line_soft = "line-soft": "#EFEFEF" / "#1E1E1E";
    /// Reserved, unused by the standard.
    line_nav = "line-nav": "#E2E2E1" / "#1E1E1E";
    // Primary, accent and focus
    /// Primary action fill; checked switch.
    primary = "primary": "#222224" / "#EBEBEB";
    /// Text and glyph on `primary`.
    on_primary = "on-primary": "#FFFFFF" / "#121211";
    /// Theme accent (equals `text` in the default theme).
    accent = "accent": "#161616" / "#EBEBEB";
    /// Text and glyph on `accent`.
    on_accent = "on-accent": "#FFFFFF" / "#121211";
    /// Keyboard focus ring.
    focus = "focus": "#222224" / "#C8C8C8";
    /// Unread count.
    badge = "badge": "#222224" / "#EBEBEB";
    /// Text on `badge`.
    on_badge = "on-badge": "#FFFFFF" / "#121211";
    /// Tooltip ground.
    tooltip = "tooltip": "#222224" / "#EBEBEB";
    /// Tooltip text.
    on_tooltip = "on-tooltip": "#FFFFFF" / "#121211";
    // Overlays
    /// Modal backdrop (black 30% light, 55% dark).
    scrim = "scrim": "#0000004D" / "#0000008C";
    /// Shadow colour (black 12% light, 48% dark).
    shadow = "shadow": "#0000001F" / "#0000007A";
    // Semantic colours
    /// Success label, glyph, connection dot (adopted light value).
    success = "success": "#2B782F" / "#5FD27A";
    /// Success soft fill.
    success_bg = "success-bg": "#E8F2E9" / "#14251A";
    /// Glyph on solid success.
    on_success = "on-success": "#FFFFFF" / "#121211";
    /// Warning label and glyph.
    warning = "warning": "#9A5800" / "#F2B94B";
    /// Warning soft fill.
    warning_bg = "warning-bg": "#FBF0DE" / "#2B2210";
    /// Destructive button fill, danger label, invalid field border.
    danger = "danger": "#C62828" / "#FF6B6B";
    /// Danger soft fill, quiet destructive button, invalid field fill.
    danger_bg = "danger-bg": "#FCEBEB" / "#2E1818";
    /// Text on solid danger.
    on_danger = "on-danger": "#FFFFFF" / "#121211";
    // Identity
    /// Person avatar ground.
    avatar_person = "avatar-person": "#E7E7E7" / "#2A2A2A";
    /// Initial on person avatar.
    on_avatar_person = "on-avatar-person": "#161616" / "#EBEBEB";
    /// Agent avatar ground.
    agent_av = "agent-av": "#222224" / "#EBEBEB";
    /// Initial on agent avatar.
    on_agent_av = "on-agent-av": "#FFFFFF" / "#121211";
    // Messaging
    /// Incoming message bubble.
    incoming = "incoming": "#EFEFEF" / "#242424";
    /// Time line inside an incoming bubble.
    incoming_meta = "incoming-meta": "#5C5C5C" / "#A2A2A2";
    /// Quote block inside an incoming bubble.
    incoming_quote = "incoming-quote": "#E3E3E3" / "#2E2E2E";
    /// Own message bubble.
    outgoing = "outgoing": "#222224" / "#3A3A3C";
    /// Text in an own bubble.
    outgoing_text = "outgoing-text": "#F6F6F6" / "#F7F7F7";
    /// Time line inside an own bubble.
    outgoing_meta = "outgoing-meta": "#ABABAB" / "#C4C4C4";
    /// Quote block inside an own bubble.
    outgoing_quote = "outgoing-quote": "#343436" / "#4A4A4C";
    // Data
    /// Chart series 1.
    chart_1 = "chart-1": "#2A78D6" / "#3987E5";
    /// Chart series 2.
    chart_2 = "chart-2": "#EB6834" / "#D95926";
}

/// Light or dark colour scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// Light scheme (the one the standard presents first).
    Light,
    /// Dark scheme.
    Dark,
}

impl Scheme {
    /// The scheme for an egui theme.
    pub fn from_egui(theme: EguiTheme) -> Self {
        match theme {
            EguiTheme::Light => Scheme::Light,
            EguiTheme::Dark => Scheme::Dark,
        }
    }

    /// The matching egui theme.
    pub fn to_egui(self) -> EguiTheme {
        match self {
            Scheme::Light => EguiTheme::Light,
            Scheme::Dark => EguiTheme::Dark,
        }
    }

    /// The token set of this scheme.
    pub fn tokens(self) -> Tokens {
        match self {
            Scheme::Light => LIGHT,
            Scheme::Dark => DARK,
        }
    }
}

/// The tokens of the scheme egui currently uses for `ctx`.
pub fn tokens_for(ctx: &Context) -> Tokens {
    Scheme::from_egui(ctx.theme()).tokens()
}

/// Linear blend of two colours in sRGB space (`t` = 0 gives `a`, 1 gives `b`).
///
/// Used only for hover and pressed fills of filled buttons, which the
/// standard does not specify (see the deviation notes in the report).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

fn channel_luminance(c: u8) -> f64 {
    let s = c as f64 / 255.0;
    if s <= 0.03928 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// WCAG 2.x relative luminance of an opaque colour.
pub fn luminance(c: Color32) -> f64 {
    0.2126 * channel_luminance(c.r())
        + 0.7152 * channel_luminance(c.g())
        + 0.0722 * channel_luminance(c.b())
}

/// WCAG 2.x contrast ratio between two opaque colours (1.0 to 21.0).
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Minimum contrast for normal text (WCAG AA), as the standard requires.
pub const MIN_TEXT_CONTRAST: f64 = 4.5;
/// Minimum contrast for large text, meaningful icons and control boundaries.
pub const MIN_NON_TEXT_CONTRAST: f64 = 3.0;

// ---------------------------------------------------------------------------
// Type scale (standard section "Typography": size tokens)
// ---------------------------------------------------------------------------

/// Font sizes in px. Names are the standard's size tokens.
pub mod size {
    /// `caption`: rail label, time in a bubble, unread count.
    pub const CAPTION: f32 = 11.0;
    /// `meta`: chip label, helper text, timestamps.
    pub const META: f32 = 12.0;
    /// `secondary`: second line of a row, field label, tip-bar text.
    pub const SECONDARY: f32 = 13.0;
    /// `button`: button label, menu item label.
    pub const BUTTON: f32 = 14.0;
    /// `body`: row title, field value, dialog body.
    pub const BODY: f32 = 15.0;
    /// `subheading`: card title, section heading.
    pub const SUBHEADING: f32 = 17.0;
    /// `title`: dialog title.
    pub const TITLE: f32 = 20.0;
    /// `page`: page title.
    pub const PAGE: f32 = 24.0;
    /// `metric`: one large figure.
    pub const METRIC: f32 = 32.0;
}

// ---------------------------------------------------------------------------
// Radii (standard section "Radii")
// ---------------------------------------------------------------------------

/// Corner radii in px. Names are the standard's radius tokens.
pub mod radius {
    /// `xs`: checkbox, segmented segment, settings-navigation item.
    pub const XS: u8 = 4;
    /// `sm`: navigation item, list row, menu item, segmented track.
    pub const SM: u8 = 6;
    /// `md`: scope block, quote, attachment, unread badge.
    pub const MD: u8 = 8;
    /// `lg`: composer, formatted-content card.
    pub const LG: u8 = 12;
    /// `xl`: send button inside the composer.
    pub const XL: u8 = 16;
    /// `button`: every button and icon button.
    pub const BUTTON: u8 = 4;
    /// `input`: text field, search field, value box.
    pub const INPUT: u8 = 4;
    /// `chip`: status chip, date pill.
    pub const CHIP: u8 = 4;
    /// `card`: cards, tip bars, security warnings.
    pub const CARD: u8 = 8;
    /// `menu`: floating menus and popovers.
    pub const MENU: u8 = 12;
    /// `dialog`: dialogs.
    pub const DIALOG: u8 = 12;
    /// `bubble`: message bubble.
    pub const BUBBLE: u8 = 12;
    /// `bubble-joined`: the corner of a grouped bubble that touches its neighbour.
    pub const BUBBLE_JOINED: u8 = 4;
    /// `full`: switch and radio (use as a pill; clamp to half the side).
    pub const FULL: u8 = 255;
}

// ---------------------------------------------------------------------------
// Spacing, borders, layout (standard sections "Spacing and layout grid",
// "Border widths", "Density")
// ---------------------------------------------------------------------------

/// The spacing scale in px: 2, 4, 6, 8, 12, 16, 20, 24, 32.
pub mod space {
    /// Optical value inside components.
    pub const S2: f32 = 2.0;
    /// Tight stacks, title to its secondary line.
    pub const S4: f32 = 4.0;
    /// Field label to field.
    pub const S6: f32 = 6.0;
    /// Default gap between sibling controls.
    pub const S8: f32 = 8.0;
    /// Items inside a card, setting row padding.
    pub const S12: f32 = 12.0;
    /// Sections inside a dialog or panel.
    pub const S16: f32 = 16.0;
    /// Card content, settings groups.
    pub const S20: f32 = 20.0;
    /// Page sections, dialog padding.
    pub const S24: f32 = 24.0;
    /// Page body.
    pub const S32: f32 = 32.0;
    /// All steps, ascending.
    pub const SCALE: [f32; 9] = [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0];
}

/// Border widths in px.
pub mod border {
    /// Cards, fields, floating layers, dividers.
    pub const DEFAULT: f32 = 1.0;
    /// Checkbox and radio.
    pub const CONTROL: f32 = 1.5;
    /// Focus ring.
    pub const FOCUS: f32 = 2.0;
}

/// Control sizes in px.
pub mod layout {
    /// Button, segmented control, icon button height.
    pub const CONTROL_HEIGHT: f32 = 32.0;
    /// Smallest button height (inline action inside a dense row).
    pub const CONTROL_HEIGHT_SMALL: f32 = 24.0;
    /// Text field box height.
    pub const FIELD_HEIGHT: f32 = 36.0;
    /// Icon button side.
    pub const ICON_BUTTON: f32 = 32.0;
    /// Menu item height (`menu-item-height`).
    pub const MENU_ITEM_HEIGHT: f32 = 38.0;
    /// Switch track width.
    pub const SWITCH_WIDTH: f32 = 36.0;
    /// Switch track height.
    pub const SWITCH_HEIGHT: f32 = 20.0;
    /// Switch knob diameter.
    pub const SWITCH_KNOB: f32 = 16.0;
    /// Button horizontal padding.
    pub const BUTTON_PAD_X: f32 = 12.0;
    /// Dialog width in the standard.
    pub const DIALOG_WIDTH: f32 = 480.0;
    /// Reading and form column width in the standard.
    pub const READING_COLUMN: f32 = 760.0;
    /// Text field default width in the standard.
    pub const FIELD_WIDTH: f32 = 320.0;
    /// Dialog padding.
    pub const DIALOG_PADDING: f32 = 24.0;
    /// `bubble-max`: widest message bubble.
    pub const BUBBLE_MAX: f32 = 600.0;
    /// Bubble padding, vertical (the standard's 8 / 12).
    pub const BUBBLE_PAD_Y: i8 = 8;
    /// Bubble padding, horizontal.
    pub const BUBBLE_PAD_X: i8 = 12;
}

/// `shadow-overlay`: 0 8px 24px in the `shadow` colour.
pub fn shadow_overlay(tk: &Tokens) -> Shadow {
    Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: tk.shadow,
    }
}

/// A 1px stroke in `color`.
pub fn hairline(color: Color32) -> Stroke {
    Stroke::new(border::DEFAULT, color)
}

// ---------------------------------------------------------------------------
// egui style
// ---------------------------------------------------------------------------

/// Build the egui `Style` for a token set.
pub fn style_for(tk: &Tokens, dark: bool) -> egui::Style {
    // Text styles: the closest egui text styles to the standard's scale.
    let text_styles = [
        (
            TextStyle::Small,
            FontId::new(size::META, FontFamily::Proportional),
        ),
        (
            TextStyle::Body,
            FontId::new(size::BODY, FontFamily::Proportional),
        ),
        // Weight 500 for button labels, weight 600 for headings.
        (
            TextStyle::Button,
            FontId::new(size::BUTTON, fonts::medium()),
        ),
        (
            TextStyle::Heading,
            FontId::new(size::TITLE, fonts::semibold()),
        ),
        (
            TextStyle::Monospace,
            FontId::new(size::SECONDARY, FontFamily::Monospace),
        ),
    ]
    .into();
    let mut style = egui::Style {
        text_styles,
        ..egui::Style::default()
    };

    // Spacing: 8px default gap, 32px controls, 12px button padding.
    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(space::S8, space::S8);
    sp.button_padding = egui::vec2(layout::BUTTON_PAD_X, space::S6);
    sp.interact_size = egui::vec2(layout::CONTROL_HEIGHT, layout::CONTROL_HEIGHT);
    sp.window_margin = Margin::same(layout::DIALOG_PADDING as i8);
    sp.menu_margin = Margin::same(4);
    sp.icon_width = 18.0;
    sp.icon_width_inner = 12.0;
    sp.text_edit_width = layout::FIELD_WIDTH;
    sp.combo_width = 120.0;
    sp.combo_height = 320.0;
    // Defaults that changed after egui 0.31 and are not part of the RF
    // standard: pinned to their 0.31 values so the look does not move with
    // the egui version.
    sp.scroll.fade.strength = 0.0;
    style.animation_time = 1.0 / 12.0;

    let v = &mut style.visuals;
    v.dark_mode = dark;
    v.override_text_color = None;

    let button_radius = CornerRadius::same(radius::BUTTON);

    // Non-interactive: labels, frames, separators.
    v.widgets.noninteractive.bg_fill = tk.bg;
    v.widgets.noninteractive.weak_bg_fill = tk.bg;
    v.widgets.noninteractive.bg_stroke = hairline(tk.line);
    v.widgets.noninteractive.fg_stroke = Stroke::new(border::DEFAULT, tk.text);
    v.widgets.noninteractive.corner_radius = button_radius;
    v.widgets.noninteractive.expansion = 0.0;

    // Inactive: a stock button is the Secondary button (white fill, 1px text border).
    v.widgets.inactive.bg_fill = tk.surface_white;
    v.widgets.inactive.weak_bg_fill = tk.surface_white;
    v.widgets.inactive.bg_stroke = hairline(tk.text);
    v.widgets.inactive.fg_stroke = Stroke::new(border::DEFAULT, tk.text);
    v.widgets.inactive.corner_radius = button_radius;
    v.widgets.inactive.expansion = 0.0;

    // Hover: `surface` on raised grounds (the standard's proposed hover).
    v.widgets.hovered.bg_fill = tk.surface;
    v.widgets.hovered.weak_bg_fill = tk.surface;
    v.widgets.hovered.bg_stroke = hairline(tk.text);
    v.widgets.hovered.fg_stroke = Stroke::new(border::DEFAULT, tk.text);
    v.widgets.hovered.corner_radius = button_radius;
    v.widgets.hovered.expansion = 0.0;

    // Pressed: `selected` (the standard's proposed pressed fill).
    v.widgets.active.bg_fill = tk.selected;
    v.widgets.active.weak_bg_fill = tk.selected;
    v.widgets.active.bg_stroke = hairline(tk.text);
    v.widgets.active.fg_stroke = Stroke::new(border::DEFAULT, tk.text);
    v.widgets.active.corner_radius = button_radius;
    v.widgets.active.expansion = 0.0;

    // Open (menus, combo boxes): same as hover.
    v.widgets.open = v.widgets.hovered;

    // Selection and focus: the 2px focus ring doubles as the text-edit focus
    // border; the selection fill is the `selected` ground.
    v.selection.bg_fill = tk.selected;
    v.selection.stroke = Stroke::new(border::FOCUS, tk.focus);
    v.text_cursor.stroke = Stroke::new(border::FOCUS, tk.text);

    v.hyperlink_color = tk.text;
    v.faint_bg_color = tk.surface;
    v.extreme_bg_color = tk.input;
    v.code_bg_color = tk.surface;
    v.warn_fg_color = tk.warning;
    v.error_fg_color = tk.danger;

    // Floating layers: `overlay` fill, 1px `overlay-border`, `shadow-overlay`.
    v.window_fill = tk.overlay;
    v.window_stroke = hairline(tk.overlay_border);
    v.window_corner_radius = CornerRadius::same(radius::DIALOG);
    v.window_shadow = shadow_overlay(tk);
    v.menu_corner_radius = CornerRadius::same(radius::MENU);
    v.popup_shadow = shadow_overlay(tk);
    v.panel_fill = tk.bg;

    v.striped = false;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.interact_cursor = None;

    style
}

/// Name of the RF theme in [`crate::Theme`].
pub const RF_THEME_NAME: &str = "RF";

/// Make egui follow the system light/dark preference with the RF styles for
/// both schemes. If the system preference is unknown, the light scheme (the
/// one the standard presents first) is used.
pub fn install(ctx: &Context) {
    // The text styles refer to the weight families: make sure they exist.
    fonts::ensure_installed(ctx);
    ctx.options_mut(|o| {
        o.theme_preference = ThemePreference::System;
        o.fallback_theme = EguiTheme::Light;
    });
    ctx.set_style_of(EguiTheme::Light, style_for(&LIGHT, false));
    ctx.set_style_of(EguiTheme::Dark, style_for(&DARK, true));
}

/// Apply `theme` to `ctx`.
///
/// The RF theme installs both schemes and follows the system preference. A
/// legacy colour theme pins egui to the dark scheme and styles it from the
/// theme's own colours.
pub fn apply(ctx: &Context, theme: &crate::Theme) {
    fonts::ensure_installed(ctx);
    if theme.is_rf() {
        install(ctx);
    } else {
        ctx.set_theme(EguiTheme::Dark);
        ctx.set_style_of(EguiTheme::Dark, style_for(&theme.tokens(), true));
    }
}

/// Apply `theme` to `ctx` unless it is already the applied theme.
///
/// Called every frame by the app entry point; the check is a single lookup.
pub fn ensure_installed(ctx: &Context, theme: &crate::Theme) {
    let id = egui::Id::unique("prxvc_applied_theme");
    let applied: Option<String> = ctx.data(|d| d.get_temp(id));
    if applied.as_deref() != Some(theme.name) {
        apply(ctx, theme);
        ctx.data_mut(|d| d.insert_temp(id, theme.name.to_owned()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_has_all_fifty_themed_tokens() {
        assert_eq!(Tokens::NAMES.len(), 50);
        let mut names = Tokens::NAMES.to_vec();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 50, "token names must be unique");
        assert_eq!(LIGHT.entries().len(), 50);
        assert_eq!(DARK.entries().len(), 50);
    }

    #[test]
    fn every_token_is_defined_for_both_schemes_and_differs_where_expected() {
        // Every entry exists for both schemes by construction; additionally the
        // grounds must differ between schemes so the dark set is not a copy.
        for ((name, l), (_, d)) in LIGHT.entries().into_iter().zip(DARK.entries()) {
            assert!(!name.is_empty());
            if [
                "bg",
                "nav",
                "rail",
                "text",
                "secondary",
                "primary",
                "surface",
            ]
            .contains(&name)
            {
                assert_ne!(l, d, "{name} must differ between schemes");
            }
        }
    }

    #[test]
    fn spot_check_transcribed_values() {
        assert_eq!(LIGHT.bg, Color32::from_rgb(0xFC, 0xFB, 0xF9));
        assert_eq!(DARK.bg, Color32::from_rgb(0x17, 0x17, 0x16));
        assert_eq!(LIGHT.primary, Color32::from_rgb(0x22, 0x22, 0x24));
        assert_eq!(DARK.primary, Color32::from_rgb(0xEB, 0xEB, 0xEB));
        // Adopted values.
        assert_eq!(LIGHT.input_border, Color32::from_rgb(0x8E, 0x8E, 0x8E));
        assert_eq!(LIGHT.success, Color32::from_rgb(0x2B, 0x78, 0x2F));
        // Translucent tokens are black with the stated alpha.
        assert_eq!(LIGHT.scrim.a(), 0x4D);
        assert_eq!(DARK.scrim.a(), 0x8C);
        assert_eq!(LIGHT.shadow.a(), 0x1F);
        assert_eq!(DARK.shadow.a(), 0x7A);
    }

    #[test]
    fn spacing_scale_is_the_standards() {
        assert_eq!(
            space::SCALE,
            [2.0, 4.0, 6.0, 8.0, 12.0, 16.0, 20.0, 24.0, 32.0]
        );
    }

    #[test]
    fn contrast_of_a_known_pair() {
        let white = Color32::WHITE;
        let black = Color32::BLACK;
        assert!((contrast_ratio(white, black) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
    }

    /// Body-text pairs the client uses, for both schemes, against the
    /// standard's rule: 4.5:1 for normal text.
    #[test]
    fn body_text_pairs_meet_the_text_contrast_rule() {
        for (scheme, tk) in [("light", LIGHT), ("dark", DARK)] {
            let pairs = [
                ("text on bg", tk.text, tk.bg),
                ("text on nav", tk.text, tk.nav),
                ("text on surface-white", tk.text, tk.surface_white),
                ("text on surface", tk.text, tk.surface),
                ("text on selected", tk.text, tk.selected),
                ("text on overlay", tk.text, tk.overlay),
                ("text on input", tk.text, tk.input),
                ("secondary on bg", tk.secondary, tk.bg),
                ("secondary on nav", tk.secondary, tk.nav),
                ("secondary on surface-white", tk.secondary, tk.surface_white),
                ("secondary on surface", tk.secondary, tk.surface),
                ("secondary on selected", tk.secondary, tk.selected),
                ("secondary on overlay", tk.secondary, tk.overlay),
                ("secondary on input", tk.secondary, tk.input),
                ("on-primary on primary", tk.on_primary, tk.primary),
                ("on-danger on danger", tk.on_danger, tk.danger),
                ("danger on danger-bg", tk.danger, tk.danger_bg),
                ("danger on overlay", tk.danger, tk.overlay),
                ("success on success-bg", tk.success, tk.success_bg),
                ("success on overlay", tk.success, tk.overlay),
                ("warning on warning-bg", tk.warning, tk.warning_bg),
                ("warning on overlay", tk.warning, tk.overlay),
                ("text on danger-bg", tk.text, tk.danger_bg),
                ("text on warning-bg", tk.text, tk.warning_bg),
                ("text on success-bg", tk.text, tk.success_bg),
            ];
            for (name, fg, bg) in pairs {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= MIN_TEXT_CONTRAST,
                    "{scheme}: {name} has contrast {ratio:.2}, needs {MIN_TEXT_CONTRAST}"
                );
            }
        }
    }

    /// Boundaries of controls and the focus ring: 3:1 against their ground.
    #[test]
    fn control_boundaries_and_focus_meet_the_non_text_rule() {
        for (scheme, tk) in [("light", LIGHT), ("dark", DARK)] {
            let pairs = [
                ("input-border on input", tk.input_border, tk.input),
                ("input-border on bg", tk.input_border, tk.bg),
                ("control on input", tk.control, tk.input),
                ("control on surface-white", tk.control, tk.surface_white),
                ("focus on bg", tk.focus, tk.bg),
                ("focus on overlay", tk.focus, tk.overlay),
                ("focus on surface-white", tk.focus, tk.surface_white),
                ("primary on overlay (switch on)", tk.primary, tk.overlay),
                ("primary on bg (switch on)", tk.primary, tk.bg),
            ];
            for (name, fg, bg) in pairs {
                let ratio = contrast_ratio(fg, bg);
                assert!(
                    ratio >= MIN_NON_TEXT_CONTRAST,
                    "{scheme}: {name} has contrast {ratio:.2}, needs {MIN_NON_TEXT_CONTRAST}"
                );
            }
        }
    }

    #[test]
    fn applying_the_rf_theme_sets_both_scheme_styles() {
        let ctx = Context::default();
        let theme = crate::Theme::rf(Scheme::Light);
        apply(&ctx, &theme);

        ctx.set_theme(EguiTheme::Light);
        let light = ctx.global_style();
        assert!(!light.visuals.dark_mode);
        assert_eq!(light.visuals.window_fill, LIGHT.overlay);
        assert_eq!(light.visuals.panel_fill, LIGHT.bg);
        assert_eq!(light.visuals.extreme_bg_color, LIGHT.input);
        assert_eq!(light.visuals.selection.stroke.color, LIGHT.focus);
        assert_eq!(light.visuals.selection.stroke.width, border::FOCUS);
        assert_eq!(
            light.visuals.widgets.inactive.corner_radius,
            CornerRadius::same(4)
        );
        assert_eq!(light.visuals.window_corner_radius, CornerRadius::same(12));
        assert_eq!(light.spacing.interact_size.y, layout::CONTROL_HEIGHT);

        ctx.set_theme(EguiTheme::Dark);
        let dark = ctx.global_style();
        assert!(dark.visuals.dark_mode);
        assert_eq!(dark.visuals.window_fill, DARK.overlay);
        assert_eq!(dark.visuals.panel_fill, DARK.bg);
        assert_eq!(dark.visuals.window_shadow.color, DARK.shadow);
    }

    #[test]
    fn rf_theme_follows_the_system_and_falls_back_to_light() {
        let ctx = Context::default();
        install(&ctx);
        assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::System);
        assert_eq!(ctx.options(|o| o.fallback_theme), EguiTheme::Light);
        // No system theme known in a bare context: the fallback applies.
        assert_eq!(ctx.theme(), EguiTheme::Light);
    }

    #[test]
    fn ensure_installed_applies_once_per_theme_name() {
        let ctx = Context::default();
        let rf = crate::Theme::rf(Scheme::Light);
        ensure_installed(&ctx, &rf);
        ctx.set_theme(EguiTheme::Dark);
        // Already applied: the pinned scheme is not overwritten.
        ensure_installed(&ctx, &rf);
        assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::Dark);
        // A different theme is applied.
        ensure_installed(&ctx, &crate::Theme::midnight());
        assert_eq!(ctx.options(|o| o.theme_preference), ThemePreference::Dark);
        assert_eq!(
            ctx.global_style().visuals.window_fill,
            crate::Theme::midnight().tokens().overlay
        );
    }
}
