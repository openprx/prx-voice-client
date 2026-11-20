//! Test rig: a minimal stand-in for the coordinator that wires the call machine, two direction machines and two segment books together and checks the machine invariants on every step.
#![allow(dead_code)] // each test file uses a different part of the rig

use std::collections::BTreeSet;

use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};
use prxvc_session::{
    Blocker, CallAction, CallEvent, CallRejection, CallState, CancelAction, ClauseEventKind,
    ClauseState, DirSet, DirectionAction, DirectionConfig, DirectionEvent, DirectionRejection,
    DirectionState, InterruptParams, InterruptTrigger, OutputCause, OutputPolicy, OutputSource,
    SegmentAction, SegmentBook, SegmentEvent, SegmentRejection, decide_interrupt,
};

pub const OUT: Direction = Direction::Outgoing;
pub const IN: Direction = Direction::Incoming;
pub const BOTH: [Direction; 2] = [OUT, IN];

pub fn ix(d: Direction) -> usize {
    match d {
        Direction::Outgoing => 0,
        Direction::Incoming => 1,
    }
}

/// One entry of the action log, in the order the actions were returned.
#[derive(Clone, Debug, PartialEq)]
pub enum Logged {
    Call(CallAction),
    Dir(Direction, DirectionAction),
    Seg(Direction, SegmentAction),
}

/// A seeded generator (xorshift64*); tests need repeatable randomness, not quality.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// A value in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % u64::try_from(n.max(1)).unwrap_or(1)).unwrap_or(0)
    }

    pub fn pick<T: Copy>(&mut self, items: &[T]) -> T {
        items[self.below(items.len())]
    }
}

#[derive(Clone)]
pub struct Rig {
    pub call: CallState,
    pub dirs: [DirectionState; 2],
    pub cfgs: [DirectionConfig; 2],
    pub books: [SegmentBook; 2],
    pub interrupt: InterruptParams,
    pub now: MonoTime,
    /// What each listener hears, tracked only from announced changes.
    pub heard: [OutputSource; 2],
    pub log: Vec<Logged>,
    pub released: [Vec<ClauseId>; 2],
    pub rejections: usize,
    opened: [Vec<SegmentId>; 2],
    dead_segments: [BTreeSet<SegmentId>; 2],
    dead_clauses: [BTreeSet<ClauseId>; 2],
}

impl Rig {
    pub fn new(out_policy: OutputPolicy, in_policy: OutputPolicy) -> Self {
        let cfg = |direction, policy| DirectionConfig { direction, policy };
        Self {
            call: CallState::Idle,
            dirs: [DirectionState::Off, DirectionState::Off],
            cfgs: [cfg(OUT, out_policy), cfg(IN, in_policy)],
            books: [SegmentBook::new(OUT), SegmentBook::new(IN)],
            interrupt: InterruptParams::default(),
            now: MonoTime::ZERO,
            heard: [OutputSource::Original; 2],
            log: Vec::new(),
            released: [Vec::new(), Vec::new()],
            rejections: 0,
            opened: [Vec::new(), Vec::new()],
            dead_segments: [BTreeSet::new(), BTreeSet::new()],
            dead_clauses: [BTreeSet::new(), BTreeSet::new()],
        }
    }

    pub fn with_defaults() -> Self {
        Self::new(OutputPolicy::default(), OutputPolicy::default())
    }

    pub fn advance_ms(&mut self, ms: u64) {
        self.now = MonoTime::from_nanos(self.now.as_nanos() + ms * 1_000_000);
    }

    // ---- call machine -------------------------------------------------

    /// Apply a call event and execute the actions the way the coordinator would.
    pub fn call_event(&mut self, ev: CallEvent) -> Result<(), CallRejection> {
        let (next, actions) = self.call.clone().on(ev)?;
        self.call = next;
        for a in &actions {
            self.log.push(Logged::Call(a.clone()));
        }
        for a in actions {
            self.run_call_action(a);
        }
        Ok(())
    }

    /// Bring the call to `Translating` with both directions running.
    pub fn start_translating(&mut self) {
        self.call_event(CallEvent::StartRequested {
            target: None,
            translate: true,
        })
        .expect("start");
        self.call_event(CallEvent::AudioRunning).expect("audio");
        self.call_event(CallEvent::ProvidersReady)
            .expect("providers");
        assert_eq!(self.call, CallState::Translating);
    }

    fn run_call_action(&mut self, a: CallAction) {
        match a {
            CallAction::StartDirections => {
                for d in BOTH {
                    self.dir_quiet(d, DirectionEvent::Start);
                    self.dir_quiet(d, DirectionEvent::StagesReady);
                }
            }
            CallAction::PauseDirections(set) => self.each(set, DirectionEvent::Pause),
            CallAction::ResumeDirections(set) => self.each(set, DirectionEvent::Resume),
            CallAction::SuspendDirections(set) => self.each(set, DirectionEvent::Suspend),
            CallAction::CancelAll => {
                for d in BOTH {
                    self.interrupt_trigger_stop(d);
                }
            }
            CallAction::StopDirections => {
                for d in BOTH {
                    self.dir_quiet(d, DirectionEvent::Stop);
                    self.dir_quiet(d, DirectionEvent::Joined);
                }
            }
            _ => {}
        }
    }

