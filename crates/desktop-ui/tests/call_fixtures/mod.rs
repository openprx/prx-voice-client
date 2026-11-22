//! Fixed call panels for the tests and snapshots of the Call translation window: the same states the owner sees, with texts in both languages, long utterances and mixed-language text.
#![allow(dead_code)] // each test file uses a different part

use prxvc_desktop_ui::call_view::{
    CaptionFlag, CaptionView, ClauseStatus, CountersView, IssueAction, LatencyView, NoticeState,
    NoticeView, PartialView, SkipView,
};
use prxvc_desktop_ui::{CallPanel, CallPhase, Heard, IssueSeverity, IssueView, LaneView};

pub fn clause(
    utterance: u64,
    index: u16,
    source: &str,
    translation: &str,
    status: ClauseStatus,
) -> CaptionView {
    CaptionView {
        utterance,
        index,
        source: source.into(),
        translation: translation.into(),
        status,
        ..CaptionView::default()
    }
}

pub fn lane(hears: Heard, languages: (&str, &str)) -> LaneView {
    LaneView {
        running: true,
        translating: true,
        languages: Some((languages.0.into(), languages.1.into())),
        hears,
        device: "Built-in Mic".into(),
        rms: 0.05,
        peak: 0.3,
        counters: Some(CountersView {
            filler_dropped: 3,
            backlog_skips: 1,
            stale_skips: 1,
            in_target_passed: 1,
            mt_retries: 1,
            ..CountersView::default()
        }),
        latency: Some(LatencyView {
            speech_start_to_first_audio_ms: Some(2500.0),
            pause_to_first_audio_ms: Some(900.0),
            stages: vec![("mt_first_token".into(), 90.0)],
        }),
        ..LaneView::default()
    }
}

/// The user speaks Chinese to an English listener; the other party's English is heard in Chinese.
pub fn live_panel() -> CallPanel {
    let mut me = lane(Heard::Translation, ("zh", "en"));
    me.clauses = vec![
        clause(
            1,
            0,
            "你好，今天的会议改到下午三点",
            "Hello, today's meeting has moved to three in the afternoon.",
            ClauseStatus::Played,
        ),
        clause(
            2,
            0,
            "我们先看一下上周的数据",
            "Let's start with last week's numbers.",
            ClauseStatus::Playing,
        ),
        {
            let mut c = clause(
                2,
                1,
                "然后讨论新的方案和预算",
                "",
                ClauseStatus::Translating,
            );
            c.retried = true;
            c
        },
    ];
    me.partial = Some(PartialView {
        utterance: 3,
        text: "对了，别忘了周五的评审会议".into(),
        stable_len: "对了，别忘了周五".len(),
    });
    // The user is mid-sentence and a clause is playing: the translation is still going out.
    me.in_flight = true;
    let mut peer = lane(Heard::Translation, ("en", "zh"));
    peer.clauses = vec![
        {
            let mut c = clause(
                1,
                0,
                "Sounds good, three works for me.",
                "好的，三点我可以。",
                ClauseStatus::Played,
            );
            c.flags = vec![CaptionFlag::AnswerLike];
            c
        },
        {
            let mut c = clause(
                2,
                0,
                "今天天气很好我们出去走走吧",
                "",
                ClauseStatus::Skipped,
            );
            c.skip = SkipView::AlreadyInTarget;
            c
        },
        {
            let mut c = clause(
                3,
                0,
                "I will send the updated deck after the call, together with the notes from last week.",
                "",
                ClauseStatus::Skipped,
            );
            c.skip = SkipView::NotSpoken;
            c
        },
        clause(
            3,
            1,
            "Also, please check the Zoom link",
            "另外，请检查 Zoom 链接。",
            ClauseStatus::Synthesizing,
        ),
    ];
    peer.partial = Some(PartialView {
        utterance: 4,
        text: "Can you hear me now or".into(),
        stable_len: "Can you hear me".len(),
    });
    CallPanel {
        phase: CallPhase::Translating,
        translate_enabled: true,
        in_call_mode: true,
        default_input_ours: true,
        outgoing: me,
        incoming: peer,
        notice: NoticeView {
            enabled: true,
            state: NoticeState::Played,
            plays: 1,
            last_played_s: Some(12),
        },
        ..CallPanel::default()
    }
}

