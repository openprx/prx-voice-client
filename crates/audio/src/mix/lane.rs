//! One input of the mixer: rate conversion, a prebuffer ("jitter buffer") and a gate that fades the input in and out so that starting and starving never click.
//!
//! A lane turns frames of any rate into a queue of samples at the output rate
//! and hands them out block by block.
//!
//! * **Rate conversion** uses the streaming mode of [`MonoResampler`]: filter
//!   history carries over frame boundaries and the filter delay is removed, so
//!   sample `n` of a stream comes out as output sample `n * out_rate / in_rate`
//!   whatever the input rate and chunking.
//! * **Prebuffer**: playout of a stream starts only when `depth` samples are
//!   queued (or the stream has ended), and restarts the same way after the
//!   queue ran dry. A chunk that arrives late but within the depth is absorbed.
//! * **Gate**: the gain of the lane moves toward 1 while more than `fade`
//!   samples are queued and toward 0 otherwise, by `1 / fade` per sample.
//!   Fading starts exactly `fade` samples before the queue runs out, so the
//!   last queued sample is played at gain 0: running dry ends in a fade, not in
//!   a step, and a stream starts with a fade-in. Streams shorter than `fade`
//!   (a few milliseconds) are therefore inaudible.
//! * **Timed lanes** (continuous captures such as the original voice) place
//!   frames by their timestamp: a gap between the expected and the actual
//!   start is filled with silence, an overlap is trimmed, so two timed lanes
//!   stay aligned even when frames are lost. An untimed lane (synthesised
//!   speech arriving in bursts) appends frames back to back.
//!
//! * **Gaps**: the queue running dry while the stream was not announced as ended is a gap in the
//!   speech, counted when it happens and measured when playout resumes ([`LaneStats::underruns`],
//!   [`LaneStats::gap_samples`], [`LaneStats::worst_gap_samples`]). A stream whose end arrives
//!   after the queue emptied was complete: that is a normal end, not a gap. A lane that is
//!   told the stream may run dry ([`LaneConfig::dry_fade`]) fades out over that longer span
//!   instead of the short end fade, so that an unavoidable gap is a fade and never a cut.
//! * **Adaptive pre-roll**: with a [`PrerollModel`] the number of samples queued before a stream
//!   starts follows the measured real-time factor of the producer (see `preroll.rs`).
//!
//! Pure processing: no clock, no device. Time is the number of samples handed out.

use std::collections::VecDeque;

use prxvc_media::{AudioFrame, MonoTime};

use super::preroll::{PrerollConfig, PrerollModel};
use super::shape::{Pace, ShapeConfig, leading_silence, trailing_silence};
use crate::AudioError;
use crate::resample::MonoResampler;
use crate::timescale::TimeScaler;

/// Largest difference between a frame's timestamp and the expected one that
/// is taken as timestamp jitter (no gap, no overlap).
const TIMING_TOLERANCE_NS: i128 = 2_000_000;
/// A gap longer than this is not filled with silence; the stream restarts.
const MAX_GAP_NS: i128 = 2_000_000_000;

/// How a lane is set up.
#[derive(Debug, Clone, Copy)]
pub struct LaneConfig {
    /// Output sample rate in Hz.
    pub out_rate: u32,
    /// Samples to queue before playout starts.
    pub depth: usize,
    /// Fade length of the gate, in samples.
    pub fade: usize,
    /// Fade-out length while the stream is not announced as ended (the queue may run dry before
    /// the speech is complete); at least `fade`.
    pub dry_fade: usize,
    /// Most samples the queue holds; the excess of a push is dropped.
    pub capacity: usize,
    /// Place frames by timestamp (see the module docs).
    pub timed: bool,
    /// Adaptive pre-roll; `None` keeps the fixed `depth`.
    pub preroll: Option<PrerollConfig>,
    /// Trim silence and catch up with a backlog; `None` plays what is queued as it is.
    pub shape: Option<ShapeConfig>,
}

/// What a lane did, for the session's health view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaneStats {
    /// Gaps: times the queue ran dry in the middle of a stream and playout resumed later (an
    /// announced end is not one).
    pub underruns: u64,
    /// Output samples of silence in those gaps, from running dry to resuming playout.
    pub gap_samples: u64,
    /// The longest single gap, in output samples.
    pub worst_gap_samples: u64,
    /// The pre-roll the lane asks for at the start of a stream now, in output samples.
    pub start_depth_samples: usize,
    /// Output samples dropped because the queue was full.
    pub dropped_samples: u64,
    /// Output samples of silence cut from the ends of clauses.
    pub trimmed_silence_samples: u64,
    /// Output samples the speed-up saved (input samples taken in less output).
    pub saved_samples: u64,
    /// Output samples of silence inserted for gaps between timed frames.
    pub filled_samples: u64,
    /// Input samples dropped because a timed frame overlapped the previous one.
    pub trimmed_samples: u64,
    /// Output samples queued now.
    pub queued_samples: usize,
    /// Output samples accepted into the queue since the lane was made (a flush
    /// counts what it threw away as never accepted).
    pub enqueued_total: u64,
    /// Output samples handed out for playing since the lane was made. A sample
    /// is "played" when the mixer takes it from the queue; the sink adds its own delay.
    pub played_total: u64,
}