    fn each(&mut self, set: DirSet, ev: DirectionEvent) {
        for d in set.iter() {
            self.dir_quiet(d, ev.clone());
        }
    }

    fn interrupt_trigger_stop(&mut self, d: Direction) {
        let cancels = decide_interrupt(InterruptTrigger::Stop, &self.interrupt, self.now, &[]);
        for c in cancels
            .into_iter()
            .filter(|c| *c == CancelAction::CancelDirection(d))
        {
            self.seg_quiet(d, SegmentEvent::Cancel(c));
        }
    }

    // ---- direction machines -------------------------------------------

    /// Apply a direction event, execute its actions and check the announcement invariant.
    pub fn dir_event(
        &mut self,
        d: Direction,
        ev: DirectionEvent,
    ) -> Result<(), DirectionRejection> {
        let i = ix(d);
        let (next, actions) = self.dirs[i].clone().on(&self.cfgs[i], ev)?;
        self.dirs[i] = next;
        let mut announced = 0;
        for a in &actions {
            self.log.push(Logged::Dir(d, a.clone()));
            if let DirectionAction::ChangeOutput { from, to, .. } = a {
                assert_eq!(
                    *from, self.heard[i],
                    "{d:?}: announced `from` differs from what was heard"
                );
                assert_ne!(from, to, "{d:?}: a no-op change was announced");
                self.heard[i] = *to;
                announced += 1;
            }
        }
        assert!(
            announced <= 1,
            "{d:?}: more than one output change in one transition"
        );
        assert_eq!(
            self.dirs[i].output(&self.cfgs[i].policy),
            self.heard[i],
            "{d:?}: the audible output changed without an announcement in {:?}",
            self.dirs[i]
        );
        for a in actions {
            if a == DirectionAction::CancelDirection(d) {
                self.seg_quiet(d, SegmentEvent::Cancel(CancelAction::CancelDirection(d)));
            }
        }
        Ok(())
    }

    fn dir_quiet(&mut self, d: Direction, ev: DirectionEvent) {
        if self.dir_event(d, ev).is_err() {
            self.rejections += 1;
        }
    }

    pub fn dir_state(&self, d: Direction) -> &DirectionState {
        &self.dirs[ix(d)]
    }

    pub fn heard(&self, d: Direction) -> OutputSource {
        self.heard[ix(d)]
    }

    /// Every announced output change of `d`, oldest first.
    pub fn announcements(&self, d: Direction) -> Vec<(OutputSource, OutputSource, OutputCause)> {
        self.log
            .iter()
            .filter_map(|l| match l {
                Logged::Dir(
                    dir,
                    DirectionAction::ChangeOutput {
                        from, to, cause, ..
                    },
                ) if *dir == d => Some((*from, *to, *cause)),
                _ => None,
            })
            .collect()
    }

    // ---- segment books --------------------------------------------------

    /// Apply a segment event and check the ordering and late-result invariants.
    pub fn seg_event(
        &mut self,
        d: Direction,
        ev: SegmentEvent,
    ) -> Result<Vec<SegmentAction>, SegmentRejection> {
        let i = ix(d);
        let dead_target = self.targets_dead(d, &ev);
        let actions = self.books[i].on(ev)?;
        if dead_target {
            assert!(
                actions
                    .iter()
                    .all(|a| matches!(a, SegmentAction::DropLate { .. })),
                "{ev:?} targets a cancelled or finished item but produced {actions:?}"
            );
        }
        for a in &actions {
            self.log.push(Logged::Seg(d, *a));
            self.track(d, a);
        }
        self.check_releases(d, &actions);
        Ok(actions)
    }

    fn seg_quiet(&mut self, d: Direction, ev: SegmentEvent) {
        if self.seg_event(d, ev).is_err() {
            self.rejections += 1;
        }
    }

    /// Whether the event addresses a segment or clause that is cancelled, skipped, finished or forgotten.
    fn targets_dead(&self, d: Direction, ev: &SegmentEvent) -> bool {
        let i = ix(d);
        let dead_segment = |s: SegmentId| {
            self.dead_segments[i].contains(&s)
                || self.books[i]
                    .segment_state(s)
                    .is_none_or(|st| st.is_terminal())
        };
        match ev {
            SegmentEvent::Hypothesis { segment, .. }
            | SegmentEvent::CommitClause { segment, .. }
            | SegmentEvent::EndSegment(segment) => dead_segment(*segment),
            SegmentEvent::Clause { clause, .. } => {
                dead_segment(clause.segment()) || self.dead_clauses[i].contains(clause)
            }
            _ => false,
        }
    }

