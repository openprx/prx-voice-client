//! The recognition stage on its own, with the fake recogniser and VAD, a manual clock and an inline draft link: re-translation cuts, skip rules and stale replies.

use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{AudioFrame, Direction, Lang, ManualClock, MonoTime, SampleRate};
use prxvc_ports::fakes::{AsrScript, FakeStreamingAsr, FakeTraceSink, FakeVadProvider, VadScript};
use prxvc_ports::{
    AsrContext, CancellationToken, MtError, MtRequest, MtStream, ProviderDescriptor, StreamingAsr,
    StreamingKind, Translator, VadProvider, VoiceSelection,
};

use super::*;
use crate::commit_policy::{EarlyCommit, RetranslateParams};
use crate::pipeline::draft::{DraftWorker, InlineLink};
use crate::pipeline::draft_cache::{Cached, CachedStream, DraftCache};

/// Twelve English words and what each one says in Chinese: a translation that only ever grows.
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

struct WordByWord;

impl Translator for WordByWord {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor::new("word-by-word", "test", StreamingKind::Native)
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        let text: String = req
            .text
            .split_whitespace()
            .map(|w| {
                let w = w
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                WORDS
                    .iter()
                    .find(|(e, _)| *e == w)
                    .map_or("某某", |(_, z)| z)
            })
            .collect();
        Ok(Box::new(CachedStream::new(Cached {
            text,
            flags: Vec::new(),
        })))
    }
}

fn sentence(words: usize) -> String {
    WORDS[..words]
        .iter()
        .map(|(e, _)| *e)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Growing partials of `text`, word by word, `per_word` identical readings each; no stable prefix.
fn growing(text: &str, per_word: usize) -> Vec<(String, usize)> {
    let words: Vec<&str> = text.split(' ').collect();
    (1..=words.len())
        .flat_map(|upto| std::iter::repeat_n((words[..upto].join(" "), 0), per_word))
        .collect()
}

struct Harness {
    stage: RecognitionStage,
    stats: Arc<Stats>,
    cache: Arc<DraftCache>,
    out: Out,
    next: u64,
}

impl Harness {
    fn new(
        partials: Vec<(String, usize)>,
        speech_frames: usize,
        silence_frames: usize,
        customize: impl FnOnce(&mut PipelineParams),
    ) -> Self {
        let clock = ManualClock::new();
        let mut p = PipelineParams::new(
            Direction::Incoming,
            Lang::En,
            Lang::Zh,
            VoiceSelection::Preset { name: "p".into() },
        );
        p.retranslate = RetranslateParams {
            enabled: true,
            continuation: true,
            ..RetranslateParams::disabled()
        };
        p.commit.clause_pause = Duration::from_secs(30);
        p.commit.segment_end_pause = Duration::from_secs(60);
        customize(&mut p);
        let mut asr = AsrScript::new([], "").then_round([], "");
        asr.final_text = partials.last().map(|(t, _)| t.clone()).unwrap_or_default();
        asr.partials = partials;
        let mut vad = vec![0.0; 10];
        vad.extend(std::iter::repeat_n(0.9, speech_frames));
        vad.extend(std::iter::repeat_n(0.0, silence_frames));
        let vad = FakeVadProvider::new(VadScript::probabilities(vad))
            .open()
            .expect("vad opens");
        let asr = FakeStreamingAsr::new(clock.clone(), asr)
            .open(
                Lang::En,
                AsrContext {
                    previous_text: None,
                    hotwords: Vec::new(),
                },
            )
            .expect("asr opens");
        let tracer = Tracer::new(Arc::new(FakeTraceSink::new(false)), Arc::new(clock.clone()));
        let stats = Arc::new(Stats::default());
        let cache = Arc::new(DraftCache::default());
        let worker = DraftWorker {
            mt: Arc::new(WordByWord),
            p: p.clone(),
            cache: Arc::clone(&cache),
            tracer: tracer.clone(),
            stats: Arc::clone(&stats),
            warned: None,
        };
        let tree = TokenTree::new(CancellationToken::new());
        let stage = RecognitionStage::new(&p, tree, tracer, Arc::clone(&stats), vad, asr)
            .with_drafts(Box::new(InlineLink::new(worker)));
        Self {
            stage,
            stats,
            cache,
            out: Vec::new(),
            next: 0,
        }
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ts = MonoTime::from_millis(self.next * 32);
            let frame = AudioFrame::new(vec![0.1; 512], SampleRate::HZ_16000, ts, self.next)
                .expect("a valid frame");
            self.next += 1;
            self.stage
                .on_frame(&frame, &mut self.out)
                .expect("the frame is processed");
        }
    }

    fn commits(&self) -> Vec<&Commit> {
        self.out
            .iter()
            .filter_map(|e| match &e.payload {
                RecognitionEvent::Commit(c) => Some(c),
                _ => None,
            })
            .collect()
    }

    fn counters(&self) -> crate::pipeline::snapshot::Counters {
        self.stats.counters(0)
    }
}

#[test]
fn a_growing_hypothesis_is_cut_once_the_reference_agrees_twice() {
    let text = sentence(12);
    let mut h = Harness::new(growing(&text, 10), 120, 0, |_| {});
    h.frames(10 + 120);
    let commits = h.commits();
    assert!(
        !commits.is_empty(),
        "a unit was cut while the speaker went on"
    );
    let first = commits[0];
    assert_eq!(first.reason, CommitReason::Retranslation);
    assert!(
        text.starts_with(&first.text) && first.text.split(' ').count() >= 3,
        "a prefix of at least three words: {:?}",
        first.text
    );
    assert!(
        first.text.split(' ').count() < 12,
        "not the whole sentence, which was still being said"
    );
    assert!(first.continues, "the sentence goes on after the cut");
    let c = h.counters();
    assert_eq!(c.retranslated_units as usize, commits.len());
    assert!(c.drafts_engine_calls >= 3, "{c:?}");
    assert_eq!(
        c.drafts_requested, c.drafts_replied,
        "every job was answered"
    );
}

