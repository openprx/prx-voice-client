//! Settings page Privacy and data: history, transcripts and logs (design 2.2).

use egui::Ui;

use crate::settings_rows::{PageCx, saved_only, switch_row, t, unavailable_row};

pub(crate) fn page(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    switch_row(
        ui,
        tk,
        &t("settings.privacy.history"),
        &saved_only(&t("settings.save_history.desc")),
        &mut cx.settings.save_history,
        true,
    );
    let history_on = cx.settings.save_history;
    let description = if history_on {
        t("settings.privacy.transcripts.desc")
    } else {
        format!(
            "{} {}",
            t("settings.privacy.transcripts.desc"),
            t("settings.privacy.transcripts.needs_history")
        )
    };
    switch_row(
        ui,
        tk,
        &t("settings.privacy.transcripts"),
        &saved_only(&description),
        &mut cx.settings.save_transcript,
        history_on,
    );
    switch_row(
        ui,
        tk,
        &t("settings.privacy.log_sensitive"),
        &saved_only(&t("settings.privacy.log_sensitive.desc")),
        &mut cx.settings.log_sensitive,
        true,
    );
    unavailable_row(
        ui,
        tk,
        &t("settings.privacy.delete_history"),
        &t("settings.privacy.delete_history.desc"),
        "settings.reason.delete_history",
        None,
    );
}
