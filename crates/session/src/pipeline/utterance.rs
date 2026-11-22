//! What one utterance (a segment) took, for the log and the call summary: the stage times the
//! tracer saw, what happened to its clauses, and the figures kept over the call. Pure data and
//! arithmetic; the control thread logs. No spoken text is held here.

use std::collections::BTreeMap;
use std::fmt;

use prxvc_media::{MonoTime, SegmentId};
use prxvc_ports::{MtFlag, TraceStage};

use super::snapshot::SkipReason;

/// How many segments the tracer keeps times for at once; the oldest are dropped beyond it.
const MAX_OPEN_SEGMENTS: usize = 64;

/// How many latency figures a call keeps per kind (the newest).
const MAX_FIGURES: usize = 2000;

/// The first and last occurrences of the stages of one segment.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SegmentTimes {
    speech_start: Option<MonoTime>,
    first_commit: Option<MonoTime>,
    first_pause: Option<MonoTime>,
    last_pause: Option<MonoTime>,
    asr_final: Option<MonoTime>,
    mt_request: Option<MonoTime>,
    mt_first_token: Option<MonoTime>,
    first_audio: Option<MonoTime>,
    last_audio_end: Option<MonoTime>,
    /// How long the audio of the first clause that had to wait waited for its turn.
    queue_wait: Option<std::time::Duration>,
}

impl SegmentTimes {
    fn note(&mut self, stage: TraceStage, at: MonoTime) {
        fn first(slot: &mut Option<MonoTime>, at: MonoTime) {
            slot.get_or_insert(at);
        }
        match stage {
            TraceStage::SpeechStart => first(&mut self.speech_start, at),
            TraceStage::ClauseCommitted => first(&mut self.first_commit, at),
            TraceStage::ClausePause => {
                first(&mut self.first_pause, at);
                self.last_pause = Some(at);
            }
            TraceStage::AsrFinalReady => first(&mut self.asr_final, at),
            TraceStage::MtRequestSent => first(&mut self.mt_request, at),
            TraceStage::MtFirstToken => first(&mut self.mt_first_token, at),
            TraceStage::PlayoutStart => first(&mut self.first_audio, at),
            TraceStage::PlayoutEnd => self.last_audio_end = Some(at),
            _ => {}
        }
    }
}

/// The times of the segments that are still open, written by the tracer from every stage thread.
#[derive(Debug, Default)]
pub(crate) struct OpenTimes(BTreeMap<SegmentId, SegmentTimes>);

impl OpenTimes {
    pub(crate) fn note(&mut self, segment: SegmentId, stage: TraceStage, at: MonoTime) {
        self.0.entry(segment).or_default().note(stage, at);
        while self.0.len() > MAX_OPEN_SEGMENTS {
            self.0.pop_first();
        }
    }

    pub(crate) fn note_wait(&mut self, segment: SegmentId, wait: std::time::Duration) {
        self.0.entry(segment).or_default().queue_wait.get_or_insert(wait);
    }

    pub(crate) fn take(&mut self, segment: SegmentId) -> SegmentTimes {
        self.0.remove(&segment).unwrap_or_default()
    }
}

/// What happened to the clauses of one segment, counted as they ended.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SegmentTally {
    pub(crate) clauses: u32,
    pub(crate) played: u32,
    pub(crate) failed: u32,
    pub(crate) cancelled: u32,
    pub(crate) skipped: BTreeMap<&'static str, u32>,
    pub(crate) flags: Vec<MtFlag>,
    pub(crate) retried: u32,
    pub(crate) suspect: u32,
    /// The name of the reason the segment's first unit was committed for.
    pub(crate) first_cut: Option<&'static str>,
    /// Units committed by the re-translation rule.
    pub(crate) retranslated: u32,
    /// Engine calls made for drafts while the segment was open.
    pub(crate) drafts: u32,
    /// Draft texts served from the cache.
    pub(crate) draft_hits: u32,
    /// Draft replies discarded as stale.
    pub(crate) drafts_stale: u32,
    /// Units whose translation continued the sentence.
    pub(crate) continued: u32,
    /// Units whose translation no longer began with the part already spoken.
    pub(crate) diverged: u32,
    /// Units that continued the sentence with nothing left to say.
    pub(crate) remainder_empty: u32,
    /// Partials or finals that contradicted text committed by the re-translation rule.
    pub(crate) src_rewrites: u32,
}

/// The name of a skip reason in the log.
pub(crate) const fn skip_name(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::Stale => "stale",
        SkipReason::Backlog => "backlog",
        SkipReason::QueueFull => "queue_full",
        SkipReason::SourceInTarget => "in_target",
        SkipReason::SelfEcho => "self_echo",
    }
}