/// One input of the mixer. See the module docs.
pub struct Lane {
    cfg: LaneConfig,
    /// The stream's resampler and its source rate.
    resampler: Option<(u32, MonoResampler)>,
    queue: VecDeque<f32>,
    playing: bool,
    ended: bool,
    gate: f32,
    /// Timestamp the next input sample should have (timed lanes).
    next_ts: Option<MonoTime>,
    stats: LaneStats,
    /// Samples handed out so far: the lane's clock.
    clock: u64,
    /// The queue ran dry mid-stream and playout has not resumed: samples of silence so far.
    gap: Option<u64>,
    /// Samples a stream must have queued before playout starts or resumes.
    start_depth: usize,
    model: Option<PrerollModel>,
    stream: StreamClock,
    pace: Pace,
    scaler: Option<TimeScaler>,
    /// No audible sample of the stream has been queued yet: its leading silence is still cut.
    lead_pending: bool,
    /// Samples of the lead-in silence kept so far in this stream.
    lead_kept: usize,
}

/// What the lane saw of the stream being received (for the pre-roll model).
#[derive(Clone, Copy, Default)]
struct StreamClock {
    first_push: Option<u64>,
    last_push: u64,
    first_len: u64,
    total: u64,
    pushes: u32,
    played_at_start: u64,
    /// The longest wait between two pushes of the stream, in lane clock samples.
    longest_wait: u64,
}

impl Lane {
    /// A lane with an empty queue.
    pub fn new(mut cfg: LaneConfig) -> Self {
        cfg.fade = cfg.fade.max(1);
        // Playout must start with more queued than the fade needs.
        cfg.depth = cfg.depth.max(cfg.fade + 1);
        cfg.dry_fade = cfg.dry_fade.max(cfg.fade);
        let model = cfg.preroll.map(|p| PrerollModel::new(p, cfg.depth));
        Self {
            start_depth: cfg.depth,
            model,
            cfg,
            resampler: None,
            queue: VecDeque::new(),
            playing: false,
            ended: false,
            gate: 0.0,
            next_ts: None,
            stats: LaneStats::default(),
            clock: 0,
            gap: None,
            stream: StreamClock::default(),
            pace: Pace::default(),
            scaler: None,
            lead_pending: true,
            lead_kept: 0,
        }
    }

    /// What the pipeline asks for the clauses queued from now on (speed and short pauses).
    pub fn set_pace(&mut self, pace: Pace) {
        let max = self.cfg.shape.map_or(1.0, |s| s.max_speed.max(1.0));
        self.pace = Pace {
            speed: pace.speed.clamp(1.0, max),
            tight: pace.tight,
        };
    }

    /// Cut the silence at the start of a stream and speed it up when asked. Only an untimed lane
    /// with a [`ShapeConfig`] changes anything.
    fn shape(&mut self, mut samples: Vec<f32>) -> Vec<f32> {
        let Some(cfg) = self.cfg.shape else {
            return samples;
        };
        if self.lead_pending && !samples.is_empty() {
            let keep = cfg.lead(self.pace);
            let silent = leading_silence(&samples, cfg.threshold);
            if silent == samples.len() {
                // Silence only: what is kept of the lead-in is spread over the blocks it spans.
                let room = keep.saturating_sub(self.lead_kept).min(samples.len());
                self.lead_kept += room;
                self.stats.trimmed_silence_samples += (samples.len() - room) as u64;
                samples.truncate(room);
                if samples.is_empty() {
                    return samples;
                }
                // Speech has not started: only the silence just kept goes on (below).
            } else {
                let cut = silent.saturating_sub(keep.saturating_sub(self.lead_kept));
                self.stats.trimmed_silence_samples += cut as u64;
                samples.drain(..cut);
                self.lead_pending = false;
            }
        }
        let scaler = self
            .scaler
            .get_or_insert_with(|| TimeScaler::new(self.cfg.out_rate));
        if self.pace.speed <= 1.0 && scaler.is_empty() {
            return samples;
        }
        let before = samples.len();
        let mut out = Vec::with_capacity(before);
        scaler.process(&samples, self.pace.speed, &mut out);
        self.stats.saved_samples += (before as u64).saturating_sub(out.len() as u64);
        out
    }

