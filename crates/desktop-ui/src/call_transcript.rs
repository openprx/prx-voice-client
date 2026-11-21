//! The conversation of the Call translation window: the utterances of both directions in the order they began, kept in memory for the session only and cleared at the next start.
//!
//! The coordinator's snapshot carries only the newest clauses of each direction, so the window
//! keeps what it has seen. [`Transcript::absorb`] merges one direction's lane into the timeline:
//! an utterance is a segment of one side, its lines are the clauses in index order, and the live
//! partial hangs on the utterance being spoken. Nothing here is written anywhere; the list is
//! bounded by [`Transcript::MAX_UTTERANCES`].

use std::collections::VecDeque;

use crate::call_view::{CaptionFlag, CaptionView, ClauseStatus, LaneView, PartialView, SkipView};

/// Whose speech an utterance is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// The local user (the outgoing direction).
    Me,
    /// The remote party (the incoming direction).
    Peer,
}

/// One committed clause of an utterance.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The clause's index in its utterance.
    pub index: u16,
    /// What was said.
    pub source: String,
    /// Its translation.
    pub translation: String,
    /// Where it is in its life.
    pub status: ClauseStatus,
    /// Why it was skipped, when it was.
    pub skip: SkipView,
    /// How the translation looked wrong.
    pub flags: Vec<CaptionFlag>,
    /// It was translated a second time.
    pub retried: bool,
}

impl Line {
    fn of(c: &CaptionView) -> Self {
        Self {
            index: c.index,
            source: c.source.clone(),
            translation: c.translation.clone(),
            status: c.status,
            skip: c.skip,
            flags: c.flags.clone(),
            retried: c.retried,
        }
    }
}

/// What an utterance is doing now, as the live indicator says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    /// The speaker is still talking (a partial is open).
    Speaking,
    /// Recognised, being translated or synthesised.
    Translating,
    /// Its translation is playing.
    Playing,
    /// Nothing is going on in it.
    Done,
}

/// One utterance: a segment of one side.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Utterance {
    /// A number that never repeats within the session (stable widget identity and height cache key).
    pub id: u64,
    /// Whose speech.
    pub side: Side,
    /// The segment number in its direction.
    pub seq: u64,
    /// Seconds after the call began when it was first seen.
    pub at_s: u32,
    /// The committed clauses, by index.
    pub lines: Vec<Line>,
    /// The clause being spoken.
    pub partial: Option<PartialView>,
    /// Changes whenever the content does (the height cache key).
    pub rev: u64,
}

impl Utterance {
    /// What it is doing now.
    pub fn activity(&self) -> Activity {
        if self.partial.is_some() {
            return Activity::Speaking;
        }
        let mut any = Activity::Done;
        for l in &self.lines {
            match l.status {
                ClauseStatus::Playing => return Activity::Playing,
                ClauseStatus::Committed
                | ClauseStatus::Recognized
                | ClauseStatus::Translating
                | ClauseStatus::Translated
                | ClauseStatus::Synthesizing => any = Activity::Translating,
                _ => {}
            }
        }
        any
    }

    /// The text the listener reads first: the translations in order, or the source of a clause
    /// that has none yet. Skipped clauses and ones already in the target language have no
    /// translation and are not part of it.
    pub fn translation_text(&self) -> String {
        join(self.lines.iter().filter_map(|l| {
            let t = l.translation.trim();
            (!t.is_empty()).then_some(t)
        }))
    }

    /// What was said, clause by clause.
    pub fn source_text(&self) -> String {
        let mut text = join(self.lines.iter().map(|l| l.source.trim()));
        if let Some(p) = &self.partial {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(p.text.trim());
        }
        text
    }

    /// Whether every clause of it was already in the listener's language.
    pub fn all_in_target(&self) -> bool {
        !self.lines.is_empty() && self.lines.iter().all(|l| l.skip == SkipView::AlreadyInTarget)
    }
}

/// Join non-empty pieces with a space, except where the text itself runs on (Chinese has no spaces).
fn join<'a>(pieces: impl Iterator<Item = &'a str>) -> String {
    let mut out = String::new();
    for p in pieces.filter(|p| !p.is_empty()) {
        if let (Some(last), Some(first)) = (out.chars().next_back(), p.chars().next())
            && last.is_ascii() && first.is_ascii()
            && !last.is_whitespace()
        {
            out.push(' ');
        }
        out.push_str(p);
    }
    out
}

/// The utterances of one session.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Transcript {
    items: VecDeque<Utterance>,
    next_id: u64,
    /// Utterances dropped from the front because the bound was reached.
    pub forgotten: u64,
}

impl Transcript {
    /// The most utterances kept; the oldest are dropped beyond it. Each holds a few clauses of
    /// text, so the list stays small, and the window only lays out what is on screen.
    pub const MAX_UTTERANCES: usize = 300;

