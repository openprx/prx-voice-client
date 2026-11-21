//! The translation engine rows: the choice between system translation and the language model,
//! each engine's one-line explanation and the status of the selected one, with its actions.
//! Shown on the Call translation page and mirrored read-only on the Speech pipeline page.

use egui::Ui;

use crate::settings_command::{SettingsCommand, SystemPane};
use crate::settings_data::{EngineData, ModelServerView, SystemEngineView};
use crate::settings_rows::{PageCx, note, select_row, status_row, t};
use crate::settings_view::{index_of, labels};
use crate::widgets::Tone;

/// Values of `call.translation_engine` in list order.
pub(crate) const ENGINES: [&str; 2] = ["system", "llm"];

/// What the status row of the selected engine says: its tone, its sentence and whether it has an
/// action (and which).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EngineStatus {
    pub tone: Tone,
    /// Short chip text.
    pub chip: String,
    /// The explaining sentence.
    pub text: String,
    pub action: Option<EngineAction>,
}

/// What the button of the status row does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EngineAction {
    OpenTranslationLanguages,
    RetryServer,
}

/// The status of the engine `engine` (a value of `call.translation_engine`).
pub(crate) fn status_of(engine: &str, data: &EngineData) -> EngineStatus {
    let mk = |tone, chip: &str, text: String, action| EngineStatus {
        tone,
        chip: t(chip),
        text,
        action,
    };
    if engine == "llm" {
        return match &data.server {
            ModelServerView::Stopped => mk(
                Tone::Neutral,
                "settings.engine.chip.stopped",
                t("settings.engine.llm.stopped"),
                None,
            ),
            ModelServerView::Starting { waited_s } => mk(
                Tone::Warning,
                "settings.engine.chip.starting",
                format!(
                    "{} ({waited_s} s)",
                    t("settings.engine.llm.starting").trim_end_matches("...")
                ),
                None,
            ),
            ModelServerView::Ready { adopted: false, .. } => mk(
                Tone::Success,
                "settings.engine.chip.ready",
                t("settings.engine.llm.ready"),
                None,
            ),
            ModelServerView::Ready {
                adopted: true,
                address,
            } => mk(
                Tone::Success,
                "settings.engine.chip.ready",
                prxvc_i18n::t_with("settings.engine.llm.adopted", &[address]),
                None,
            ),
            ModelServerView::Stopping => mk(
                Tone::Neutral,
                "settings.engine.chip.stopping",
                t("settings.engine.llm.stopping"),
                None,
            ),
            ModelServerView::Failed(reason) => mk(
                Tone::Danger,
                "settings.engine.chip.failed",
                prxvc_i18n::t_with("settings.engine.llm.failed", &[reason]),
                Some(EngineAction::RetryServer),
            ),
        };
    }
    match &data.system {
        SystemEngineView::NotChecked => mk(
            Tone::Neutral,
            "settings.engine.chip.checking",
            t("settings.engine.system.checking"),
            None,
        ),
        SystemEngineView::Ready => mk(
            Tone::Success,
            "settings.engine.chip.ready",
            t("settings.engine.system.ready"),
            None,
        ),
        SystemEngineView::PackMissing(pair) => mk(
            Tone::Warning,
            "settings.engine.chip.pack_missing",
            prxvc_i18n::t_with("settings.engine.system.pack_missing", &[pair]),
            Some(EngineAction::OpenTranslationLanguages),
        ),
        SystemEngineView::Unsupported(pair) => mk(
            Tone::Danger,
            "settings.engine.chip.unavailable",
            prxvc_i18n::t_with("settings.engine.system.unsupported", &[pair]),
            None,
        ),
        SystemEngineView::OsTooOld => mk(
            Tone::Danger,
            "settings.engine.chip.unavailable",
            t("settings.engine.system.os_too_old"),
            None,
        ),
        SystemEngineView::NotBuilt => mk(
            Tone::Danger,
            "settings.engine.chip.unavailable",
            t("settings.engine.system.not_built"),
            None,
        ),
        SystemEngineView::Failed(reason) => mk(
            Tone::Danger,
            "settings.engine.chip.failed",
            prxvc_i18n::t_with("settings.engine.system.failed", &[reason]),
            None,
        ),
    }
}

