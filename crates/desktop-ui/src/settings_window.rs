//! The settings window's layout and navigation: the section list or selector, the page header and the scrolling content column.
//!
//! Below [`WIDE_MIN_WIDTH`] the window is one column with the section list collapsed into a
//! select at the top; from there up it is the RF settings mode: a section list of
//! [`NAV_WIDTH`] beside a content column of at most [`COLUMN_MAX`] (design 2.1a, RF 3.12). The
//! pages themselves are in `settings_view` and the `settings_*` files it names.

use std::collections::HashMap;

use egui::{Align, Layout, Ui, vec2};

use crate::settings_command::SettingsCommand;
use crate::settings_data::{SettingsData, SettingsNotice};
use crate::settings_rows::{PageCx, notice};
use crate::settings_section::SettingsSection;
use crate::settings_view;
use crate::shared::NetworkStatus;
use crate::state::UiSettings;
use crate::theme::{Tokens, size, space};
use crate::tool_window::WINDOW_PADDING;
use crate::widgets::{self, Tone, text};

/// Window width from which the section list sits beside the content.
pub const WIDE_MIN_WIDTH: f32 = 900.0;
/// Width of the section list in the wide layout.
pub const NAV_WIDTH: f32 = 264.0;
/// Widest the content column grows.
pub const COLUMN_MAX: f32 = 760.0;
/// Seconds between re-checks while a permission waits for System Settings.
pub const WAITING_RECHECK_SECONDS: f64 = 2.0;

/// Navigation state and drafts of the settings window.
#[derive(Debug, Clone, Default)]
pub struct SettingsViewState {
    /// The page shown.
    pub section: SettingsSection,
    /// Text being edited in a field, keyed by field, until it is committed on blur.
    pub(crate) drafts: HashMap<&'static str, String>,
    /// Host being typed for the offline allow list.
    pub(crate) host_draft: String,
    /// When the user was sent to System Settings for the microphone.
    pub(crate) mic_waiting_since: Option<f64>,
    /// When the user was sent to System Settings for call audio.
    pub(crate) call_audio_waiting_since: Option<f64>,
    /// When the waiting states were last re-checked.
    pub(crate) last_recheck: f64,
    /// The destructive dialog for removing the driver is open.
    pub(crate) confirm_uninstall: bool,
    /// Move the keyboard focus to the section list on the next frame.
    pub(crate) focus_nav: bool,
    /// An arrow key was pressed on the section list: the item that has the focus next
    /// frame is the one chosen.
    pub(crate) arrow_pending: bool,
    /// The window had keyboard focus in the previous frame (to re-check on regaining it).
    pub(crate) had_focus: bool,
}

/// The window's wide layout applies at this window width.
pub fn is_wide(window_width: f32) -> bool {
    window_width >= WIDE_MIN_WIDTH
}

/// Everything the window body reads and changes.
pub struct BodyArgs<'a> {
    /// The editable values.
    pub settings: &'a mut UiSettings,
    /// What the app probed.
    pub data: &'a SettingsData,
    /// Requests for the app.
    pub commands: &'a mut Vec<SettingsCommand>,
    /// Navigation state.
    pub view: &'a mut SettingsViewState,
    /// Name of the active colour theme.
    pub current_theme: &'a str,
    /// A theme chosen this frame.
    pub pending_theme: &'a mut Option<String>,
    /// Assistant transport status.
    pub network: &'a NetworkStatus,
}

/// Draw the window body into the padded frame of the settings window.
pub fn body(ui: &mut Ui, tk: &Tokens, args: BodyArgs<'_>) {
    let BodyArgs {
        settings,
        data,
        commands,
        view,
        current_theme,
        pending_theme,
        network,
    } = args;
    let now = ui.ctx().input(|i| i.time);
    let window_width = ui.available_width() + 2.0 * WINDOW_PADDING;
    watch_focus(ui, view, commands);

    let mut cx = PageCx {
        tk,
        settings,
        data,
        commands,
        view,
        current_theme,
        pending_theme,
        network,
        now,
    };
    if is_wide(window_width) {
        wide(ui, &mut cx);
    } else {
        narrow(ui, &mut cx);
    }
}

