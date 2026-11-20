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
//! Pure processing: no clock, no device.

use std::collections::VecDeque;

use prxvc_media::{AudioFrame, MonoTime};

use crate::AudioError;
use crate::resample::MonoResampler;

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
    /// Most samples the queue holds; the excess of a push is dropped.
    pub capacity: usize,
    /// Place frames by timestamp (see the module docs).
    pub timed: bool,
}

/// What a lane did, for the session's health view.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LaneStats {
    /// Times the queue ran dry in the middle of a stream (an announced end
    /// is not one).
    pub underruns: u64,
    /// Output samples dropped because the queue was full.
    pub dropped_samples: u64,
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
}

impl Lane {
    /// A lane with an empty queue.
    pub fn new(mut cfg: LaneConfig) -> Self {
        cfg.fade = cfg.fade.max(1);
        // Playout must start with more queued than the fade needs.
        cfg.depth = cfg.depth.max(cfg.fade + 1);
        Self {
            cfg,
            resampler: None,
            queue: VecDeque::new(),
            playing: false,
            ended: false,
            gate: 0.0,
            next_ts: None,
            stats: LaneStats::default(),
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
        self.enqueue(&converted);
        Ok(())
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
        self.ended = true;
        self.next_ts = None;
        Ok(())
    }

    fn finish_stream(&mut self) -> Result<(), AudioError> {
        if let Some((_, mut resampler)) = self.resampler.take() {
            let tail = resampler.stream_finish()?;
            self.enqueue(&tail);
        }
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
        let step = 1.0 / self.cfg.fade as f32;
        for slot in out.iter_mut() {
            if !self.playing
                && (self.queue.len() >= self.cfg.depth || (self.ended && !self.queue.is_empty()))
            {
                self.playing = true;
            }
            if self.playing && self.queue.is_empty() {
                self.playing = false;
                if !self.ended {
                    self.stats.underruns += 1;
                }
                self.ended = false;
            }
            let target = if self.playing && self.queue.len() > self.cfg.fade {
                1.0
            } else {
                0.0
            };
            self.gate += (target - self.gate).clamp(-step, step);
            let sample = if self.playing {
                self.stats.played_total += 1;
                self.queue.pop_front().unwrap_or(0.0)
            } else {
                0.0
            };
            *slot = sample * self.gate;
        }
    }

    /// Counters and the queue length.
    pub fn stats(&self) -> LaneStats {
        LaneStats {
            queued_samples: self.queue.len(),
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
