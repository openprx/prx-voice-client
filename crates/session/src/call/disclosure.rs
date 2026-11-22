//! The spoken notice to the other party: a short sentence in their language saying the call is being translated by a machine and that they hear a synthetic voice.
//!
//! The notice never goes through the pipeline: it is not translated, not judged as filler, not
//! skipped by the backlog or the barge-in rule. It is synthesised once, while the models load, with
//! the outgoing direction's own synthesiser and voice, and kept in memory. It is queued on the
//! outgoing translated lane as a clause of its own as soon as the speaker's first speech of the
//! session begins (the forwarder calls [`Disclosure::on_speech_start`]), so that it plays while
//! recognition and translation work; if it was not ready then, just before the first translated
//! audio ([`Disclosure::before_audio`]); or again on request ([`Disclosure::play`]). The mixer plays
//! it in order, so the first translation waits only for what is left of the notice when it is
//! ready, once.
//!
//! "Played" means it was audible from its first to its last sample. If the lane was flushed, or
//! the direction stopped being audible (a pause) while it played, it is counted as interrupted and
//! plays again before the next translated sentence.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use prxvc_media::{ClauseId, Direction, Lang, MonoTime, PcmChunk, SegmentId};
use prxvc_ports::{CancellationToken, SpeechSynth, TtsItem, TtsRequest, VoiceSelection};

use super::coordinator::{Coordinator, slot};
use super::error::CallErrorCode;
use super::host::{DirectionBuild, PlaybackReport, Playout};

/// How long the first translated audio waits for a notice that is still being synthesised.
const PREPARE_WAIT: Duration = Duration::from_secs(3);

/// What the host decided about the notice for this call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisclosureSpec {
    /// The notice is on (`call.disclosure`).
    pub enabled: bool,
    /// The sentence, in the language the other party hears.
    pub text: String,
}

/// Where the notice stands, for the snapshot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisclosureState {
    /// Not part of this call (disabled, or the call does not translate).
    #[default]
    Off,
    /// Being synthesised.
    Preparing,
    /// Ready; it plays before the first translated sentence.
    Ready,
    /// Playing now.
    Playing,
    /// It was played.
    Played,
    /// It could not be prepared.
    Failed,
}

/// The notice as the call snapshot shows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DisclosureSnapshot {
    /// The setting is on for this call.
    pub enabled: bool,
    /// Where it stands.
    pub state: DisclosureState,
    /// How many times it was played to the end.
    pub plays: u32,
    /// How many times it was cut short and queued again.
    pub interrupted: u32,
    /// Seconds into the call when it was last played to the end.
    pub last_played_s: Option<u32>,
}

enum Prep {
    Preparing,
    Ready(Arc<Vec<PcmChunk>>),
    Failed(String),
}

struct Inner {
    prep: Prep,
    /// The first play is still owed.
    auto_pending: bool,
    /// The clause now in the lane, and whether the lane was audible when it started.
    playing: Option<(ClauseId, bool)>,
    /// A manual repeat is the one playing (an interruption does not re-arm the automatic play).
    manual: bool,
    plays: u32,
    interrupted: u32,
    last_played: Option<MonoTime>,
    next: u16,
}

struct Shared {
    inner: Mutex<Inner>,
    ready: Condvar,
    /// The outgoing listener hears the translated lane now.
    audible: AtomicBool,
}

/// The notice of one call.
pub struct Disclosure {
    shared: Arc<Shared>,
}

/// The segment number the notice's clauses use: no real segment ever reaches it.
const SEGMENT_SEQ: u64 = u64::MAX;

fn clause_of(n: u16) -> ClauseId {
    ClauseId::new(
        SegmentId::new(Direction::Outgoing, SEGMENT_SEQ).expect("seq >= 1"),
        n,
    )
}

/// Whether `clause` is one of the notice's.
pub(super) fn is_notice(clause: ClauseId) -> bool {
    clause.segment().direction() == Direction::Outgoing && clause.segment().seq() == SEGMENT_SEQ
}