    /// The utterances, oldest first.
    pub fn items(&self) -> &VecDeque<Utterance> {
        &self.items
    }

    /// No utterance yet.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Forget everything (the next call starts).
    pub fn clear(&mut self) {
        self.items.clear();
        self.forgotten = 0;
    }

    fn position(&self, side: Side, seq: u64) -> Option<usize> {
        self.items
            .iter()
            .rposition(|u| u.side == side && u.seq == seq)
    }

    fn open(&mut self, side: Side, seq: u64, at_s: u32) -> usize {
        if let Some(i) = self.position(side, seq) {
            return i;
        }
        self.next_id += 1;
        self.items.push_back(Utterance {
            id: self.next_id,
            side,
            seq,
            at_s,
            lines: Vec::new(),
            partial: None,
            rev: 0,
        });
        while self.items.len() > Self::MAX_UTTERANCES {
            self.items.pop_front();
            self.forgotten += 1;
        }
        self.items.len() - 1
    }

    /// Merge what `lane` shows now into the timeline. `at_s` is the time since the call began.
    pub fn absorb(&mut self, side: Side, lane: &LaneView, at_s: u32) {
        for c in &lane.clauses {
            let i = self.open(side, c.utterance, at_s);
            let u = &mut self.items[i];
            let line = Line::of(c);
            match u.lines.binary_search_by_key(&c.index, |l| l.index) {
                Ok(at) if u.lines[at] == line => {}
                Ok(at) => {
                    u.lines[at] = line;
                    u.rev += 1;
                }
                Err(at) => {
                    u.lines.insert(at, line);
                    u.rev += 1;
                }
            }
        }
        let live = lane.partial.as_ref();
        if let Some(p) = live {
            let i = self.open(side, p.utterance, at_s);
            let u = &mut self.items[i];
            if u.partial.as_ref() != Some(p) {
                u.partial = Some(p.clone());
                u.rev += 1;
            }
        }
        // A partial that is no longer the lane's was committed or dropped.
        let live_seq = live.map(|p| p.utterance);
        for u in self.items.iter_mut().filter(|u| u.side == side) {
            if u.partial.is_some() && Some(u.seq) != live_seq {
                u.partial = None;
                u.rev += 1;
            }
        }
        self.items
            .retain(|u| u.side != side || !u.lines.is_empty() || u.partial.is_some());
    }

    /// The conversation as plain text for the clipboard: one block per utterance, the speaker, the
    /// translation and, with `with_source`, what was said. No timestamps of the wall clock.
    pub fn conversation_text(&self, with_source: bool, me: &str, peer: &str) -> String {
        let mut out = String::new();
        for u in &self.items {
            let who = if u.side == Side::Me { me } else { peer };
            let translation = u.translation_text();
            let source = u.source_text();
            let main = if translation.is_empty() { &source } else { &translation };
            if main.is_empty() {
                continue;
            }
            out.push_str(&format!("[{}] {who}: {main}\n", mmss(u.at_s)));
            if with_source && !translation.is_empty() && !source.is_empty() {
                out.push_str(&format!("    ({source})\n"));
            }
        }
        out
    }
}