    fn track(&mut self, d: Direction, a: &SegmentAction) {
        let i = ix(d);
        match a {
            SegmentAction::Opened(s) => self.opened[i].push(*s),
            SegmentAction::Cancel(CancelAction::CancelSegment(s)) => {
                self.dead_segments[i].insert(*s);
            }
            SegmentAction::Cancel(CancelAction::CancelDirection(_)) => {
                let all: Vec<SegmentId> = self.opened[i].clone();
                self.dead_segments[i].extend(all);
            }
            SegmentAction::ClauseAdvanced {
                clause,
                state: ClauseState::Cancelled | ClauseState::Skipped,
            } => {
                self.dead_clauses[i].insert(*clause);
            }
            _ => {}
        }
    }

    fn check_releases(&mut self, d: Direction, actions: &[SegmentAction]) {
        let i = ix(d);
        let views = self.books[i].clause_views();
        for a in actions {
            let SegmentAction::ReleaseForPlayback(c) = a else {
                continue;
            };
            assert!(
                !self.dead_clauses[i].contains(c),
                "released a cancelled or skipped clause {c}"
            );
            assert!(
                !self.dead_segments[i].contains(&c.segment()),
                "released a clause of cancelled segment {c}"
            );
            if let Some(last) = self.released[i].last() {
                assert!(
                    c > last,
                    "{d:?}: clause {c} released after {last}: out of order"
                );
            }
            for v in views.iter().filter(|v| v.clause < *c) {
                assert!(
                    v.state.is_terminal(),
                    "{d:?}: {c} released while earlier {} is {:?}",
                    v.clause,
                    v.state
                );
            }
            self.released[i].push(*c);
        }
    }

    pub fn book(&self, d: Direction) -> &SegmentBook {
        &self.books[ix(d)]
    }

    // ---- speech helpers for scenarios ----------------------------------

    /// A speech region starts: open the segment, then apply the same-speaker barge-in decision.
    pub fn speech_start(&mut self, d: Direction) -> SegmentId {
        self.try_speech_start(d).expect("a new segment")
    }

    /// [`Rig::speech_start`] that reports a rejected opening instead of panicking.
    pub fn try_speech_start(&mut self, d: Direction) -> Result<SegmentId, SegmentRejection> {
        let acts = self.seg_event(d, SegmentEvent::SpeechStarted { at: self.now })?;
        let SegmentAction::Opened(id) = acts[0] else {
            panic!("no Opened in {acts:?}")
        };
        let views = self.books[ix(d)].clause_views();
        let cancels = decide_interrupt(
            InterruptTrigger::SameSpeakerNewSegment { new_segment: id },
            &self.interrupt,
            self.now,
            &views,
        );
        for c in cancels {
            self.seg_event(d, SegmentEvent::Cancel(c))
                .expect("a decided cancel is legal");
        }
        Ok(id)
    }

    /// Commit the next clause of `segment` whose speech ends now.
    pub fn commit(&mut self, d: Direction, segment: SegmentId, revision: u32) -> ClauseId {
        let acts = self
            .seg_event(
                d,
                SegmentEvent::CommitClause {
                    segment,
                    revision,
                    source_end: self.now,
                },
            )
            .expect("commit");
        let SegmentAction::ClauseCommitted(c) = acts[0] else {
            panic!("no ClauseCommitted in {acts:?}")
        };
        c
    }

    pub fn clause(
        &mut self,
        d: Direction,
        clause: ClauseId,
        kinds: &[ClauseEventKind],
    ) -> Vec<SegmentAction> {
        let mut all = Vec::new();
        for k in kinds {
            let a = self
                .seg_event(d, SegmentEvent::Clause { clause, kind: *k })
                .unwrap_or_else(|r| panic!("{clause} {k:?}: {r}"));
            all.extend(a);
        }
        all
    }

    /// Recognised, translated and synthesised, first audio ready.
    pub fn ready_to_play(&mut self, d: Direction, clause: ClauseId) -> Vec<SegmentAction> {
        use ClauseEventKind as K;
        self.clause(
            d,
            clause,
            &[
                K::Recognized,
                K::TranslationStarted,
                K::Translated,
                K::SynthesisStarted,
                K::AudioReady,
            ],
        )
    }

    pub fn end_segment(&mut self, d: Direction, segment: SegmentId) {
        self.seg_event(d, SegmentEvent::EndSegment(segment))
            .expect("end");
    }

    pub fn state_of(&self, d: Direction, clause: ClauseId) -> Option<ClauseState> {
        self.book(d).clause_state(clause)
    }

    pub fn blocker() -> Blocker {
        Blocker::NoMicrophone
    }
}

pub fn released_in(actions: &[SegmentAction]) -> Vec<ClauseId> {
    actions
        .iter()
        .filter_map(|a| {
            if let SegmentAction::ReleaseForPlayback(c) = a {
                Some(*c)
            } else {
                None
            }
        })
        .collect()
}
