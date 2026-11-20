//! Settings page Shortcuts: the global shortcuts (design 2.2; the recorder is a later package).

use egui::Ui;

use crate::settings_rows::{PageCx, saved_only, segmented_row, switch_row, t, unavailable_row};
use crate::settings_view::{index_of, labels};

/// Values of `hotkeys.assistant_mode` in segment order.
const MODES: [&str; 2] = ["toggle", "hold"];

fn shortcut_or_none(value: &str) -> String {
    if value.is_empty() {
        t("settings.shortcuts.none")
    } else {
        value.to_owned()
    }
}

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    switch_row(
        ui,
        tk,
        &t("settings.shortcuts.enabled"),
        &saved_only(&t("settings.hotkey.desc")),
        &mut cx.settings.hotkey_enabled,
        true,
    );
    let rows = [
        (
            "settings.shortcuts.toggle",
            "settings.shortcuts.toggle.desc",
            cx.settings.hotkey_session_toggle.clone(),
        ),
        (
            "settings.shortcuts.ptt",
            "settings.shortcuts.ptt.desc",
            cx.settings.hotkey_push_to_talk.clone(),
        ),
        (
            "settings.shortcuts.interrupt",
            "settings.shortcuts.interrupt.desc",
            cx.settings.hotkey_interrupt.clone(),
        ),
    ];
    for (label, desc, value) in rows {
        unavailable_row(
            ui,
            tk,
            &t(label),
            &t(desc),
            "settings.reason.recorder",
            Some(&shortcut_or_none(&value)),
        );
    }
    let options = labels(&[
        "settings.shortcuts.mode.toggle",
        "settings.shortcuts.mode.hold",
    ]);
    let selected = index_of(&MODES, &cx.settings.hotkey_assistant_mode);
    if let Some(i) = segmented_row(
        ui,
        tk,
        &t("settings.shortcuts.mode"),
        &saved_only(&t("settings.shortcuts.mode.desc")),
        &options,
        selected,
    ) {
        cx.settings.hotkey_assistant_mode = MODES[i].to_owned();
    }
}