/// Re-check permissions when the window regains focus (the user may have just
/// changed System Settings), and every [`WAITING_RECHECK_SECONDS`] while a
/// permission waits for System Settings.
fn watch_focus(ui: &Ui, view: &mut SettingsViewState, commands: &mut Vec<SettingsCommand>) {
    let (focused, now) = ui
        .ctx()
        .input(|i| (i.viewport().focused.unwrap_or(true), i.time));
    if focused && !view.had_focus {
        commands.push(SettingsCommand::Refresh);
    }
    view.had_focus = focused;
    let waiting = view.mic_waiting_since.is_some() || view.call_audio_waiting_since.is_some();
    if waiting {
        if now - view.last_recheck >= WAITING_RECHECK_SECONDS {
            view.last_recheck = now;
            commands.push(SettingsCommand::Refresh);
        }
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(WAITING_RECHECK_SECONDS));
    }
}

/// Section list on the left, the page on the right.
fn wide(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let height = ui.available_height();
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.allocate_ui_with_layout(
            vec2(NAV_WIDTH, height),
            Layout::top_down(Align::Min),
            |ui| {
                ui.set_width(NAV_WIDTH);
                section_list(ui, cx);
            },
        );
        ui.add_space(space::S24);
        let column = ui.available_width().min(COLUMN_MAX);
        ui.allocate_ui_with_layout(vec2(column, height), Layout::top_down(Align::Min), |ui| {
            ui.set_width(column);
            content(ui, cx);
        });
    });
}

/// One column: the section select on top, then the page.
fn narrow(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let labels: Vec<String> = SettingsSection::ALL.iter().map(|s| s.label()).collect();
    let selected = cx.view.section.index();
    let width = ui.available_width();
    let name = prxvc_i18n::t("settings.section_select");
    if let Some(i) = widgets::select_named(
        ui,
        cx.tk,
        "settings_section_select",
        width,
        &name,
        &labels[selected],
        &labels,
        selected,
    ) {
        cx.view.section = SettingsSection::ALL[i];
    }
    ui.add_space(space::S8);
    content(ui, cx);
}

/// The section list: one 38 px menu item per section, the current one marked.
/// Arrow keys move the focus (egui's own navigation); a section is chosen with
/// Enter or Space, or at once when an arrow key lands on it.
fn section_list(ui: &mut Ui, cx: &mut PageCx<'_>) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let arrow =
        ui.input(|i| i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowUp));
    let focus_first = std::mem::take(&mut cx.view.focus_nav);
    let follow_focus = std::mem::take(&mut cx.view.arrow_pending);
    let mut any_focused = false;
    for section in SettingsSection::ALL {
        let current = cx.view.section == section;
        let response = ui
            .push_id(("settings_nav", section), |ui| {
                widgets::menu_item(ui, cx.tk, &section.label(), current)
            })
            .inner;
        if focus_first && current {
            response.request_focus();
        }
        any_focused |= response.has_focus();
        // egui moves the focus at the end of the frame in which the arrow key was
        // pressed, so the item that took it is chosen one frame later.
        if response.clicked() || (follow_focus && response.has_focus()) {
            cx.view.section = section;
        }
    }
    cx.view.arrow_pending = arrow && any_focused;
}

/// Header, notices and the scrolling page.
fn content(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let section = cx.view.section;
    egui::ScrollArea::vertical()
        .id_salt(("settings_page", section))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.add(egui::Label::new(text(section.label(), size::TITLE, cx.tk.text)).wrap());
            ui.add_space(space::S8);
            notices(ui, cx);
            settings_view::render_page(ui, cx, section);
            ui.add_space(space::S24);
        });
}

fn notices(ui: &mut Ui, cx: &PageCx<'_>) {
    for n in &cx.data.notices {
        let (tone, message) = match n {
            SettingsNotice::UnreadableFileKept { kept_as } => (
                Tone::Warning,
                prxvc_i18n::t_with("settings.notice.unreadable", &[kept_as]),
            ),
            SettingsNotice::SaveFailed { detail } => (
                Tone::Danger,
                prxvc_i18n::t_with("settings.notice.save_failed", &[detail]),
            ),
        };
        notice(ui, cx.tk, tone, &message);
        ui.add_space(space::S8);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wide_layout_starts_at_900() {
        assert!(!is_wide(899.9));
        assert!(is_wide(900.0));
    }

    #[test]
    fn the_wide_layout_fits_the_largest_window() {
        let max = crate::tool_window::ToolKind::Settings.max_size().x;
        assert!(NAV_WIDTH + space::S24 + COLUMN_MAX + 2.0 * WINDOW_PADDING <= max + 40.0);
    }
}
