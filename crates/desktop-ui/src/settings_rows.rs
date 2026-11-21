//! The row vocabulary of the settings pages: one function per control type of design 2.2.
//!
//! Every row is an `RF / Setting row` (label 15/500, helper line 13 `secondary`, control on the
//! right); text fields and radio lists take the full width below their label. A row that cannot
//! be used yet is the "not available yet" row: the control slot holds `Button / Disabled` and the
//! reason is a visible line under the description, never an opacity-dimmed control (RF 3.1 rule 6).

use egui::{Align, CornerRadius, FontId, Frame, Layout, Margin, Ui};

use crate::fonts;
use crate::settings_command::SettingsCommand;
use crate::settings_data::SettingsData;
use crate::settings_window::SettingsViewState;
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, radius, size, space};
use crate::widgets::{self, ButtonKind, Tone, text, text_medium};

/// Width reserved for a select in a row.
pub(crate) const SELECT_WIDTH: f32 = 200.0;

/// What every page needs to draw and to report.
pub(crate) struct PageCx<'a> {
    /// The active tokens.
    pub tk: &'a Tokens,
    /// The editable values; the app diffs and saves them.
    pub settings: &'a mut UiSettings,
    /// What the app probed.
    pub data: &'a SettingsData,
    /// Requests for the app.
    pub commands: &'a mut Vec<SettingsCommand>,
    /// Drafts and waiting states of the window.
    pub view: &'a mut SettingsViewState,
    /// Name of the active colour theme.
    pub current_theme: &'a str,
    /// A theme chosen this frame, for the app to apply.
    pub pending_theme: &'a mut Option<String>,
    /// Time of this frame in seconds (egui's clock), for waiting states.
    pub now: f64,
}

/// Width of a button with `label` (as `widgets::button_with` lays it out).
pub(crate) fn button_width(ui: &Ui, label: &str) -> f32 {
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::new(size::BUTTON, fonts::medium()),
        egui::Color32::PLACEHOLDER,
    );
    galley.size().x + 2.0 * layout::BUTTON_PAD_X
}

/// Width of a segmented control with `labels`.
pub(crate) fn segmented_width(ui: &Ui, labels: &[&str]) -> f32 {
    let widths: f32 = labels
        .iter()
        .map(|l| {
            ui.painter()
                .layout_no_wrap(
                    (*l).to_owned(),
                    FontId::proportional(size::SECONDARY),
                    egui::Color32::PLACEHOLDER,
                )
                .size()
                .x
                + 2.0 * space::S12
        })
        .sum();
    widths + 2.0 * space::S2 + space::S2 * labels.len().saturating_sub(1) as f32
}

/// Width of a chip with `label`.
pub(crate) fn chip_width(ui: &Ui, label: &str) -> f32 {
    ui.painter()
        .layout_no_wrap(
            label.to_owned(),
            FontId::proportional(size::META),
            egui::Color32::PLACEHOLDER,
        )
        .size()
        .x
        + 16.0
}

/// A switch row. Returns true when toggled.
pub(crate) fn switch_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    value: &mut bool,
    enabled: bool,
) -> bool {
    let mut changed = false;
    widgets::setting_row(
        ui,
        tk,
        label,
        Some(description),
        layout::SWITCH_WIDTH,
        false,
        |ui| {
            changed = widgets::switch_with(ui, tk, value, enabled).changed();
        },
    );
    changed
}

/// A segmented row. Returns the newly chosen index.
pub(crate) fn segmented_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    options: &[String],
    selected: usize,
) -> Option<usize> {
    let refs: Vec<&str> = options.iter().map(String::as_str).collect();
    let width = segmented_width(ui, &refs).min(ui.available_width() * 0.6);
    let mut chosen = None;
    widgets::setting_row(ui, tk, label, Some(description), width, false, |ui| {
        chosen = widgets::segmented(ui, tk, selected, &refs);
    });
    chosen
}

/// A select row. Returns the newly chosen index.
pub(crate) fn select_row(
    ui: &mut Ui,
    tk: &Tokens,
    id: &str,
    label: &str,
    description: &str,
    options: &[String],
    selected: usize,
) -> Option<usize> {
    let current = options.get(selected).map_or("", String::as_str);
    let mut chosen = None;
    widgets::setting_row(
        ui,
        tk,
        label,
        Some(description),
        SELECT_WIDTH,
        false,
        |ui| {
            chosen =
                widgets::select_named(ui, tk, id, SELECT_WIDTH, label, current, options, selected);
        },
    );
    chosen
}

/// A status row: a chip and at most one action. Returns true when the action was clicked.
pub(crate) fn status_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    tone: Tone,
    status: &str,
    action: Option<(&str, bool)>,
) -> bool {
    let actions: Vec<(&str, ButtonKind, bool)> = action
        .into_iter()
        .map(|(a, enabled)| (a, ButtonKind::Secondary, enabled))
        .collect();
    status_row_multi(ui, tk, label, description, tone, status, &actions) == Some(0)
}

