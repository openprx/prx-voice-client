//! The spoken notice to the other party in the Call translation window: one quiet line saying where it stands (getting ready, plays before your first sentence, played at ...), the "say it again" action, and, when the notice is off, a line saying the other party is not being told. The switch itself is in the details.

use crate::call_transcript::mmss;
use crate::call_view::{CallPanel, CallPhase, NoticeState, command};
use crate::call_window_view::CallWindowAction;
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, ButtonKind};

/// What the line says, and whether "say it again" is offered; `None` when there is nothing to say
/// (idle with the notice on, a call without translation).
pub(crate) fn line(panel: &CallPanel, settings: &UiSettings) -> Option<(String, bool)> {
    let running = !matches!(panel.phase, CallPhase::Idle | CallPhase::Stopping);
    if !running {
        return (!settings.call_disclosure)
            .then(|| (prxvc_i18n::t("callwin.notice.off"), false));
    }
    if !panel.translate_enabled {
        return None;
    }
    if !panel.notice.enabled {
        return Some((prxvc_i18n::t("callwin.notice.off"), false));
    }
    let n = &panel.notice;
    Some(match n.state {
        NoticeState::Off | NoticeState::Preparing => {
            (prxvc_i18n::t("callwin.notice.preparing"), false)
        }
        NoticeState::Ready => (prxvc_i18n::t("callwin.notice.ready"), true),
        NoticeState::Playing => (prxvc_i18n::t("callwin.notice.playing"), false),
        NoticeState::Played => (
            prxvc_i18n::t_with(
                "callwin.notice.played",
                &[&n.last_played_s.map_or_else(|| "-".to_owned(), mmss)],
            ),
            true,
        ),
        NoticeState::Failed => (prxvc_i18n::t("callwin.notice.failed"), false),
    })
}

/// Draw the line and its action.
pub(crate) fn notice_line(
    ui: &mut egui::Ui,
    tk: &Tokens,
    panel: &CallPanel,
    settings: &UiSettings,
    actions: &mut Vec<CallWindowAction>,
) {
    let Some((text, again)) = line(panel, settings) else {
        return;
    };
    ui.add_space(space::S8);
    ui.horizontal_wrapped(|ui| {
        ui.add(egui::Label::new(widgets::text(text, size::META, tk.secondary)).wrap());
        if again
            && widgets::button_with(
                ui,
                tk,
                ButtonKind::Text,
                &prxvc_i18n::t("callwin.notice.again"),
                layout::CONTROL_HEIGHT_SMALL,
                true,
            )
            .clicked()
        {
            actions.push(CallWindowAction::Command(command::SAY_DISCLOSURE));
        }
    });
}

/// The switch in the details; it applies from the next start.
pub(crate) fn switch_row(ui: &mut egui::Ui, tk: &Tokens, settings: &mut UiSettings) {
    ui.add_space(space::S8);
    let label = prxvc_i18n::t("callwin.notice.switch");
    ui.horizontal_wrapped(|ui| {
        ui.name_inputs(&label, |ui| {
            widgets::switch(ui, tk, &mut settings.call_disclosure);
        });
        ui.label(widgets::text(&label, size::META, tk.text));
    });
    ui.add(
        egui::Label::new(widgets::text(
            prxvc_i18n::t("callwin.notice.switch_hint"),
            size::META,
            tk.secondary,
        ))
        .wrap(),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_view::NoticeView;

    fn running(notice: NoticeView) -> CallPanel {
        CallPanel {
            phase: CallPhase::Translating,
            translate_enabled: true,
            notice,
            ..CallPanel::default()
        }
    }

    #[test]
    fn idle_with_the_notice_on_says_nothing_and_off_says_the_other_party_is_not_told() {
        let on = UiSettings::default();
        assert!(line(&CallPanel::default(), &on).is_none());
        let off = UiSettings {
            call_disclosure: false,
            ..UiSettings::default()
        };
        assert!(line(&CallPanel::default(), &off).is_some());
    }

    #[test]
    fn a_running_call_says_where_the_notice_stands_and_offers_the_repeat_when_it_can() {
        let s = UiSettings::default();
        let notice = |state, enabled| NoticeView {
            enabled,
            state,
            plays: 0,
            last_played_s: Some(75),
        };
        assert_eq!(line(&running(notice(NoticeState::Preparing, true)), &s).unwrap().1, false);
        assert_eq!(line(&running(notice(NoticeState::Ready, true)), &s).unwrap().1, true);
        let (text, again) = line(&running(notice(NoticeState::Played, true)), &s).unwrap();
        assert!(again && text.contains("1:15"), "{text}");
        assert_eq!(line(&running(notice(NoticeState::Failed, true)), &s).unwrap().1, false);
        let off = line(&running(notice(NoticeState::Off, false)), &s).unwrap();
        assert!(!off.1);
        // A call without translation tells nobody anything.
        let mut audio_only = running(notice(NoticeState::Off, false));
        audio_only.translate_enabled = false;
        assert!(line(&audio_only, &s).is_none());
    }
}