fn issue(key: &str, severity: IssueSeverity, action: IssueAction) -> IssueView {
    IssueView {
        key: key.into(),
        severity,
        detail: "detail from the coordinator".into(),
        action,
    }
}

pub fn degraded_panel() -> CallPanel {
    CallPanel {
        phase: CallPhase::Degraded,
        issues: vec![
            issue(
                "call.issue.speaker_output",
                IssueSeverity::Degraded,
                IssueAction::OpenAudioSettings,
            ),
            issue(
                "call.issue.provider.server_unreachable",
                IssueSeverity::Blocking,
                IssueAction::OpenSpeechSettings,
            ),
        ],
        ..live_panel()
    }
}

/// The same call after the user's last clause has played: nothing pending, the lane drained.
pub fn drained_panel() -> CallPanel {
    let mut panel = live_panel();
    panel.outgoing.in_flight = false;
    panel.outgoing.partial = None;
    for c in &mut panel.outgoing.clauses {
        c.status = ClauseStatus::Played;
    }
    panel
}

pub fn paused_panel() -> CallPanel {
    CallPanel {
        phase: CallPhase::Paused,
        ..live_panel()
    }
}

pub fn loading_panel() -> CallPanel {
    CallPanel {
        phase: CallPhase::Initializing,
        translate_enabled: true,
        notice: NoticeView {
            enabled: true,
            state: NoticeState::Preparing,
            ..NoticeView::default()
        },
        ..CallPanel::default()
    }
}

pub fn listening_panel() -> CallPanel {
    CallPanel {
        phase: CallPhase::Translating,
        translate_enabled: true,
        in_call_mode: true,
        default_input_ours: true,
        outgoing: lane(Heard::Translation, ("zh", "en")),
        incoming: lane(Heard::Translation, ("en", "zh")),
        notice: NoticeView {
            enabled: true,
            state: NoticeState::Ready,
            ..NoticeView::default()
        },
        ..CallPanel::default()
    }
}

/// A translated call whose notice to the other party is switched off.
pub fn undisclosed_panel() -> CallPanel {
    CallPanel {
        notice: NoticeView::default(),
        ..listening_panel()
    }
}

pub fn blocked_panel() -> CallPanel {
    CallPanel {
        issues: vec![
            issue(
                "call.issue.assistant_active",
                IssueSeverity::Blocking,
                IssueAction::EndAssistantAndStart,
            ),
            issue(
                "call.issue.mic_consent_denied",
                IssueSeverity::Blocking,
                IssueAction::OpenPermissions,
            ),
        ],
        last_error: Some(issue(
            "call.issue.driver_missing",
            IssueSeverity::Blocking,
            IssueAction::OpenPermissions,
        )),
        ..CallPanel::default()
    }
}

/// `n` finished utterances per side, some of them long, with Chinese, English and mixed text.
pub fn long_panel(n: u64) -> CallPanel {
    let mut me = lane(Heard::Translation, ("zh", "en"));
    let mut peer = lane(Heard::Translation, ("en", "zh"));
    for i in 1..=n {
        let long = i % 7 == 0;
        me.clauses.push(clause(
            i,
            0,
            &if long {
                format!("第{i}条：{}", "我们需要把这个问题讲清楚，然后再确认每一个细节，最后把结论发给所有人。".repeat(3))
            } else {
                format!("第{i}条：我们三点在机场见面")
            },
            &if long {
                format!("Item {i}: {}", "We need to explain this clearly, then confirm every detail, and finally send the conclusion to everyone. ".repeat(3))
            } else {
                format!("Item {i}: we meet at the airport at three.")
            },
            ClauseStatus::Played,
        ));
        peer.clauses.push(clause(
            i,
            0,
            &format!("Item {i}: I'll check the iPhone and call you back."),
            &format!("第{i}条：我查一下 iPhone，然后给你回电话。"),
            ClauseStatus::Played,
        ));
    }
    CallPanel {
        phase: CallPhase::Translating,
        translate_enabled: true,
        in_call_mode: true,
        default_input_ours: true,
        outgoing: me,
        incoming: peer,
        ..CallPanel::default()
    }
}