impl Disclosure {
    /// Start synthesising `text` in `lang` with `voice` on `tts`, on a thread of its own. The
    /// result is kept; a failure is kept too and shown by the coordinator.
    pub(super) fn start(
        text: String,
        lang: Lang,
        voice: VoiceSelection,
        tts: Arc<dyn SpeechSynth>,
        cancel: CancellationToken,
    ) -> Self {
        let shared = Arc::new(Shared {
            inner: Mutex::new(Inner {
                prep: Prep::Preparing,
                auto_pending: true,
                playing: None,
                manual: false,
                plays: 0,
                interrupted: 0,
                last_played: None,
                next: 0,
            }),
            ready: Condvar::new(),
            audible: AtomicBool::new(false),
        });
        let worker = Arc::clone(&shared);
        let spawned = std::thread::Builder::new()
            .name("prxvc-call-disclosure".into())
            .spawn(move || {
                let result = synthesise(&*tts, &text, lang, voice, &cancel);
                let mut inner = worker.inner.lock();
                inner.prep = match result {
                    Ok(chunks) => Prep::Ready(Arc::new(chunks)),
                    Err(why) => Prep::Failed(why),
                };
                drop(inner);
                worker.ready.notify_all();
            });
        if let Err(e) = spawned {
            shared.inner.lock().prep = Prep::Failed(format!("cannot start the thread: {e}"));
        }
        Self { shared }
    }

    /// A notice that is ready at once (tests, and a cached one).
    #[cfg(test)]
    pub(super) fn ready(chunks: Vec<PcmChunk>) -> Self {
        let this = Self::start_state(Prep::Ready(Arc::new(chunks)));
        this
    }

    #[cfg(test)]
    fn start_state(prep: Prep) -> Self {
        Self {
            shared: Arc::new(Shared {
                inner: Mutex::new(Inner {
                    prep,
                    auto_pending: true,
                    playing: None,
                    manual: false,
                    plays: 0,
                    interrupted: 0,
                    last_played: None,
                    next: 0,
                }),
                ready: Condvar::new(),
                audible: AtomicBool::new(false),
            }),
        }
    }

    /// What the outgoing listener hears changed: the notice only counts while the translated lane
    /// is audible.
    pub(super) fn set_audible(&self, audible: bool) {
        self.shared.audible.store(audible, Ordering::SeqCst);
    }

    /// The speaker's first speech of the session began: play the notice now when it is owed and
    /// ready, so that it overlaps the time recognition and translation need and the first
    /// translated sentence waits only for what is left of it. Never waits: a notice still being
    /// synthesised plays before the first translated audio instead ([`Self::before_audio`]).
    pub(super) fn on_speech_start(&self, playout: &dyn Playout) {
        let mut inner = self.shared.inner.lock();
        if !inner.auto_pending {
            return;
        }
        if let Prep::Ready(chunks) = &inner.prep {
            let chunks = Arc::clone(chunks);
            inner.auto_pending = false;
            Self::queue(
                &mut inner,
                &chunks,
                playout,
                self.shared.audible.load(Ordering::SeqCst),
                false,
            );
        }
    }

    /// The first translated audio of a clause is about to go to `playout`: play the notice first when
    /// it is still owed. Waits (at most [`PREPARE_WAIT`]) for a notice that is still being
    /// synthesised.
    pub(super) fn before_audio(&self, playout: &dyn Playout) {
        let mut inner = self.shared.inner.lock();
        if !inner.auto_pending {
            return;
        }
        if matches!(inner.prep, Prep::Preparing) {
            self.shared.ready.wait_for(&mut inner, PREPARE_WAIT);
        }
        if let Prep::Ready(chunks) = &inner.prep {
            let chunks = Arc::clone(chunks);
            inner.auto_pending = false;
            Self::queue(
                &mut inner,
                &chunks,
                playout,
                self.shared.audible.load(Ordering::SeqCst),
                false,
            );
        }
    }

