//! The link between a running pump and a translation pipeline: a tap that hands the pump's frames on, a feed that takes translated speech in, and the tracker that reports when that speech actually plays.
//!
//! The pump stays ignorant of the pipeline: it knows a closure that takes frames
//! at a given rate ([`FrameTap`]), a thread-safe queue of translated audio
//! ([`TranslatedFeed`]) and a closure that is told about playback
//! ([`PlaybackSignal`]). The composition root binds them to a pipeline.
//!
//! # Playback signals
//!
//! The translated lane of the mixer counts the samples it accepted and the
//! samples it handed out (`LaneStats::enqueued_total`, `played_total`). For each
//! clause the tracker remembers where its audio starts and ends on that count:
//!
//! * `Started` is sent when the lane hands out the first sample of the clause;
//! * `Finished` is sent when the lane has handed out the last sample of a clause
//!   whose end was announced, which is when the lane has drained.
//!
//! Both carry the time the sample reaches the speaker: the time the mixer handed
//! it out plus what the sink still holds (`sink.queued()`). Nothing is derived
//! from the length of the audio. A flush forgets every clause not finished: the
//! pipeline cancelled them and expects no report.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{AudioFrame, ClauseId, Clock, MonoTime, PcmChunk, SampleRate};

use crate::mix::{Mixer, MixerStats};
use crate::resample::MonoResampler;

/// Most translated audio the feed holds before it drops the excess, in samples
/// (about 80 s at 24 kHz). The pipeline holds at most 20 s, so this is a guard only.
const FEED_CAP_SAMPLES: u64 = 2_000_000;

/// A report about the playback of one clause.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaybackSignal {
    /// The first sample of the clause reaches the speaker at `at`.
    Started {
        /// The clause.
        clause: ClauseId,
        /// When.
        at: MonoTime,
    },
    /// The last sample of the clause reaches the speaker at `at`.
    Finished {
        /// The clause.
        clause: ClauseId,
        /// When.
        at: MonoTime,
    },
}

/// Receives playback reports on the pump thread.
pub type PlaybackFn = Box<dyn FnMut(PlaybackSignal) + Send>;

/// Receives the pump's frames, resampled to `rate`, on the pump thread.
pub struct FrameTap {
    /// The rate the receiver wants (the VAD's rate).
    pub rate: SampleRate,
    /// The receiver. It must not block.
    pub sink: Box<dyn FnMut(AudioFrame) + Send>,
}

/// Everything [`super::Pump::connect`] takes.
pub struct PumpLink {
    /// Where the pump's frames go.
    pub tap: FrameTap,
    /// Where playback reports go.
    pub on_playback: PlaybackFn,
    /// The clock the reports and the restamped frames are on (the pipeline's).
    pub clock: Arc<dyn Clock>,
}

enum FeedItem {
    Audio(ClauseId, AudioFrame),
    End(ClauseId),
    Flush,
}

#[derive(Default)]
struct FeedShared {
    items: Mutex<Vec<FeedItem>>,
    queued_samples: AtomicU64,
    dropped_samples: AtomicU64,
}

/// The way translated speech enters a pump. Clones feed the same queue; every
/// method is cheap and never blocks on the pump thread.
#[derive(Clone, Default)]
pub struct TranslatedFeed(Arc<FeedShared>);

impl TranslatedFeed {
    /// Queue a chunk of `clause`. A chunk that does not fit is dropped and counted.
    pub fn push(&self, clause: ClauseId, chunk: &PcmChunk) {
        let n = chunk.samples.len() as u64;
        let Ok(frame) = AudioFrame::new(chunk.samples.clone(), chunk.rate, MonoTime::ZERO, 0)
        else {
            self.0.dropped_samples.fetch_add(n, Ordering::Relaxed);
            return;
        };
        if self.0.queued_samples.load(Ordering::Relaxed) + n > FEED_CAP_SAMPLES {
            self.0.dropped_samples.fetch_add(n, Ordering::Relaxed);
            return;
        }
        self.0.queued_samples.fetch_add(n, Ordering::Relaxed);
        self.0.items.lock().push(FeedItem::Audio(clause, frame));
    }