#[test]
fn the_committed_text_is_the_candidate_text_byte_for_byte_and_its_translation_is_cached() {
    let text = sentence(12);
    let mut h = Harness::new(growing(&text, 10), 120, 0, |_| {});
    h.frames(10 + 120);
    let first = h.commits()[0].text.clone();
    let cached = h
        .cache
        .get(&first)
        .expect("the draft of the unit is the cache entry");
    assert!(!cached.text.is_empty());
    let second = h
        .commits()
        .get(1)
        .map(|c| c.text.clone())
        .expect("a second unit");
    // From the second unit on, the key is the whole sentence so far.
    let whole = format!("{first} {second}");
    assert!(
        h.cache.get(&whole).is_some(),
        "the key of a continuing unit is the sentence from its start"
    );
}

#[test]
fn the_last_unit_of_a_segment_does_not_continue() {
    let text = sentence(12);
    let mut h = Harness::new(growing(&text, 10), 120, 120, |p| {
        // The segment ends before a pause inside it could commit the rest as a unit of its own.
        p.commit.clause_pause = Duration::from_secs(5);
        p.commit.segment_end_pause = Duration::from_millis(900);
    });
    h.frames(10 + 120 + 120);
    let ended = h
        .out
        .iter()
        .any(|e| matches!(e.payload, RecognitionEvent::EndSegment));
    assert!(ended, "the segment ended");
    let commits = h.commits();
    let last = commits.last().expect("commits");
    assert_eq!(last.reason, CommitReason::SegmentEnd);
    assert!(!last.continues);
    assert!(
        commits
            .iter()
            .any(|c| c.reason == CommitReason::Retranslation && c.continues),
        "an earlier unit was cut by the rule"
    );
}

#[test]
fn no_draft_is_asked_for_while_the_speaker_is_silent() {
    let text = sentence(12);
    // Speech for the first words, then silence while the recogniser still reads more.
    let mut h = Harness::new(growing(&text, 10), 60, 100, |_| {});
    h.frames(10 + 60);
    let before = h.counters().drafts_requested;
    assert!(
        before >= 1,
        "drafts were asked for while the speaker talked"
    );
    h.frames(100);
    assert_eq!(h.counters().drafts_requested, before, "silence: no new job");
}

#[test]
fn no_draft_is_asked_for_while_the_hypothesis_is_in_the_language_the_listener_hears() {
    let chinese: Vec<(String, usize)> = (1..=12)
        .flat_map(|n| {
            let t: String = WORDS[..n].iter().map(|(_, z)| *z).collect();
            std::iter::repeat_n((t, 0), 10)
        })
        .collect();
    let mut h = Harness::new(chinese, 120, 0, |_| {});
    h.frames(10 + 120);
    assert_eq!(h.counters().drafts_requested, 0);
    assert!(h.commits().is_empty());
}

#[test]
fn drafts_are_at_most_one_per_interval() {
    let text = sentence(12);
    // A new reading every frame (32 ms): only the first falls outside the 300 ms interval.
    let mut h = Harness::new(growing(&text, 1), 12, 0, |_| {});
    h.frames(10 + 12);
    assert_eq!(h.counters().drafts_requested, 1);
}

#[test]
fn a_reply_that_arrives_after_a_punctuation_commit_is_discarded_as_stale() {
    let text = format!("{}. {}", sentence(8), "India juliet kilo");
    let stable = text.find('.').expect("a mark") + 1;
    let partials = vec![(text.clone(), stable); 40];
    let mut h = Harness::new(partials, 40, 0, |p| {
        p.commit.early = EarlyCommit::Punctuation
    });
    h.frames(10 + 40);
    let commits = h.commits();
    assert!(
        commits
            .iter()
            .any(|c| c.reason == CommitReason::Punctuation),
        "the mark was cut by the early rule"
    );
    assert!(
        commits
            .iter()
            .all(|c| c.reason != CommitReason::Retranslation)
    );
    let c = h.counters();
    assert!(c.drafts_stale >= 1, "{c:?}");
}

/// Commit `text` as a unit of the open segment for `reason`, and say whether it continues.
fn continues_after(text: &str, reason: CommitReason) -> bool {
    let mut h = Harness::new(growing(&sentence(2), 10), 120, 0, |_| {});
    h.frames(10 + 5);
    let end = MonoTime::from_millis(500);
    h.stage
        .emit_commit(text.to_owned(), end, false, reason, &mut h.out);
    h.commits()
        .last()
        .map(|c| c.continues)
        .expect("the unit was emitted")
}

#[test]
fn a_unit_that_ends_its_sentence_does_not_continue_it() {
    for (text, reason, expected) in [
        ("我们先开会。", CommitReason::ClausePause, false),
        ("We start the meeting.", CommitReason::ClausePause, false),
        ("Is it ready?\u{201D}", CommitReason::Punctuation, false),
        ("我们先开会，", CommitReason::ClausePause, true),
        ("我们先开会", CommitReason::ClausePause, true),
        ("we start the meeting", CommitReason::Retranslation, true),
        ("we start the meeting.", CommitReason::Retranslation, false),
        ("we start the meeting", CommitReason::SegmentEnd, false),
    ] {
        assert_eq!(
            continues_after(text, reason),
            expected,
            "{text:?} {reason:?}"
        );
    }
}
