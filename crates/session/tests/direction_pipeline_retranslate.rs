//! End-to-end tests of incremental re-translation: a long utterance is cut and spoken while the speaker goes on, with a translator whose answers the test controls and the fake recogniser on a manual clock.

mod pipeline_rig;

use std::sync::Arc;

use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};
use prxvc_ports::fakes::{
    AsrScript, FakeSpeechSynth, FakeStreamingAsr, FakeTraceSink, FakeTranslator, FakeVadProvider,
    MtScript, TtsScript, VadScript,
};
use prxvc_ports::{
    CancellationToken, MtDelta, MtError, MtRequest, MtStream, ProviderDescriptor, StreamingKind,
    Translator,
};
use prxvc_session::pipeline::{DirectionSnapshot, PipelineEvent, PipelineProviders};
use prxvc_session::{ClauseState, EarlyCommit, RetranslateParams};

use pipeline_rig::{Fakes, Rig, params, vad_runs};

const IN: Direction = Direction::Incoming;

/// Twelve English words and what each says in Chinese: a translation that only ever grows.
const WORDS: [(&str, &str); 12] = [
    ("alpha", "阿尔"),
    ("bravo", "布拉"),
    ("charlie", "查理"),
    ("delta", "德尔"),
    ("echo", "回声"),
    ("foxtrot", "狐步"),
    ("golf", "高尔"),
    ("hotel", "酒店"),
    ("india", "印度"),
    ("juliet", "朱丽"),
    ("kilo", "基洛"),
    ("lima", "利马"),
];

/// Translates word by word. With `diverge`, a long text that ends in the last word is translated
/// with a different first word, so that its translation no longer begins with the earlier one.
#[derive(Clone, Default)]
struct Words {
    diverge: bool,
}

impl Words {
    fn translation(&self, text: &str) -> String {
        let diverges = self.diverge && text.starts_with("alpha") && text.contains("lima");
        text.split_whitespace()
            .enumerate()
            .map(|(i, w)| {
                if diverges && i == 0 {
                    return "某某";
                }
                WORDS
                    .iter()
                    .find(|(e, _)| *e == w.to_lowercase())
                    .map_or("某某", |(_, z)| z)
            })
            .collect()
    }
}

struct Reply {
    text: String,
    sent: u8,
}

impl MtStream for Reply {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>> {
        if cancel.is_cancelled() {
            return Some(Err(MtError::Cancelled));
        }
        self.sent += 1;
        match self.sent {
            1 => Some(Ok(MtDelta::Text(self.text.clone()))),
            2 => Some(Ok(MtDelta::Done {
                prompt_tokens: 0,
                completion_tokens: 0,
            })),
            _ => None,
        }
    }
}

impl Translator for Words {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new("words", "test", StreamingKind::Native)
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        Ok(Box::new(Reply {
            text: self.translation(&req.text),
            sent: 0,
        }))
    }
}

fn sentence() -> String {
    WORDS.map(|(e, _)| e).join(" ")
}

/// One reading per frame, `per_word` frames for each added word, no stable prefix.
fn growing(per_word: usize) -> Vec<(String, usize)> {
    let words: Vec<&str> = WORDS.map(|(e, _)| e).to_vec();
    (1..=words.len())
        .flat_map(|upto| std::iter::repeat_n((words[..upto].join(" "), 0), per_word))
        .collect()
}

const SPEECH_FRAMES: usize = 120;
const LEAD: usize = 10;
const TAIL: usize = 25;

fn long_utterance() -> (AsrScript, VadScript) {
    let mut asr = AsrScript::new([], &sentence()).then_round([], "");
    asr.partials = growing(10);
    (
        asr,
        vad_runs(&[(0.0, LEAD), (0.9, SPEECH_FRAMES), (0.0, TAIL)]),
    )
}

/// An incoming (English to Chinese) pipeline with re-translation on, translating through `words`.
fn start(clock: &ManualClock, words: &Words) -> Rig {
    let (asr, vad) = long_utterance();
    let fakes = Fakes {
        vad: FakeVadProvider::new(vad),
        asr: FakeStreamingAsr::new(clock.clone(), asr),
        mt: FakeTranslator::new(clock.clone()),
        tts: FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400)),
    };
    let mut p = params(IN);
    p.retranslate = RetranslateParams {
        enabled: true,
        continuation: true,
        ..RetranslateParams::disabled()
    };
    p.commit.early = EarlyCommit::Punctuation;
    // The segment ends before a pause inside it could commit the rest as a unit of its own.
    p.commit.clause_pause = std::time::Duration::from_secs(5);
    let providers = PipelineProviders {
        vad: Arc::new(fakes.vad.clone()),
        asr: Arc::new(fakes.asr.clone()),
        refine: None,
        mt: Arc::new(words.clone()),
        tts: Arc::new(fakes.tts.clone()),
    };
    Rig::launch_with_providers(
        clock.clone(),
        fakes,
        providers,
        p,
        &CancellationToken::new(),
        FakeTraceSink::new(true),
    )
}

fn first_clause() -> ClauseId {
    ClauseId::new(SegmentId::new(IN, 1).expect("seq >= 1"), 0)
}

/// Every clause of the snapshot has its translation (a clause that ended its sentence after the
/// part already spoken has the rest of it).
fn translated(s: &DirectionSnapshot) -> bool {
    s.clauses.len() >= 2
        && s.clauses.len() as u64 == s.counters.clauses_committed
        && s.clauses.iter().all(|c| {
            !c.translation.is_empty()
                && matches!(
                    c.state,
                    ClauseState::Translated
                        | ClauseState::Synthesizing
                        | ClauseState::Playing
                        | ClauseState::Played
                )
        })
}