    /// End of a stream's audio: what the speed-up holds back is queued.
    fn flush_shaper(&mut self) {
        if let Some(scaler) = self.scaler.as_mut() {
            let mut out = Vec::new();
            scaler.finish(self.pace.speed, &mut out);
            self.enqueue(&out);
        }
        self.lead_pending = true;
        self.lead_kept = 0;
    }

    /// Cut the silence at the end of the stream that was just completed down to what is kept.
    fn trim_tail(&mut self) {
        let Some(cfg) = self.cfg.shape else {
            return;
        };
        let keep = cfg.trail(self.pace);
        let (a, b) = self.queue.as_slices();
        let mut silent = trailing_silence(b, cfg.threshold);
        if silent == b.len() {
            silent += trailing_silence(a, cfg.threshold);
        }
        let cut = silent.saturating_sub(keep).min(self.queue.len());
        if cut > 0 {
            self.queue.truncate(self.queue.len() - cut);
            self.stats.enqueued_total = self.stats.enqueued_total.saturating_sub(cut as u64);
            self.stats.trimmed_silence_samples += cut as u64;
        }
    }

    /// Queue a frame (converted to the output rate).
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when the resampler cannot be built
    /// or run.
    pub fn push(&mut self, frame: &AudioFrame) -> Result<(), AudioError> {
        let rate = frame.rate().hz();
        self.ended = false;
        if self.resampler.as_ref().is_some_and(|(r, _)| *r != rate) {
            self.finish_stream()?;
        }
        let mut samples = frame.samples();
        let mut silence = 0usize;
        if self.cfg.timed {
            match self.place(frame) {
                Placement::Append => {}
                Placement::Fill(n) => silence = n,
                Placement::Skip(n) => {
                    self.stats.trimmed_samples += n.min(samples.len()) as u64;
                    if n >= samples.len() {
                        return Ok(());
                    }
                    samples = &samples[n..];
                }
                Placement::Restart => self.finish_stream()?,
            }
            self.next_ts = Some(frame.end_ts());
        }
        if self.resampler.is_none() {
            self.resampler = Some((rate, MonoResampler::new(rate, self.cfg.out_rate)?));
        }
        let Some((_, resampler)) = self.resampler.as_mut() else {
            return Ok(());
        };
        let mut converted = Vec::new();
        if silence > 0 {
            self.stats.filled_samples +=
                silence as u64 * u64::from(self.cfg.out_rate) / u64::from(rate);
            converted.extend(resampler.stream_write(&vec![0.0; silence])?);
        }
        converted.extend(resampler.stream_write(samples)?);
        let converted = self.shape(converted);
        self.note_push(converted.len());
        self.enqueue(&converted);
        Ok(())
    }

    /// Bookkeeping of the stream's arrival, and the pre-roll of a stream that starts now.
    fn note_push(&mut self, len: usize) {
        let st = &mut self.stream;
        if st.first_push.is_none() {
            st.first_push = Some(self.clock);
            st.first_len = len as u64;
            st.played_at_start = self.stats.played_total;
            if let Some(m) = &self.model {
                self.start_depth = m.depth_for_new_stream();
            }
        }
        if st.pushes > 0 {
            st.longest_wait = st.longest_wait.max(self.clock - st.last_push);
        }
        st.last_push = self.clock;
        st.total += len as u64;
        st.pushes += 1;
    }

    /// The stream is complete: what it showed about the producer goes into the model.
    fn learn(&mut self) {
        let st = std::mem::take(&mut self.stream);
        let (Some(first), Some(model)) = (st.first_push, self.model.as_mut()) else {
            return;
        };
        model.observe(
            st.total,
            st.total.saturating_sub(st.first_len),
            st.last_push.saturating_sub(first),
            st.pushes,
            st.longest_wait,
        );
        self.start_depth = model.depth_for_new_stream();
    }

    /// Where `frame` goes relative to the previous one (timed lanes).
    fn place(&self, frame: &AudioFrame) -> Placement {
        let Some(expected) = self.next_ts else {
            return Placement::Append;
        };
        let gap = i128::from(frame.ts().as_nanos()) - i128::from(expected.as_nanos());
        let rate = i128::from(frame.rate().hz());
        let samples_of =
            |ns: i128| usize::try_from(ns.abs() * rate / 1_000_000_000).unwrap_or(usize::MAX);
        if gap > MAX_GAP_NS {
            Placement::Restart
        } else if gap > TIMING_TOLERANCE_NS {
            Placement::Fill(samples_of(gap))
        } else if gap < -TIMING_TOLERANCE_NS {
            Placement::Skip(samples_of(gap))
        } else {
            Placement::Append
        }
    }