/// The engine choice, the two explanations and the status row of the selected engine.
pub(crate) fn rows(ui: &mut Ui, cx: &mut PageCx<'_>) {
    let tk = cx.tk;
    let options = labels(&["settings.engine.system", "settings.engine.llm"]);
    let selected = index_of(&ENGINES, &cx.settings.call_translation_engine);
    if let Some(i) = select_row(
        ui,
        tk,
        "settings_call_engine",
        &t("settings.engine.title"),
        &t("settings.engine.desc"),
        &options,
        selected,
    ) {
        cx.settings.call_translation_engine = ENGINES[i].to_owned();
    }
    note(
        ui,
        tk,
        &format!(
            "{}: {}",
            t("settings.engine.system"),
            t("settings.engine.system.desc")
        ),
    );
    note(
        ui,
        tk,
        &format!(
            "{}: {}",
            t("settings.engine.llm"),
            t("settings.engine.llm.desc")
        ),
    );
    let status = status_of(&cx.settings.call_translation_engine, &cx.data.engine);
    let action_label = status.action.map(|a| match a {
        EngineAction::OpenTranslationLanguages => t("settings.engine.action.open_languages"),
        EngineAction::RetryServer => t("settings.engine.action.retry"),
    });
    let clicked = status_row(
        ui,
        tk,
        &t("settings.engine.status"),
        &status.text,
        status.tone,
        &status.chip,
        action_label.as_deref().map(|l| (l, true)),
    );
    if clicked {
        match status.action {
            Some(EngineAction::OpenTranslationLanguages) => cx.commands.push(
                SettingsCommand::OpenSystemPane(SystemPane::TranslationLanguages),
            ),
            Some(EngineAction::RetryServer) => cx.commands.push(SettingsCommand::RetryModelServer),
            None => {}
        }
    }
}

/// The read-only line of the Speech pipeline page: which engine translates, and its status.
pub(crate) fn mirror(ui: &mut Ui, cx: &PageCx<'_>) {
    let status = status_of(&cx.settings.call_translation_engine, &cx.data.engine);
    let name = if cx.settings.call_translation_engine == "llm" {
        t("settings.engine.llm")
    } else {
        t("settings.engine.system")
    };
    note(
        ui,
        cx.tk,
        &format!(
            "{} {}",
            prxvc_i18n::t_with("settings.engine.mirror", &[&name]),
            status.text
        ),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(system: SystemEngineView, server: ModelServerView) -> EngineData {
        EngineData { system, server }
    }

    #[test]
    fn the_system_engine_offers_the_pane_only_when_a_pack_is_missing() {
        let d = |s| data(s, ModelServerView::Stopped);
        assert_eq!(
            status_of("system", &d(SystemEngineView::PackMissing("a".into()))).action,
            Some(EngineAction::OpenTranslationLanguages)
        );
        for s in [
            SystemEngineView::Ready,
            SystemEngineView::OsTooOld,
            SystemEngineView::NotBuilt,
            SystemEngineView::NotChecked,
        ] {
            assert_eq!(status_of("system", &d(s)).action, None);
        }
        assert_eq!(
            status_of("system", &d(SystemEngineView::Ready)).tone,
            Tone::Success
        );
    }

    #[test]
    fn the_model_server_shows_starting_ready_and_failed_with_a_retry() {
        let d = |s| data(SystemEngineView::Ready, s);
        assert_eq!(
            status_of("llm", &d(ModelServerView::Starting { waited_s: 4 })).tone,
            Tone::Warning
        );
        let ready = status_of(
            "llm",
            &d(ModelServerView::Ready {
                adopted: true,
                address: "127.0.0.1:8080".into(),
            }),
        );
        assert_eq!(ready.tone, Tone::Success);
        assert!(ready.text.contains("127.0.0.1:8080"));
        let failed = status_of("llm", &d(ModelServerView::Failed("no binary".into())));
        assert_eq!(failed.action, Some(EngineAction::RetryServer));
        assert!(failed.text.contains("no binary"));
    }

    #[test]
    fn the_status_follows_the_selected_engine_only() {
        let both = data(
            SystemEngineView::OsTooOld,
            ModelServerView::Failed("x".into()),
        );
        assert_eq!(
            status_of("system", &both).chip,
            t("settings.engine.chip.unavailable")
        );
        assert_eq!(
            status_of("llm", &both).chip,
            t("settings.engine.chip.failed")
        );
    }
}
