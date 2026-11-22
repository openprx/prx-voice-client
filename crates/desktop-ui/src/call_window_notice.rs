//! The spoken notice to the other party in the Call translation window: one quiet line saying where it stands (getting ready, plays when you first speak, played at ...), the "say it again" action, and, when the notice is off, a line saying the other party is not being told. The switch itself is in the details.

use crate::call_transcript::mmss;
use crate::call_view::{CallPanel, CallPhase, NoticeState, command};
use crate::call_window_view::CallWindowAction;
use crate::state::UiSettings;
use crate::theme::{Tokens, layout, size, space};
use crate::widgets::{self, ButtonKind};

/// What the line says, and whether "say it again" is offered; `None` when there is nothing to say
/// (no call session runs, or a call without translation). Where the notice is switched is the
/// details; the line is about the running call only.
pub(crate) fn line(panel: &CallPanel) -> Option<(String, bool)> {
    let running = !matches!(panel.phase, CallPhase::Idle | CallPhase::Stopping);
    if !running {
        return None;
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
    actions: &mut Vec<CallWindowAction>,
) {
    let Some((text, again)) = line(panel) else {
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
    fn the_line_is_about_a_running_call_only() {
        assert!(line(&CallPanel::default()).is_none());
        assert!(line(&running(NoticeView::default())).is_some());
    }

    #[test]
    fn a_running_call_says_where_the_notice_stands_and_offers_the_repeat_when_it_can() {
        let notice = |state, enabled| NoticeView {
            enabled,
            state,
            plays: 0,
            last_played_s: Some(75),
        };
        assert_eq!(
            line(&running(notice(NoticeState::Preparing, true)))
                .unwrap()
                .1,
            false
        );
        assert_eq!(
            line(&running(notice(NoticeState::Ready, true))).unwrap().1,
            true
        );
        let (text, again) = line(&running(notice(NoticeState::Played, true))).unwrap();
        assert!(again && text.contains("1:15"), "{text}");
        assert_eq!(
            line(&running(notice(NoticeState::Failed, true))).unwrap().1,
            false
        );
        let off = line(&running(notice(NoticeState::Off, false))).unwrap();
        assert!(!off.1);
        // A call without translation tells nobody anything.
        let mut audio_only = running(notice(NoticeState::Off, false));
        audio_only.translate_enabled = false;
        assert!(line(&audio_only).is_none());
    }
}