/// `m:ss` (or `h:mm:ss` from an hour on).
pub fn mmss(secs: u32) -> String {
    let (h, m, s) = (secs / 3600, secs / 60 % 60, secs % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clause(utterance: u64, index: u16, source: &str, translation: &str, status: ClauseStatus) -> CaptionView {
        CaptionView {
            utterance,
            index,
            source: source.into(),
            translation: translation.into(),
            status,
            ..CaptionView::default()
        }
    }

    fn lane(clauses: Vec<CaptionView>, partial: Option<PartialView>) -> LaneView {
        LaneView { clauses, partial, translating: true, ..LaneView::default() }
    }

    #[test]
    fn the_snapshot_window_becomes_a_growing_timeline_in_the_order_things_began() {
        let mut t = Transcript::default();
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "你好", "Hello.", ClauseStatus::Played)], None), 1);
        t.absorb(Side::Peer, &lane(vec![clause(1, 0, "Hi", "你好。", ClauseStatus::Playing)], None), 4);
        // The same clauses again change nothing; a newer window that dropped clause 1 keeps it.
        let rev = t.items()[0].rev;
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "你好", "Hello.", ClauseStatus::Played)], None), 5);
        assert_eq!(t.items()[0].rev, rev);
        t.absorb(Side::Me, &lane(vec![clause(2, 0, "再见", "Bye.", ClauseStatus::Translating)], None), 9);
        let order: Vec<(Side, u64)> = t.items().iter().map(|u| (u.side, u.seq)).collect();
        assert_eq!(order, [(Side::Me, 1), (Side::Peer, 1), (Side::Me, 2)]);
        assert_eq!(t.items()[0].at_s, 1, "the time of the first sight is kept");
    }

    #[test]
    fn a_clause_that_advances_updates_its_line_in_place() {
        let mut t = Transcript::default();
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "你好", "", ClauseStatus::Translating)], None), 1);
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "你好", "Hello.", ClauseStatus::Playing)], None), 2);
        let u = &t.items()[0];
        assert_eq!(u.lines.len(), 1);
        assert_eq!(u.translation_text(), "Hello.");
        assert_eq!(u.activity(), Activity::Playing);
    }

    #[test]
    fn the_partial_belongs_to_its_utterance_and_goes_when_the_lane_moves_on() {
        let mut t = Transcript::default();
        let p = PartialView { utterance: 1, text: "今天天".into(), stable_len: 3 };
        t.absorb(Side::Me, &lane(vec![], Some(p)), 1);
        assert_eq!(t.items()[0].activity(), Activity::Speaking);
        assert_eq!(t.items()[0].source_text(), "今天天");
        // Committed: the clause is there and the partial is gone.
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "今天天气", "", ClauseStatus::Committed)], None), 2);
        assert!(t.items()[0].partial.is_none());
        assert_eq!(t.items()[0].activity(), Activity::Translating);
        // A partial that never became a clause leaves no empty bubble.
        let p2 = PartialView { utterance: 2, text: "嗯".into(), stable_len: 0 };
        t.absorb(Side::Me, &lane(vec![], Some(p2)), 3);
        assert_eq!(t.items().len(), 2);
        t.absorb(Side::Me, &lane(vec![], None), 4);
        assert_eq!(t.items().len(), 1);
    }

    #[test]
    fn the_other_side_is_not_touched_by_a_lane() {
        let mut t = Transcript::default();
        t.absorb(Side::Peer, &lane(vec![], Some(PartialView { utterance: 1, text: "Hel".into(), stable_len: 0 })), 1);
        t.absorb(Side::Me, &lane(vec![], None), 2);
        assert_eq!(t.items().len(), 1, "the peer's live partial stays");
    }

    #[test]
    fn the_timeline_is_bounded_and_says_how_much_it_forgot() {
        let mut t = Transcript::default();
        for n in 1..=(Transcript::MAX_UTTERANCES as u64 + 20) {
            t.absorb(Side::Me, &lane(vec![clause(n, 0, "a", "b", ClauseStatus::Played)], None), 0);
        }
        assert_eq!(t.items().len(), Transcript::MAX_UTTERANCES);
        assert_eq!(t.forgotten, 20);
        assert_eq!(t.items()[0].seq, 21);
        t.clear();
        assert!(t.is_empty() && t.forgotten == 0);
    }

    #[test]
    fn texts_join_without_a_space_inside_chinese_and_with_one_between_english_clauses() {
        let mut t = Transcript::default();
        t.absorb(
            Side::Me,
            &lane(
                vec![
                    clause(1, 0, "你好，", "Hello.", ClauseStatus::Played),
                    clause(1, 1, "今天开会", "Today we meet.", ClauseStatus::Played),
                ],
                None,
            ),
            0,
        );
        let u = &t.items()[0];
        assert_eq!(u.translation_text(), "Hello. Today we meet.");
        assert_eq!(u.source_text(), "你好，今天开会");
    }

    #[test]
    fn skipped_and_passed_through_clauses_have_no_translation_and_say_so() {
        let mut t = Transcript::default();
        let mut c = clause(1, 0, "今天天气很好", "", ClauseStatus::Skipped);
        c.skip = SkipView::AlreadyInTarget;
        t.absorb(Side::Peer, &lane(vec![c], None), 0);
        let u = &t.items()[0];
        assert!(u.all_in_target());
        assert_eq!(u.translation_text(), "");
        assert_eq!(u.activity(), Activity::Done);
    }

    #[test]
    fn the_conversation_copies_as_plain_text() {
        let mut t = Transcript::default();
        t.absorb(Side::Me, &lane(vec![clause(1, 0, "你好", "Hello.", ClauseStatus::Played)], None), 5);
        t.absorb(Side::Peer, &lane(vec![clause(1, 0, "Hi there", "你好呀。", ClauseStatus::Played)], None), 75);
        assert_eq!(
            t.conversation_text(true, "Me", "Them"),
            "[0:05] Me: Hello.\n    (你好)\n[1:15] Them: 你好呀。\n    (Hi there)\n"
        );
        assert_eq!(
            t.conversation_text(false, "Me", "Them"),
            "[0:05] Me: Hello.\n[1:15] Them: 你好呀。\n"
        );
        assert_eq!(mmss(3725), "1:02:05");
    }
}