/// Milliseconds between two times; `None` when either is missing or they are out of order.
fn between(from: Option<MonoTime>, to: Option<MonoTime>) -> Option<u64> {
    let (from, to) = (from?, to?);
    (to >= from).then(|| u64::try_from(to.saturating_since(from).as_millis()).unwrap_or(u64::MAX))
}

/// One finished utterance as the log line says it. Every time is milliseconds after the speech
/// began, except where the name says otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Utterance {
    pub(crate) segment: SegmentId,
    pub(crate) tally: SegmentTally,
    /// The speech itself: its start to the last pause that ended a clause.
    pub(crate) speech_ms: Option<u64>,
    /// The speech start to the first unit committed.
    pub(crate) first_commit_ms: Option<u64>,
    /// Recognition final of the first clause.
    pub(crate) asr_final_ms: Option<u64>,
    /// Translation: request sent to first token, of the first clause that got one.
    pub(crate) mt_first_token_ms: Option<u64>,
    /// The first translated sample played.
    pub(crate) first_audio_ms: Option<u64>,
    /// The last clause finished playing.
    pub(crate) playback_end_ms: Option<u64>,
    /// The end of the first clause's speech to the first translated sample, the figure the
    /// listener feels as "the wait".
    pub(crate) pause_to_first_audio_ms: Option<u64>,
    /// How long the first clause's synthesised audio waited behind earlier speech before it was
    /// released to play: waiting for its turn, not processing.
    pub(crate) queue_wait_ms: Option<u64>,
}

impl Utterance {
    pub(crate) fn new(segment: SegmentId, tally: SegmentTally, t: &SegmentTimes) -> Self {
        Self {
            segment,
            tally,
            speech_ms: between(t.speech_start, t.last_pause),
            first_commit_ms: between(t.speech_start, t.first_commit),
            asr_final_ms: between(t.speech_start, t.asr_final),
            mt_first_token_ms: between(t.mt_request, t.mt_first_token),
            first_audio_ms: between(t.speech_start, t.first_audio),
            playback_end_ms: between(t.speech_start, t.last_audio_end),
            pause_to_first_audio_ms: between(t.first_pause, t.first_audio),
            queue_wait_ms: t
                .queue_wait
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        }
    }

    /// How it ended: `played`, or what happened instead.
    pub(crate) fn outcome(&self) -> &'static str {
        let t = &self.tally;
        if t.failed > 0 {
            "failed"
        } else if t.played > 0 {
            "played"
        } else if t.skipped.get("in_target").is_some_and(|n| *n > 0) {
            "passed_through"
        } else if !t.skipped.is_empty() {
            "skipped"
        } else {
            "cancelled"
        }
    }
}

/// A figure or a dash.
pub(crate) struct Ms(pub(crate) Option<u64>);

impl fmt::Display for Ms {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(v) => write!(f, "{v}"),
            None => f.write_str("-"),
        }
    }
}

/// The flags and skips of a tally as one short text: `wrong_language,in_target=1`.
pub(crate) fn notes(t: &SegmentTally) -> String {
    let mut parts: Vec<String> = t.flags.iter().map(|f| f.name().to_owned()).collect();
    parts.extend(t.skipped.iter().map(|(k, n)| format!("skip_{k}={n}")));
    if t.retried > 0 {
        parts.push(format!("retried={}", t.retried));
    }
    if t.suspect > 0 {
        parts.push(format!("suspect={}", t.suspect));
    }
    if parts.is_empty() {
        "-".to_owned()
    } else {
        parts.join(",")
    }
}

/// The median and the 95th percentile of a set of figures, in whole milliseconds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Percentiles {
    /// How many figures.
    pub n: u32,
    /// The median.
    pub median_ms: u64,
    /// The 95th percentile (nearest rank).
    pub p95_ms: u64,
}

impl Percentiles {
    /// The percentiles of `figures` (unsorted); all zero for none.
    pub fn of(figures: &[u64]) -> Self {
        if figures.is_empty() {
            return Self::default();
        }
        let mut v = figures.to_vec();
        v.sort_unstable();
        let rank = |p: usize| {
            // Nearest rank: the smallest value with at least p percent of the figures at or below it.
            let idx = (v.len() * p).div_ceil(100).max(1) - 1;
            v[idx.min(v.len() - 1)]
        };
        let median = if v.len() % 2 == 1 {
            v[v.len() / 2]
        } else {
            (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2
        };
        Self {
            n: u32::try_from(v.len()).unwrap_or(u32::MAX),
            median_ms: median,
            p95_ms: rank(95),
        }
    }
}

/// The figures of a call's utterances, for the summary at its end.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ledger {
    /// Utterances that finished with at least one clause.
    pub utterances: u64,
    /// Speech start to the first translated sample.
    pub start_to_first_audio: Percentiles,
    /// The end of the first clause's speech to the first translated sample.
    pub end_to_first_audio: Percentiles,
    /// How long the first clause of an utterance waited behind earlier translated speech (not
    /// processing time: translation and synthesis were done).
    pub queue_wait: Percentiles,
    /// Speech start to the first unit committed.
    pub first_commit: Percentiles,
    /// Engine time of one draft translation (re-translation).
    pub draft_ms: Percentiles,
    /// Clauses released at a speed above 1 while a backlog was caught up.
    pub sped_clauses: u64,
    /// The highest speed used, in percent (125 is 1.25x); 100 when none.
    pub max_speed_pct: u32,
}