fn spoken(s: &DirectionSnapshot) -> String {
    s.clauses.iter().map(|c| c.translation.as_str()).collect()
}

#[test]
fn a_long_utterance_is_first_heard_from_a_re_translation_unit_before_the_segment_ends() {
    let clock = ManualClock::new();
    let words = Words::default();
    let rig = start(&clock, &words);
    rig.wait_running();
    // Speech only: the segment cannot end before this returns.
    rig.feed_settling_drafts((LEAD + SPEECH_FRAMES) as u64);
    let first = first_clause();
    rig.wait_events("translated audio while the speaker is still talking", |e| {
        e.iter()
            .any(|x| matches!(x, PipelineEvent::Audio(a) if a.payload.clause == first))
    });
    let snap = rig.latest();
    assert!(
        snap.counters.retranslated_units >= 1,
        "the first unit came from the rule: {:?}",
        snap.counters
    );
    assert!(
        rig.trace_stages(first)
            .iter()
            .any(|(s, _)| *s == prxvc_ports::TraceStage::RetranslationCut)
    );
    let cut = rig
        .trace
        .events()
        .into_iter()
        .find(|e| e.clause == first && e.stage == prxvc_ports::TraceStage::UnitCut)
        .and_then(|e| e.text);
    assert_eq!(cut.as_deref(), Some("retranslation continues"));
    // The rest of the audio, then the segment ends.
    rig.feed_settling_drafts(TAIL as u64);
    let snap = rig.wait_snapshot("every unit is translated", translated);
    assert_eq!(
        spoken(&snap),
        words.translation(&sentence()),
        "the units' translations are the translation of the sentence"
    );
    assert_eq!(snap.counters.continuation_diverged, 0);
    assert!(snap.counters.continued >= 1, "{:?}", snap.counters);
    assert!(snap.counters.drafts_engine_calls >= 3);
    assert_eq!(snap.counters.drafts_failed, 0);
    let stop = rig.stop();
    assert!(stop.clean, "the draft thread ended with the others");
    assert_eq!(stop.leaked, 0);
}

#[test]
fn a_translation_that_stops_agreeing_with_what_was_spoken_is_translated_alone_and_nothing_is_repeated()
 {
    let clock = ManualClock::new();
    let words = Words { diverge: true };
    let rig = start(&clock, &words);
    rig.wait_running();
    rig.feed_settling_drafts((LEAD + SPEECH_FRAMES + TAIL) as u64);
    let snap = rig.wait_snapshot("every unit is translated", translated);
    assert_eq!(
        snap.counters.continuation_diverged, 1,
        "{:?}",
        snap.counters
    );
    let said = spoken(&snap);
    let pairs: Vec<String> = said
        .chars()
        .collect::<Vec<_>>()
        .chunks(2)
        .map(|c| c.iter().collect())
        .collect();
    let mut unique = pairs.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), pairs.len(), "no word is spoken twice: {said}");
    assert!(
        said.starts_with("阿尔布拉"),
        "what was spoken before the divergence stays: {said}"
    );
    assert!(rig.stop().clean);
}

#[test]
fn the_other_direction_runs_as_before_and_with_the_rule_off_nothing_new_exists() {
    let clock = ManualClock::new();
    let words = Words::default();
    let on = start(&clock, &words);
    let on_queues: Vec<&str> = on.pipeline.stats().queues.iter().map(|q| q.name).collect();
    assert_eq!(&on_queues[6..], ["draft_in", "draft_out"]);
    let draft_in = on.pipeline.stats().queues[6].clone();
    assert_eq!(
        (draft_in.capacity, draft_in.policy),
        (1, prxvc_session::pipeline::Policy::DropOldest)
    );
    assert!(on.stop().clean);

    let mt = FakeTranslator::new(clock.clone())
        .with_script("Hello.", MtScript::tokens(["Hello."]))
        .with_script(
            "今天天气很好",
            MtScript::tokens(["The weather ", "is nice."]),
        );
    let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(1, 2400));
    let off = Rig::start_with(
        &clock,
        Direction::Outgoing,
        pipeline_rig::utterances(1, 0),
        pipeline_rig::asr_script(&["今天天气很好"]),
        mt,
        tts,
        |_| {},
    );
    off.wait_running();
    off.feed(55);
    let snap = off.wait_snapshot("the clause is spoken", |s| {
        s.clauses
            .first()
            .is_some_and(|c| c.translation == "The weather is nice.")
    });
    let stats = off.pipeline.stats();
    let names: Vec<&str> = stats.queues.iter().map(|q| q.name).collect();
    assert_eq!(
        names,
        [
            "audio_in",
            "inbox",
            "refine_in",
            "mt_in",
            "tts_in",
            "events_out"
        ],
        "no draft queue and no draft thread with the rule off"
    );
    assert_eq!(stats.drafts_requested, 0);
    let c = snap.counters;
    assert_eq!(
        (
            c.drafts_requested,
            c.drafts_engine_calls,
            c.retranslated_units,
            c.continued,
            c.continuation_diverged
        ),
        (0, 0, 0, 0, 0)
    );
    let asked: Vec<String> = off
        .fakes
        .mt
        .requests()
        .iter()
        .map(|r| r.text.clone())
        .collect();
    assert_eq!(asked, ["今天天气很好"], "the clause only, as before");
    assert!(off.stop().clean);
}