    /// The current stream has no more audio for now: its resampler tail is
    /// queued, and running dry afterwards is not an underrun.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when the resampler fails.
    pub fn end_stream(&mut self) -> Result<(), AudioError> {
        self.finish_stream()?;
        self.trim_tail();
        if self.queue.is_empty() {
            // Everything of the stream was played before its end was known: it was complete,
            // so the queue running dry was its normal end and not a gap.
            if self.gap.take().is_some() {
                self.stats.underruns = self.stats.underruns.saturating_sub(1);
            }
        }
        self.ended = true;
        self.next_ts = None;
        self.learn();
        Ok(())
    }

    fn finish_stream(&mut self) -> Result<(), AudioError> {
        if let Some((_, mut resampler)) = self.resampler.take() {
            let tail = resampler.stream_finish()?;
            let tail = self.shape(tail);
            self.enqueue(&tail);
        }
        self.flush_shaper();
        Ok(())
    }

    /// Throw away what is queued except the last `fade` samples, which play
    /// out under the fade so that the cut is not a step.
    pub fn flush(&mut self) {
        self.resampler = None;
        self.queue.truncate(self.cfg.fade);
        self.stats.enqueued_total = self.stats.played_total + self.queue.len() as u64;
        self.ended = true;
        self.next_ts = None;
        self.gap = None;
        self.stream = StreamClock::default();
        if let Some(scaler) = self.scaler.as_mut() {
            scaler.reset();
        }
        self.lead_pending = true;
        self.lead_kept = 0;
    }

    fn enqueue(&mut self, samples: &[f32]) {
        let room = self.cfg.capacity.saturating_sub(self.queue.len());
        let take = samples.len().min(room);
        self.queue.extend(&samples[..take]);
        self.stats.enqueued_total += take as u64;
        self.stats.dropped_samples += (samples.len() - take) as u64;
    }

    /// Fill `out` with the next samples of the lane (silence while it is not
    /// playing).
    pub fn next_block(&mut self, out: &mut [f32]) {
        for slot in out.iter_mut() {
            self.clock += 1;
            if !self.playing && self.may_start() {
                self.playing = true;
                if let Some(dry) = self.gap.take() {
                    self.stats.gap_samples += dry;
                    self.stats.worst_gap_samples = self.stats.worst_gap_samples.max(dry);
                }
            }
            if self.playing && self.queue.is_empty() {
                self.playing = false;
                if !self.ended {
                    self.stats.underruns += 1;
                    self.gap = Some(0);
                    self.start_depth = self.resume_depth();
                }
                self.ended = false;
            }
            if !self.playing
                && let Some(dry) = self.gap.as_mut()
            {
                *dry += 1;
            }
            // Fade over the short end fade when the stream is complete, over the longer one
            // while more of it is still expected.
            let fade = if self.ended {
                self.cfg.fade
            } else {
                self.cfg.dry_fade
            };
            let target = if self.playing && self.queue.len() > fade {
                1.0
            } else {
                0.0
            };
            // Fading in is always quick; fading out takes `fade`.
            let (up, down) = (1.0 / self.cfg.fade as f32, 1.0 / fade as f32);
            self.gate += (target - self.gate).clamp(-down, up);
            let sample = if self.playing {
                self.stats.played_total += 1;
                self.queue.pop_front().unwrap_or(0.0)
            } else {
                0.0
            };
            *slot = sample * self.gate;
        }
    }

    fn may_start(&self) -> bool {
        self.queue.len() >= self.start_depth || (self.ended && !self.queue.is_empty())
    }

    /// The pre-roll for resuming after a gap: what the rest of the stream still needs.
    fn resume_depth(&self) -> usize {
        let Some(m) = &self.model else {
            return self.cfg.depth;
        };
        let played = self
            .stats
            .played_total
            .saturating_sub(self.stream.played_at_start);
        m.depth_to_resume(played)
    }

    /// Counters and the queue length.
    pub fn stats(&self) -> LaneStats {
        LaneStats {
            queued_samples: self.queue.len(),
            start_depth_samples: self.start_depth,
            ..self.stats
        }
    }
}

/// What to do with a timed frame.
enum Placement {
    /// Contiguous with the previous frame (or the first).
    Append,
    /// Insert this many source-rate samples of silence first.
    Fill(usize),
    /// Drop this many leading source-rate samples (overlap).
    Skip(usize),
    /// The gap is too long to fill: start a new stream.
    Restart,
}