/// Keep the newest [`MAX_FIGURES`] figures.
fn push_bounded(list: &mut Vec<u64>, figure: u64) {
    list.push(figure);
    if list.len() > MAX_FIGURES {
        list.remove(0);
    }
}

/// The figure lists behind a [`Ledger`].
#[derive(Debug, Default)]
pub(crate) struct LedgerBook {
    utterances: u64,
    start: Vec<u64>,
    end: Vec<u64>,
    wait: Vec<u64>,
    first_commit: Vec<u64>,
    draft_ms: Vec<u64>,
    sped: u64,
    max_speed_pct: u32,
}

impl LedgerBook {
    pub(crate) fn record(&mut self, u: &Utterance) {
        self.utterances += 1;
        for (list, figure) in [
            (&mut self.start, u.first_audio_ms),
            (&mut self.end, u.pause_to_first_audio_ms),
            (&mut self.wait, u.queue_wait_ms),
            (&mut self.first_commit, u.first_commit_ms),
        ] {
            if let Some(v) = figure {
                push_bounded(list, v);
            }
        }
    }

    /// The engine times of the draft calls of one segment.
    pub(crate) fn note_drafts(&mut self, engine_ms: &[u64]) {
        for v in engine_ms {
            push_bounded(&mut self.draft_ms, *v);
        }
    }

    /// A clause was released at `speed` (above 1).
    pub(crate) fn note_sped(&mut self, speed: f32) {
        self.sped += 1;
        self.max_speed_pct = self.max_speed_pct.max((speed * 100.0).round() as u32);
    }

