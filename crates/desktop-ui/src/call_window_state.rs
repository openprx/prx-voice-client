//! What the Call translation window remembers between frames: the conversation, the scroll follow state, the measured heights of the utterances, which parts are open, and when the call began.
//!
//! The caption text size and the "show original" switch are settings (`call.captions_size`,
//! `call.captions_show_source`); everything here is the window's own memory and is never saved.

use std::collections::HashMap;
use std::time::Instant;

use crate::call_transcript::{Side, Transcript};
use crate::call_view::{CallPanel, CallPhase};
use crate::theme::size;

/// The caption text size choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptionSize {
    /// Small.
    Small,
    /// Medium (the default).
    Medium,
    /// Large.
    Large,
}

impl CaptionSize {
    /// Every choice, smallest first.
    pub const ALL: [Self; 3] = [Self::Small, Self::Medium, Self::Large];

    /// The choice a setting value names; anything unknown is the default.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }

    /// The setting value of this choice.
    pub const fn setting(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
        }
    }

    /// The size of the primary caption text: the `secondary`, `body` and `subheading` tokens.
    pub const fn px(self) -> f32 {
        match self {
            Self::Small => size::SECONDARY,
            Self::Medium => size::BODY,
            Self::Large => size::SUBHEADING,
        }
    }

    /// The i18n key of the short label of this choice.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Small => "callwin.size.small",
            Self::Medium => "callwin.size.medium",
            Self::Large => "callwin.size.large",
        }
    }
}

/// The measured height of one utterance, valid for the layout it was measured in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Measured {
    pub height: f32,
    pub key: u64,
}

/// The window's memory.
#[derive(Debug, Default)]
pub struct CallWindowState {
    /// The conversation of this session.
    pub transcript: Transcript,
    /// The details part of the footer is open.
    pub details_open: bool,
    /// When the running call began; `None` while idle.
    pub began: Option<Instant>,
    /// The panel the transcript has absorbed, to notice a change.
    seen: CallPanel,
    pub(crate) heights: HashMap<u64, Measured>,
    /// The captions were scrolled to the end at the last frame.
    pub(crate) at_bottom: bool,
    /// The user asked for the jump to the newest line.
    pub(crate) jump: bool,
    /// Utterances that began while the captions were scrolled away from the end.
    pub(crate) unseen_from: Option<u64>,
    /// How many utterances began while the captions were scrolled away from the end.
    pub(crate) unseen: usize,
    /// What was copied last, and when (the "Copied" note).
    pub(crate) copied: Option<(Option<u64>, f64)>,
}

impl CallWindowState {
    /// Seconds since the running call began.
    pub fn elapsed_s(&self) -> u32 {
        self.began
            .map_or(0, |b| u32::try_from(b.elapsed().as_secs()).unwrap_or(u32::MAX))
    }

    /// Take in a new panel: start a fresh session when a call begins, then merge both lanes into
    /// the conversation. Cheap when nothing changed.
    pub fn sync(&mut self, panel: &CallPanel) {
        if *panel == self.seen {
            return;
        }
        let was_idle = self.seen.phase == CallPhase::Idle;
        let is_idle = panel.phase == CallPhase::Idle;
        if was_idle && !is_idle {
            // The next start: the old captions go, nothing of them was ever stored.
            self.transcript.clear();
            self.heights.clear();
            self.began = Some(Instant::now());
            self.unseen_from = None;
        } else if !was_idle && is_idle {
            self.began = None;
        }
        let at = self.elapsed_s();
        self.transcript.absorb(Side::Me, &panel.outgoing, at);
        self.transcript.absorb(Side::Peer, &panel.incoming, at);
        self.seen = panel.clone();
        // Forget heights of utterances that left the bounded list.
        if self.heights.len() > 2 * Transcript::MAX_UTTERANCES {
            let live: std::collections::HashSet<u64> =
                self.transcript.items().iter().map(|u| u.id).collect();
            self.heights.retain(|id, _| live.contains(id));
        }
    }

    /// The captions were scrolled to the end at the last frame, so they follow the newest line.
    pub fn at_bottom(&self) -> bool {
        self.at_bottom
    }

    /// How many utterances began while the captions were scrolled away from the end (the number
    /// on the jump pill).
    pub fn unseen(&self) -> usize {
        self.unseen
    }

    /// The utterance whose Copy action was used last, while its "Copied" note still shows.
    pub fn last_copied(&self) -> Option<u64> {
        self.copied.and_then(|(id, _)| id)
    }

    /// Forget the conversation (the Clear action).
    pub fn clear_captions(&mut self) {
        self.transcript.clear();
        self.heights.clear();
        self.unseen_from = None;
    }

    /// Pretend a call began `secs` ago (tests and snapshots).
    pub fn began_ago(&mut self, secs: u64) {
        self.began = Instant::now().checked_sub(std::time::Duration::from_secs(secs));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::call_view::{CaptionView, ClauseStatus, LaneView};

    fn panel(phase: CallPhase, text: &str) -> CallPanel {
        CallPanel {
            phase,
            outgoing: LaneView {
                translating: true,
                clauses: vec![CaptionView {
                    utterance: 1,
                    source: text.into(),
                    translation: "Hi.".into(),
                    status: ClauseStatus::Played,
                    ..CaptionView::default()
                }],
                ..LaneView::default()
            },
            ..CallPanel::default()
        }
    }

    #[test]
    fn captions_are_kept_after_stop_and_cleared_at_the_next_start() {
        let mut w = CallWindowState::default();
        w.sync(&panel(CallPhase::Translating, "你好"));
        assert_eq!(w.transcript.items().len(), 1);
        assert!(w.began.is_some());
        // Stopped: the lines stay readable.
        w.sync(&CallPanel {
            phase: CallPhase::Idle,
            ..panel(CallPhase::Idle, "你好")
        });
        assert_eq!(w.transcript.items().len(), 1);
        assert!(w.began.is_none());
        // The next start clears them before the new session's first line arrives.
        w.sync(&CallPanel {
            phase: CallPhase::Initializing,
            ..CallPanel::default()
        });
        assert!(w.transcript.is_empty());
        assert!(w.began.is_some());
    }

    #[test]
    fn the_size_setting_round_trips_and_unknown_means_medium() {
        for s in CaptionSize::ALL {
            assert_eq!(CaptionSize::from_setting(s.setting()), s);
        }
        assert_eq!(CaptionSize::from_setting("huge"), CaptionSize::Medium);
        assert!(CaptionSize::Small.px() < CaptionSize::Medium.px());
        assert!(CaptionSize::Medium.px() < CaptionSize::Large.px());
    }
}