    /// The audio of `clause` is complete.
    pub fn end_clause(&self, clause: ClauseId) {
        self.0.items.lock().push(FeedItem::End(clause));
    }

    /// Throw away what has not reached the mixer and what the mixer holds.
    pub fn flush(&self) {
        let mut items = self.0.items.lock();
        items.clear();
        self.0.queued_samples.store(0, Ordering::Relaxed);
        items.push(FeedItem::Flush);
    }

    /// Samples dropped because the feed was full or a chunk was invalid.
    pub fn dropped_samples(&self) -> u64 {
        self.0.dropped_samples.load(Ordering::Relaxed)
    }

    fn take(&self) -> Vec<FeedItem> {
        let mut items = self.0.items.lock();
        let taken = std::mem::take(&mut *items);
        self.0.queued_samples.store(0, Ordering::Relaxed);
        taken
    }
}

struct Marker {
    clause: ClauseId,
    first: u64,
    end: Option<u64>,
    started: bool,
}

/// The pump's half of a link: the receivers and the bookkeeping.
pub(super) struct LinkState {
    tap: FrameTap,
    resampler: Option<(u32, MonoResampler)>,
    seq: u64,
    next_ts: MonoTime,
    feed: TranslatedFeed,
    on_playback: PlaybackFn,
    clock: Arc<dyn Clock>,
    markers: VecDeque<Marker>,
}

impl LinkState {
    pub(super) fn new(link: PumpLink, feed: TranslatedFeed) -> Self {
        Self {
            tap: link.tap,
            resampler: None,
            seq: 0,
            next_ts: MonoTime::ZERO,
            feed,
            on_playback: link.on_playback,
            clock: link.clock,
            markers: VecDeque::new(),
        }
    }