    pub(crate) fn snapshot(&self) -> Ledger {
        Ledger {
            utterances: self.utterances,
            start_to_first_audio: Percentiles::of(&self.start),
            end_to_first_audio: Percentiles::of(&self.end),
            queue_wait: Percentiles::of(&self.wait),
            first_commit: Percentiles::of(&self.first_commit),
            draft_ms: Percentiles::of(&self.draft_ms),
            sped_clauses: self.sped,
            max_speed_pct: self.max_speed_pct.max(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use prxvc_media::Direction;

    use super::*;

    fn seg(n: u64) -> SegmentId {
        SegmentId::new(Direction::Incoming, n).unwrap()
    }

    fn at(ms: u64) -> MonoTime {
        MonoTime::from_millis(ms)
    }

    #[test]
    fn percentiles_use_the_nearest_rank_and_the_mean_of_the_middle_pair() {
        assert_eq!(Percentiles::of(&[]), Percentiles::default());
        let one = Percentiles::of(&[700]);
        assert_eq!((one.n, one.median_ms, one.p95_ms), (1, 700, 700));
        let even = Percentiles::of(&[400, 100, 300, 200]);
        assert_eq!((even.median_ms, even.p95_ms), (250, 400));
        let twenty: Vec<u64> = (1..=20).map(|v| v * 10).collect();
        let p = Percentiles::of(&twenty);
        assert_eq!((p.median_ms, p.p95_ms), (105, 190));
    }

    #[test]
    fn the_times_of_a_segment_become_the_figures_of_the_log_line() {
        let mut open = OpenTimes::default();
        let s = seg(5);
        for (stage, ms) in [
            (TraceStage::SpeechStart, 1000),
            (TraceStage::ClauseCommitted, 2100),
            (TraceStage::ClausePause, 2500),
            (TraceStage::AsrFinalReady, 2900),
            (TraceStage::MtRequestSent, 2910),
            (TraceStage::MtFirstToken, 3050),
            (TraceStage::PlayoutStart, 3600),
            (TraceStage::ClausePause, 4200),
            (TraceStage::PlayoutEnd, 5100),
            (TraceStage::PlayoutEnd, 6000),
        ] {
            open.note(s, stage, at(ms));
        }
        let tally = SegmentTally {
            clauses: 2,
            played: 2,
            ..SegmentTally::default()
        };
        let u = Utterance::new(s, tally, &open.take(s));
        assert_eq!(u.speech_ms, Some(3200));
        assert_eq!(
            u.first_commit_ms,
            Some(1100),
            "the first unit, not a later one"
        );
        assert_eq!(u.asr_final_ms, Some(1900));
        assert_eq!(u.mt_first_token_ms, Some(140));
        assert_eq!(u.first_audio_ms, Some(2600));
        assert_eq!(u.playback_end_ms, Some(5000));
        assert_eq!(u.pause_to_first_audio_ms, Some(1100));
        assert_eq!(u.outcome(), "played");
        assert_eq!(open.take(s), SegmentTimes::default(), "taken times are gone");
    }

    #[test]
    fn the_queue_wait_is_the_first_clause_that_waited_and_has_a_figure_of_its_own() {
        let mut open = OpenTimes::default();
        let s = seg(7);
        open.note(s, TraceStage::SpeechStart, at(1000));
        open.note_wait(s, std::time::Duration::from_millis(2300));
        open.note_wait(s, std::time::Duration::from_millis(100));
        let u = Utterance::new(s, SegmentTally::default(), &open.take(s));
        assert_eq!(u.queue_wait_ms, Some(2300));
        let mut book = LedgerBook::default();
        book.record(&u);
        book.note_sped(1.25);
        let l = book.snapshot();
        assert_eq!((l.queue_wait.n, l.queue_wait.median_ms), (1, 2300));
        assert_eq!((l.sped_clauses, l.max_speed_pct), (1, 125));
        let none = Utterance::new(seg(1), SegmentTally::default(), &SegmentTimes::default());
        assert_eq!(none.queue_wait_ms, None);
    }

    #[test]
    fn the_ledger_keeps_the_first_commit_and_the_draft_times() {
        let mut book = LedgerBook::default();
        let mut u = Utterance::new(seg(1), SegmentTally::default(), &SegmentTimes::default());
        u.first_commit_ms = Some(1900);
        book.record(&u);
        book.note_drafts(&[80, 120, 100]);
        let l = book.snapshot();
        assert_eq!((l.first_commit.n, l.first_commit.median_ms), (1, 1900));
        assert_eq!(
            (l.draft_ms.n, l.draft_ms.median_ms, l.draft_ms.p95_ms),
            (3, 100, 120)
        );
    }

    #[test]
    fn a_segment_with_missing_stages_has_dashes_not_guesses() {
        let u = Utterance::new(seg(1), SegmentTally::default(), &SegmentTimes::default());
        assert_eq!(Ms(u.first_audio_ms).to_string(), "-");
        assert_eq!(Ms(Some(12)).to_string(), "12");
        assert_eq!(u.outcome(), "cancelled");
    }

    #[test]
    fn the_outcome_names_what_became_of_the_clauses() {
        let mut t = SegmentTally {
            clauses: 1,
            ..SegmentTally::default()
        };
        t.skipped.insert("in_target", 1);
        let u = Utterance::new(seg(1), t.clone(), &SegmentTimes::default());
        assert_eq!(u.outcome(), "passed_through");
        t.skipped.clear();
        t.skipped.insert("backlog", 1);
        assert_eq!(
            Utterance::new(seg(1), t.clone(), &SegmentTimes::default()).outcome(),
            "skipped"
        );
        t.failed = 1;
        assert_eq!(
            Utterance::new(seg(1), t, &SegmentTimes::default()).outcome(),
            "failed"
        );
    }

    #[test]
    fn the_notes_list_flags_skips_and_retries() {
        let mut t = SegmentTally::default();
        assert_eq!(notes(&t), "-");
        t.flags.push(MtFlag::WrongLanguage);
        t.skipped.insert("stale", 2);
        t.retried = 1;
        assert_eq!(notes(&t), "wrong_language,skip_stale=2,retried=1");
    }

    #[test]
    fn the_ledger_keeps_the_newest_figures_and_ignores_missing_ones() {
        let mut book = LedgerBook::default();
        let mut u = Utterance::new(seg(1), SegmentTally::default(), &SegmentTimes::default());
        book.record(&u);
        assert_eq!(book.snapshot().start_to_first_audio.n, 0);
        u.first_audio_ms = Some(2000);
        u.pause_to_first_audio_ms = Some(900);
        for _ in 0..(MAX_FIGURES + 5) {
            book.record(&u);
        }
        let l = book.snapshot();
        assert_eq!(l.utterances, (MAX_FIGURES + 6) as u64);
        assert_eq!(l.start_to_first_audio.n as usize, MAX_FIGURES);
        assert_eq!(l.end_to_first_audio.median_ms, 900);
    }

    #[test]
    fn open_times_are_bounded() {
        let mut open = OpenTimes::default();
        for n in 1..=(MAX_OPEN_SEGMENTS as u64 + 10) {
            open.note(seg(n), TraceStage::SpeechStart, at(n));
        }
        assert_eq!(open.0.len(), MAX_OPEN_SEGMENTS);
        assert!(!open.0.contains_key(&seg(1)));
    }
}