    /// Say it again now (queued after what is playing). `false` when it is not ready.
    pub(super) fn play(&self, playout: &dyn Playout) -> bool {
        let mut inner = self.shared.inner.lock();
        let Prep::Ready(chunks) = &inner.prep else {
            return false;
        };
        let chunks = Arc::clone(chunks);
        Self::queue(
            &mut inner,
            &chunks,
            playout,
            self.shared.audible.load(Ordering::SeqCst),
            true,
        );
        true
    }

    fn queue(
        inner: &mut Inner,
        chunks: &[PcmChunk],
        playout: &dyn Playout,
        audible: bool,
        manual: bool,
    ) {
        let id = clause_of(inner.next);
        inner.next = inner.next.wrapping_add(1);
        for c in chunks {
            playout.push(id, c);
        }
        playout.end_clause(id);
        inner.playing = Some((id, audible));
        inner.manual = manual;
    }

    /// A playback report; `true` when it was about the notice (and so is not the pipeline's).
    pub(super) fn report(&self, report: PlaybackReport, clock_now: MonoTime) -> bool {
        let clause = match report {
            PlaybackReport::Started { clause, .. } | PlaybackReport::Finished { clause, .. } => {
                clause
            }
        };
        if !is_notice(clause) {
            return false;
        }
        let audible = self.shared.audible.load(Ordering::SeqCst);
        let mut inner = self.shared.inner.lock();
        match report {
            PlaybackReport::Started { clause, .. } => {
                if let Some((id, was)) = inner.playing.as_mut()
                    && *id == clause
                {
                    *was = audible;
                }
            }
            PlaybackReport::Finished { clause, .. } => {
                if let Some((id, started_audible)) = inner.playing
                    && id == clause
                {
                    inner.playing = None;
                    if started_audible && audible {
                        inner.plays += 1;
                        inner.last_played = Some(clock_now);
                    } else {
                        Self::interrupted(&mut inner);
                    }
                }
            }
        }
        true
    }

    /// The lane was flushed: what was playing is gone.
    pub(super) fn flushed(&self) {
        let mut inner = self.shared.inner.lock();
        if inner.playing.take().is_some() {
            Self::interrupted(&mut inner);
        }
    }

    fn interrupted(inner: &mut Inner) {
        inner.interrupted += 1;
        // A cut notice is owed again before the next translated sentence; a repeat the user asked
        // for is theirs to ask again.
        if !inner.manual && inner.plays == 0 {
            inner.auto_pending = true;
        }
    }

    /// The failure reason, once preparation failed.
    pub(super) fn failure(&self) -> Option<String> {
        match &self.shared.inner.lock().prep {
            Prep::Failed(why) => Some(why.clone()),
            _ => None,
        }
    }

    /// What the snapshot shows; `began` is when the call began on the coordinator's clock.
    pub(super) fn snapshot(&self, began: Option<MonoTime>) -> DisclosureSnapshot {
        let inner = self.shared.inner.lock();
        let state = match (&inner.prep, inner.playing, inner.plays) {
            (Prep::Failed(_), ..) => DisclosureState::Failed,
            (Prep::Preparing, ..) => DisclosureState::Preparing,
            (_, Some(_), _) => DisclosureState::Playing,
            (_, None, 0) => DisclosureState::Ready,
            (_, None, _) => DisclosureState::Played,
        };
        DisclosureSnapshot {
            enabled: true,
            state,
            plays: inner.plays,
            interrupted: inner.interrupted,
            last_played_s: inner
                .last_played
                .zip(began)
                .map(|(at, b)| u32::try_from(at.saturating_since(b).as_secs()).unwrap_or(u32::MAX)),
        }
    }
}

/// Synthesise the notice into memory.
fn synthesise(
    tts: &dyn SpeechSynth,
    text: &str,
    lang: Lang,
    voice: VoiceSelection,
    cancel: &CancellationToken,
) -> Result<Vec<PcmChunk>, String> {
    let request = TtsRequest {
        clause: clause_of(0),
        text: text.to_owned(),
        lang,
        voice,
    };
    let mut stream = tts.synthesize(request).map_err(|e| e.to_string())?;
    let mut chunks = Vec::new();
    loop {
        match stream.next(cancel) {
            Some(Ok(TtsItem::Chunk(c))) => chunks.push(c),
            Some(Ok(TtsItem::Done { .. })) => break,
            Some(Err(e)) => return Err(e.to_string()),
            None => return Err("synthesis ended early".to_owned()),
        }
    }
    if chunks.is_empty() {
        return Err("synthesis produced no audio".to_owned());
    }
    Ok(chunks)
}

