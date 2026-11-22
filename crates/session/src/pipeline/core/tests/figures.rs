//! The re-translation figures of an utterance: the tally of its segment, the log line that says them and the absence of any spoken text in it.

use std::sync::Mutex;

use tracing::field::{Field, Visit};

use super::*;
use crate::pipeline::messages::DraftTally;

/// Records the fields of every event as `name=value` text.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<Line>>>);

/// The fields of one event as `(name, value)`.
type Line = Vec<(String, String)>;

struct Fields<'a>(&'a mut Vec<(String, String)>);

impl Visit for Fields<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push((field.name().to_owned(), format!("{value:?}")));
    }
}

impl tracing::Subscriber for Capture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }

    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}

    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let mut fields = Vec::new();
        event.record(&mut Fields(&mut fields));
        self.0.lock().expect("capture lock").push(fields);
    }

    fn enter(&self, _: &tracing::span::Id) {}

    fn exit(&self, _: &tracing::span::Id) {}
}

const SPOKEN: &str = "秘密的原文只有说话的人知道";

fn commit_with(h: &mut Harness, reason: CommitReason, start_ms: u64, end_ms: u64) {
    let c = Commit {
        text: SPOKEN.to_owned(),
        source_start: MonoTime::from_millis(start_ms),
        source_end: MonoTime::from_millis(end_ms),
        audio: None::<Arc<ClauseSamples>>,
        reason,
        continues: true,
    };
    let e = Envelope::new(
        seg(1),
        1,
        MonoTime::from_millis(100),
        RecognitionEvent::Commit(c),
    );
    h.core.handle(Inbound::Recognition(e));
}

fn utterance_line(capture: &Capture) -> Line {
    capture
        .0
        .lock()
        .expect("capture lock")
        .iter()
        .find(|fields| {
            fields
                .iter()
                .any(|(k, v)| k == "message" && v.contains("utterance finished"))
        })
        .cloned()
        .expect("the utterance line was logged")
}

#[test]
fn the_tally_of_a_segment_holds_the_figures_of_its_units_and_its_drafts_and_no_text() {
    let mut h = harness(|_| {});
    h.speech(1);
    commit_with(&mut h, CommitReason::Retranslation, 100, 600);
    commit_with(&mut h, CommitReason::Punctuation, 600, 1_100);
    let id = cl(1, 0);
    h.stage(
        id,
        StageEvent::MtContinuation {
            clause: id,
            outcome: ContinuationOutcome::Continued,
        },
    );
    let id1 = cl(1, 1);
    h.stage(
        id1,
        StageEvent::MtContinuation {
            clause: id1,
            outcome: ContinuationOutcome::Diverged,
        },
    );
    h.stage(
        id1,
        StageEvent::MtContinuation {
            clause: id1,
            outcome: ContinuationOutcome::ContinuedApprox,
        },
    );
    let e = Envelope::new(
        seg(1),
        2,
        MonoTime::from_millis(100),
        RecognitionEvent::DraftTally(DraftTally {
            drafts: 5,
            hits: 2,
            stale: 1,
            src_rewrites: 1,
            engine_ms: vec![80, 120, 90],
        }),
    );
    h.core.handle(Inbound::Recognition(e));
    let tally = h
        .core
        .tallies
        .get(&seg(1))
        .expect("the segment's tally")
        .clone();
    assert_eq!(tally.clauses, 2);
    assert_eq!(tally.first_cut, Some("retranslation"));
    assert_eq!(tally.retranslated, 1);
    assert_eq!(
        (
            tally.drafts,
            tally.draft_hits,
            tally.drafts_stale,
            tally.src_rewrites
        ),
        (5, 2, 1, 1)
    );
    assert_eq!(
        (
            tally.continued,
            tally.continued_approx,
            tally.diverged,
            tally.remainder_empty
        ),
        (1, 1, 1, 0)
    );
    assert!(
        !format!("{tally:?}").contains(SPOKEN),
        "a tally holds figures only"
    );
    let ledger = h.snapshot().ledger.clone();
    assert_eq!(ledger.draft_ms.n, 3);
    assert_eq!(ledger.draft_ms.median_ms, 90);
}

#[test]
fn the_utterance_line_names_the_new_figures_and_carries_no_spoken_text() {
    let capture = Capture::default();
    let mut h = harness(|_| {});
    tracing::subscriber::with_default(capture.clone(), || {
        h.speech(1);
        commit_with(&mut h, CommitReason::Retranslation, 100, 600);
        let c = cl(1, 0);
        h.translate(c);
        h.chunk(c, 100);
        h.stage(c, StageEvent::TtsDone { clause: c });
        let end = Envelope::new(
            seg(1),
            2,
            MonoTime::from_millis(100),
            RecognitionEvent::EndSegment,
        );
        h.core.handle(Inbound::Recognition(end));
        let at = MonoTime::from_millis(1);
        h.command(Command::PlaybackStarted { clause: c, at });
        h.command(Command::PlaybackFinished { clause: c, at });
    });
    let line = utterance_line(&capture);
    let get = |name: &str| {
        line.iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("{name} missing from {line:?}"))
    };
    assert_eq!(get("first_cut"), "\"retranslation\"");
    assert_eq!(get("retranslated"), "1");
    for name in [
        "first_commit_ms",
        "drafts",
        "draft_hits",
        "drafts_stale",
        "continued",
        "continued_approx",
        "diverged",
        "remainder_empty",
        "src_rewrites",
    ] {
        get(name);
    }
    assert!(
        line.iter()
            .all(|(_, v)| !v.contains(SPOKEN) && !v.contains("Hi.")),
        "no text in the line: {line:?}"
    );
}