    /// Hand a frame of the pump's source to the tap, converted and restamped on the pipeline clock.
    pub(super) fn tap_frame(&mut self, frame: &AudioFrame) {
        let samples = if frame.rate() == self.tap.rate {
            frame.samples().to_vec()
        } else {
            match self.convert(frame) {
                Some(s) if !s.is_empty() => s,
                _ => return,
            }
        };
        let duration = self.tap.rate.duration_of(samples.len());
        let now = self.clock.now();
        let back = u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX);
        let ts = MonoTime::from_nanos(now.as_nanos().saturating_sub(back)).max(self.next_ts);
        self.next_ts = ts.saturating_add(duration);
        if let Ok(out) = AudioFrame::new(samples, self.tap.rate, ts, self.seq) {
            self.seq += 1;
            (self.tap.sink)(out.with_discontinuity(frame.discontinuity()));
        }
    }

    fn convert(&mut self, frame: &AudioFrame) -> Option<Vec<f32>> {
        let rate = frame.rate().hz();
        if self.resampler.as_ref().is_none_or(|(r, _)| *r != rate) {
            match MonoResampler::new(rate, self.tap.rate.hz()) {
                Ok(r) => self.resampler = Some((rate, r)),
                Err(e) => {
                    tracing::warn!(error = %e, "the pipeline tap cannot resample");
                    return None;
                }
            }
        }
        let (_, resampler) = self.resampler.as_mut()?;
        match resampler.stream_write(frame.samples()) {
            Ok(out) => Some(out),
            Err(e) => {
                tracing::warn!(error = %e, "the pipeline tap failed to resample a frame");
                None
            }
        }
    }

    /// Move queued translated audio into the mixer and keep the clause markers.
    pub(super) fn apply_feed(&mut self, mixer: &mut Mixer) {
        for item in self.feed.take() {
            match item {
                FeedItem::Audio(clause, frame) => {
                    let before = mixer.stats().translated.enqueued_total;
                    if self
                        .markers
                        .back()
                        .is_none_or(|m| m.clause != clause || m.end.is_some())
                    {
                        self.markers.push_back(Marker {
                            clause,
                            first: before,
                            end: None,
                            started: false,
                        });
                    }
                    if let Err(e) = mixer.push_translated(&frame) {
                        tracing::warn!(error = %e, "the mixer refused translated audio");
                    }
                }
                FeedItem::End(clause) => {
                    if let Err(e) = mixer.end_translated() {
                        tracing::warn!(error = %e, "the mixer could not end the clause");
                    }
                    let total = mixer.stats().translated.enqueued_total;
                    match self.markers.iter_mut().rev().find(|m| m.clause == clause) {
                        Some(m) => m.end = Some(total),
                        None => self.markers.push_back(Marker {
                            clause,
                            first: total,
                            end: Some(total),
                            started: false,
                        }),
                    }
                }
                FeedItem::Flush => {
                    mixer.flush_translated();
                    self.markers.clear();
                }
            }
        }
    }

    /// Report the clauses whose audio started or finished playing. `sink_queued` is what the
    /// sink holds after the mixer's last write.
    pub(super) fn poll_playback(&mut self, stats: &MixerStats, sink_queued: Duration) {
        let played = stats.translated.played_total;
        let at = self.clock.now().saturating_add(sink_queued);
        while let Some(m) = self.markers.front_mut() {
            let began = played > m.first || m.end.is_some_and(|e| e == m.first);
            if began && !m.started {
                m.started = true;
                (self.on_playback)(PlaybackSignal::Started {
                    clause: m.clause,
                    at,
                });
            }
            match m.end {
                Some(end) if m.started && played >= end => {
                    (self.on_playback)(PlaybackSignal::Finished {
                        clause: m.clause,
                        at,
                    });
                    self.markers.pop_front();
                }
                _ => break,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use parking_lot::Mutex;
    use prxvc_media::{Direction, ManualClock, SegmentId};

    use super::*;
    use crate::mix::{MixMode, MixerConfig};

    fn clause(i: u16) -> ClauseId {
        ClauseId::new(SegmentId::first(Direction::Outgoing), i)
    }

    fn chunk(ms: u64) -> PcmChunk {
        let rate = SampleRate::HZ_48000;
        PcmChunk {
            samples: vec![0.3; rate.samples_in(Duration::from_millis(ms))],
            rate,
        }
    }

    struct Rig {
        link: LinkState,
        feed: TranslatedFeed,
        mixer: Mixer,
        clock: ManualClock,
        signals: Arc<Mutex<Vec<PlaybackSignal>>>,
        frames: Arc<Mutex<Vec<AudioFrame>>>,
    }

    fn rig() -> Rig {
        let clock = ManualClock::new();
        let signals: Arc<Mutex<Vec<PlaybackSignal>>> = Arc::default();
        let frames: Arc<Mutex<Vec<AudioFrame>>> = Arc::default();
        let feed = TranslatedFeed::default();
        let link = LinkState::new(
            PumpLink {
                tap: FrameTap {
                    rate: SampleRate::HZ_16000,
                    sink: {
                        let frames = Arc::clone(&frames);
                        Box::new(move |f| frames.lock().push(f))
                    },
                },
                on_playback: {
                    let signals = Arc::clone(&signals);
                    Box::new(move |s| signals.lock().push(s))
                },
                clock: Arc::new(clock.clone()),
            },
            feed.clone(),
        );
        let mixer = Mixer::new(MixerConfig {
            mode: MixMode::Translation,
            jitter_buffer: Duration::from_millis(40),
            ..MixerConfig::default()
        });
        Rig {
            link,
            feed,
            mixer,
            clock,
            signals,
            frames,
        }
    }

    impl Rig {
        /// Render `ms` of output and run the tracker after it.
        fn play(&mut self, ms: u64) {
            let mut out = vec![0.0; SampleRate::HZ_48000.samples_in(Duration::from_millis(ms))];
            self.link.apply_feed(&mut self.mixer);
            self.mixer.render(&mut out);
            self.clock.advance(Duration::from_millis(ms));
            self.link
                .poll_playback(&self.mixer.stats(), Duration::from_millis(40));
        }
    }

    #[test]
    fn a_clause_starts_when_the_lane_hands_out_its_audio_and_finishes_when_it_drained() {
        let mut r = rig();
        r.feed.push(clause(0), &chunk(100));
        r.feed.end_clause(clause(0));
        r.play(10);
        // The jitter buffer (40 ms) holds the first 10 ms back until the queue is deep enough:
        // the lane is playing already because the end was announced.
        assert!(matches!(
            r.signals.lock().first(),
            Some(PlaybackSignal::Started { .. })
        ));
        assert_eq!(r.signals.lock().len(), 1, "not finished after 10 of 100 ms");
        r.play(80);
        assert_eq!(r.signals.lock().len(), 1);
        r.play(30);
        let signals = r.signals.lock().clone();
        assert_eq!(signals.len(), 2);
        let PlaybackSignal::Finished { clause: c, at } = signals[1] else {
            panic!("expected Finished, got {:?}", signals[1]);
        };
        assert_eq!(c, clause(0));
        // The time carries the sink's queue: the clock is at 120 ms, the sink holds 40 ms.
        assert_eq!(at, MonoTime::from_millis(160));
    }

    #[test]
    fn two_clauses_report_in_order_and_each_separately() {
        let mut r = rig();
        r.feed.push(clause(0), &chunk(50));
        r.feed.end_clause(clause(0));
        r.feed.push(clause(1), &chunk(50));
        r.feed.end_clause(clause(1));
        r.play(200);
        let kinds: Vec<_> = r
            .signals
            .lock()
            .iter()
            .map(|s| match s {
                PlaybackSignal::Started { clause, .. } => ('S', clause.index()),
                PlaybackSignal::Finished { clause, .. } => ('F', clause.index()),
            })
            .collect();
        assert_eq!(kinds, [('S', 0), ('F', 0), ('S', 1), ('F', 1)]);
    }

    #[test]
    fn a_flush_forgets_the_clauses_and_reports_nothing_more() {
        let mut r = rig();
        r.feed.push(clause(0), &chunk(500));
        r.play(20);
        assert_eq!(r.signals.lock().len(), 1);
        r.feed.flush();
        r.play(600);
        assert_eq!(
            r.signals.lock().len(),
            1,
            "the cut clause is never reported finished"
        );
        assert!(r.link.markers.is_empty());
        assert_eq!(r.mixer.stats().translated.queued_samples, 0);
    }

    #[test]
    fn a_clause_that_is_not_ended_does_not_finish_even_when_the_lane_runs_dry() {
        let mut r = rig();
        r.feed.push(clause(0), &chunk(100));
        r.play(300);
        let signals = r.signals.lock().clone();
        assert_eq!(signals.len(), 1);
        assert!(matches!(signals[0], PlaybackSignal::Started { .. }));
    }

    #[test]
    fn the_feed_drops_and_counts_what_does_not_fit() {
        let feed = TranslatedFeed::default();
        let big = PcmChunk {
            samples: vec![0.1; 1_500_000],
            rate: SampleRate::HZ_24000,
        };
        feed.push(clause(0), &big);
        feed.push(clause(0), &big);
        assert_eq!(feed.dropped_samples(), 1_500_000);
    }

    #[test]
    fn tapped_frames_are_resampled_and_restamped_on_the_pipeline_clock() {
        let mut r = rig();
        r.clock.advance(Duration::from_secs(5));
        let samples = vec![0.2f32; 960]; // 20 ms at 48 kHz
        let frame = AudioFrame::new(
            samples,
            SampleRate::HZ_48000,
            MonoTime::from_millis(900_000),
            7,
        )
        .expect("frame")
        .with_discontinuity(true);
        for _ in 0..20 {
            r.link.tap_frame(&frame);
        }
        let frames = r.frames.lock();
        assert!(!frames.is_empty());
        assert!(frames.iter().all(|f| f.rate() == SampleRate::HZ_16000));
        assert!(
            frames[0].ts() <= MonoTime::from_millis(5_000),
            "restamped, not 900 s"
        );
        assert!(frames[0].discontinuity());
        let total: usize = frames.iter().map(AudioFrame::len).sum();
        assert!(
            (total as i64 - 20 * 320).abs() < 1_500,
            "about 400 ms of 16 kHz, got {total}"
        );
        assert!(frames.windows(2).all(|w| w[1].ts() >= w[0].end_ts()));
    }
}