/// A status row with several actions, left to right after the chip. Returns the index of the
/// action clicked (a disabled one never is).
pub(crate) fn status_row_multi(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    tone: Tone,
    status: &str,
    actions: &[(&str, ButtonKind, bool)],
) -> Option<usize> {
    let chip_w = chip_width(ui, status);
    let actions_w: f32 = actions
        .iter()
        .map(|(a, _, _)| button_width(ui, a) + space::S8)
        .sum();
    let mut clicked = None;
    widgets::setting_row(
        ui,
        tk,
        label,
        Some(description),
        (chip_w + actions_w).min(ui.available_width() * 0.62),
        false,
        |ui| {
            // Right to left: the last action is the rightmost control, the chip the leftmost.
            for (i, (a, kind, enabled)) in actions.iter().enumerate().rev() {
                let response =
                    widgets::button_with(ui, tk, *kind, a, layout::CONTROL_HEIGHT, *enabled)
                        .accessible_name(format!("{label}: {a}"));
                if *enabled && response.clicked() {
                    clicked = Some(i);
                }
            }
            widgets::chip(ui, tk, tone, status);
        },
    );
    clicked
}

/// An action row: a text and a button. Returns true when clicked.
pub(crate) fn action_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    action: &str,
    kind: ButtonKind,
) -> bool {
    let width = button_width(ui, action);
    let mut clicked = false;
    widgets::setting_row(ui, tk, label, Some(description), width, false, |ui| {
        clicked = widgets::button(ui, tk, kind, action)
            .accessible_name(format!("{label}: {action}"))
            .clicked();
    });
    clicked
}

/// A row whose back end does not exist yet. The control is `Button / Disabled`
/// and the reason (an i18n key) is shown under the description. `value` is the
/// current value, shown in the description, for rows that display one.
pub(crate) fn unavailable_row(
    ui: &mut Ui,
    tk: &Tokens,
    label: &str,
    description: &str,
    reason_key: &str,
    value: Option<&str>,
) {
    let action = prxvc_i18n::t("settings.unavailable");
    let width = button_width(ui, &action);
    let mut text = description.to_owned();
    if let Some(value) = value.filter(|v| !v.is_empty()) {
        text.push('\n');
        text.push_str(&prxvc_i18n::t_with("settings.value_now", &[value]));
    }
    text.push('\n');
    text.push_str(&prxvc_i18n::t(reason_key));
    widgets::setting_row(ui, tk, label, Some(&text), width, false, |ui| {
        widgets::button_with(
            ui,
            tk,
            ButtonKind::Secondary,
            &action,
            layout::CONTROL_HEIGHT,
            false,
        )
        .accessible_name(format!("{label}: {action}"));
    });
}

/// A full-width text field row with a draft, committed on blur when valid.
///
/// Returns `Some(text)` when a valid edit was committed this frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn text_row(
    ui: &mut Ui,
    tk: &Tokens,
    view: &mut SettingsViewState,
    key: &'static str,
    label: &str,
    helper: &str,
    stored: &str,
    validate: impl Fn(&str) -> Result<(), String>,
) -> Option<String> {
    ui.add_space(space::S12);
    let draft = view.drafts.entry(key).or_insert_with(|| stored.to_owned());
    let verdict = validate(draft);
    let shown_helper = match &verdict {
        Err(msg) => msg.clone(),
        Ok(()) => helper.to_owned(),
    };
    let helper = (!shown_helper.is_empty()).then_some(shown_helper.as_str());
    let response = widgets::text_field(ui, tk, label, draft, helper, verdict.is_err());
    let mut committed = None;
    if response.lost_focus() {
        if verdict.is_ok() {
            if draft.as_str() != stored {
                committed = Some(draft.clone());
            }
        } else {
            // Leaving an invalid draft restores the stored value (RF 3.8: the old value stays).
            *draft = stored.to_owned();
        }
    } else if !response.has_focus() && draft.as_str() != stored {
        // The stored value changed elsewhere (a failed save or a reload).
        *draft = stored.to_owned();
    }
    ui.add_space(space::S12);
    committed
}

/// A wrapped notice with a soft tone fill (tip bar anatomy; the text wraps).
pub(crate) fn notice(ui: &mut Ui, tk: &Tokens, tone: Tone, message: &str) {
    let fill = match tone {
        Tone::Neutral => tk.surface,
        Tone::Success => tk.success_bg,
        Tone::Warning => tk.warning_bg,
        Tone::Danger => tk.danger_bg,
    };
    Frame::NONE
        .fill(fill)
        .corner_radius(CornerRadius::same(radius::CARD))
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(egui::Label::new(text(message, size::SECONDARY, tk.text)).wrap());
        });
}

/// A secondary text line (13, `secondary`) that wraps.
pub(crate) fn note(ui: &mut Ui, tk: &Tokens, message: &str) {
    ui.add(egui::Label::new(text(message, size::SECONDARY, tk.secondary)).wrap());
}

/// A line of body text (15/500) that wraps.
pub(crate) fn body_medium(ui: &mut Ui, tk: &Tokens, message: &str) {
    ui.add(egui::Label::new(text_medium(message, size::BODY, tk.text)).wrap());
}

/// Buttons laid out left to right in the page body.
pub(crate) fn button_strip(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = space::S8;
        add(ui);
    });
}

/// The description of a row whose value is saved but not used by the app yet.
pub(crate) fn saved_only(description: &str) -> String {
    format!(
        "{description} {}",
        prxvc_i18n::t("settings.note.saved_only")
    )
}

/// Translate a key.
pub(crate) fn t(key: &str) -> String {
    prxvc_i18n::t(key)
}

/// A radio list of `(label, description)` options. Returns the newly chosen index.
pub(crate) fn radio_row(
    ui: &mut Ui,
    tk: &Tokens,
    id: &str,
    options: &[(String, String)],
    selected: usize,
) -> Option<usize> {
    crate::widgets_radio::radio_list(ui, tk, id, options, selected, true)
}