impl Coordinator {
    /// Begin preparing the notice for a call that translates the outgoing direction (while the models
    /// load), when the host says it is on.
    pub(super) fn start_disclosure(&mut self, build: &DirectionBuild) {
        let Some(spec) = &build.disclosure else {
            return;
        };
        if !spec.enabled {
            tracing::info!("the spoken notice to the other party is off for this call");
            return;
        }
        self.disclosure = Some(Arc::new(Disclosure::start(
            spec.text.clone(),
            build.params.target,
            build.params.voice.clone(),
            Arc::clone(&build.providers.tts),
            self.call_cancel.clone(),
        )));
    }

    /// `SayDisclosure`: queue the notice on the outgoing lane now.
    pub(super) fn say_disclosure(&mut self) {
        let playout = self.lanes[slot(Direction::Outgoing)]
            .running
            .as_ref()
            .and_then(|r| r.playout());
        let played = match (&self.disclosure, playout) {
            (Some(d), Some(p)) => d.play(&*p),
            _ => false,
        };
        if played {
            tracing::info!("the spoken notice to the other party was queued again on request");
        } else {
            self.raise(
                CallErrorCode::CommandRejected,
                "say the notice again: it is not ready or the call is not translating".to_owned(),
            );
        }
    }

    /// Show a notice that could not be prepared, once.
    pub(super) fn check_disclosure(&mut self) {
        if self.disclosure_failure_shown {
            return;
        }
        if let Some(why) = self.disclosure.as_ref().and_then(|d| d.failure()) {
            self.disclosure_failure_shown = true;
            self.raise(CallErrorCode::DisclosureUnavailable, why);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use prxvc_media::SampleRate;

    use super::*;

    #[derive(Default)]
    struct Lane {
        log: Mutex<Vec<String>>,
        chunks: AtomicUsize,
    }

    impl Playout for Lane {
        fn push(&self, clause: ClauseId, _chunk: &PcmChunk) {
            self.chunks.fetch_add(1, Ordering::SeqCst);
            self.log.lock().push(format!("push {}", clause.index()));
        }
        fn end_clause(&self, clause: ClauseId) {
            self.log.lock().push(format!("end {}", clause.index()));
        }
        fn flush(&self) {
            self.log.lock().push("flush".into());
        }
        fn set_pace(&self, _speed: f32, _tight: bool) {}
    }

    fn chunk() -> PcmChunk {
        PcmChunk {
            samples: vec![0.1; 2400],
            rate: SampleRate::HZ_24000,
        }
    }

    fn at(ms: u64) -> MonoTime {
        MonoTime::from_millis(ms)
    }

    fn started(d: &Disclosure, n: u16) {
        d.report(
            PlaybackReport::Started {
                clause: clause_of(n),
                at: at(1),
            },
            at(1),
        );
    }

    fn finished(d: &Disclosure, n: u16, now: u64) -> bool {
        d.report(
            PlaybackReport::Finished {
                clause: clause_of(n),
                at: at(now),
            },
            at(now),
        )
    }

    #[test]
    fn it_plays_once_before_the_first_translated_audio_and_not_again() {
        let d = Disclosure::ready(vec![chunk(), chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.before_audio(&lane);
        assert_eq!(*lane.log.lock(), ["push 0", "push 0", "end 0"]);
        started(&d, 0);
        assert!(finished(&d, 0, 5_000));
        d.before_audio(&lane);
        assert_eq!(
            lane.chunks.load(Ordering::SeqCst),
            2,
            "the second clause does not repeat it"
        );
        let s = d.snapshot(Some(at(0)));
        assert_eq!(
            (s.state, s.plays, s.last_played_s),
            (DisclosureState::Played, 1, Some(5))
        );
    }

    #[test]
    fn it_plays_at_the_first_speech_start_and_not_again_before_the_first_translated_audio() {
        let d = Disclosure::ready(vec![chunk(), chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.on_speech_start(&lane);
        assert_eq!(*lane.log.lock(), ["push 0", "push 0", "end 0"]);
        d.on_speech_start(&lane);
        d.before_audio(&lane);
        assert_eq!(lane.chunks.load(Ordering::SeqCst), 2, "once per session");
        started(&d, 0);
        assert!(finished(&d, 0, 4_000));
        assert_eq!(d.snapshot(Some(at(0))).plays, 1);
    }

    #[test]
    fn a_notice_still_being_prepared_does_not_hold_up_the_speech_start() {
        let d = Disclosure::start_state(Prep::Preparing);
        let lane = Lane::default();
        let began = std::time::Instant::now();
        d.on_speech_start(&lane);
        assert!(began.elapsed() < Duration::from_secs(1), "no waiting");
        assert!(lane.log.lock().is_empty());
        assert!(
            d.shared.inner.lock().auto_pending,
            "it is still owed before the first audio"
        );
    }

    #[test]
    fn a_cut_notice_plays_again_at_the_next_speech_start() {
        let d = Disclosure::ready(vec![chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.on_speech_start(&lane);
        d.flushed();
        d.on_speech_start(&lane);
        assert_eq!(lane.chunks.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn pause_during_the_notice_makes_it_owed_again_and_a_pause_before_it_changes_nothing() {
        let d = Disclosure::ready(vec![chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.before_audio(&lane);
        started(&d, 0);
        // The direction is paused while it plays: the lane is no longer heard.
        d.set_audible(false);
        assert!(finished(&d, 0, 2_000));
        let s = d.snapshot(None);
        assert_eq!((s.plays, s.interrupted), (0, 1));
        // After the resume the next translated sentence is preceded by it again.
        d.set_audible(true);
        d.before_audio(&lane);
        assert_eq!(lane.log.lock().last().map(String::as_str), Some("end 1"));
        started(&d, 1);
        finished(&d, 1, 4_000);
        assert_eq!(d.snapshot(None).plays, 1);
    }

    #[test]
    fn a_flush_cuts_it_and_it_is_owed_again() {
        let d = Disclosure::ready(vec![chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.before_audio(&lane);
        d.flushed();
        assert_eq!(d.snapshot(None).interrupted, 1);
        d.before_audio(&lane);
        assert_eq!(lane.chunks.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn say_it_again_queues_it_now_and_is_not_re_armed_when_cut() {
        let d = Disclosure::ready(vec![chunk()]);
        d.set_audible(true);
        let lane = Lane::default();
        d.before_audio(&lane);
        started(&d, 0);
        finished(&d, 0, 3_000);
        assert!(d.play(&lane));
        assert_eq!(lane.log.lock().last().map(String::as_str), Some("end 1"));
        d.flushed();
        d.before_audio(&lane);
        assert_eq!(
            lane.chunks.load(Ordering::SeqCst),
            2,
            "a cut repeat is not automatic"
        );
    }

    #[test]
    fn a_notice_that_is_not_ready_cannot_be_repeated_and_a_failure_is_reported() {
        let d = Disclosure::start_state(Prep::Preparing);
        assert!(!d.play(&Lane::default()));
        assert_eq!(d.snapshot(None).state, DisclosureState::Preparing);
        d.shared.inner.lock().prep = Prep::Failed("no voice".into());
        assert_eq!(d.failure().as_deref(), Some("no voice"));
        assert_eq!(d.snapshot(None).state, DisclosureState::Failed);
    }

    #[test]
    fn reports_about_other_clauses_are_not_the_notices() {
        let d = Disclosure::ready(vec![chunk()]);
        let real = ClauseId::new(SegmentId::first(Direction::Outgoing), 0);
        assert!(!is_notice(real));
        assert!(!d.report(
            PlaybackReport::Started {
                clause: real,
                at: at(1)
            },
            at(1)
        ));
    }
}
