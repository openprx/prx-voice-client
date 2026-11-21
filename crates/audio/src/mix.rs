//! The mixer of one direction: turns the translated speech and the original (passthrough) voice into the single stream a listener hears.
//!
//! One mixer serves one direction. Its two inputs are the translated speech
//! stream (bursty, produced by synthesis) and the original stream (continuous,
//! from a capture). Its output is a mono stream at a fixed rate that a sink
//! endpoint plays: the virtual microphone for what the remote party hears, the
//! headphones for what the local user hears.
//!
//! # Modes
//!
//! | [`MixMode`] | Translated input | Original input |
//! |---|---|---|
//! | `Translation` | `translated_gain_db` | silent |
//! | `Both` | `translated_gain_db` | `original_gain_db` (default -12 dB: lowered, translation on top) |
//! | `Original` | silent | `passthrough_gain_db` (default 0 dB) |
//!
//! The mixer only **executes** a mode: [`Mixer::set_mode`] is commanded from
//! outside (call state, user, failure policy). A switch moves the two gains to
//! their new values along a raised cosine over `crossfade` (default 20 ms), so
//! it is never a click and, for correlated inputs, never a dip to silence. A
//! switch commanded during a crossfade starts from the gains reached so far.
//!
//! # Inputs
//!
//! Each input is a [`lane::Lane`]: frames of any rate are converted to the
//! output rate with the crate's streaming resampler, queued, and played out
//! after a prebuffer. The translated lane's prebuffer is the jitter buffer
//! (`jitter_buffer_ms`): a chunk that arrives late but within that depth
//! leaves no gap. The depth adapts to the measured real-time factor of synthesis
//! ([`PrerollModel`]): a clause never starts with less queued than it needs to
//! finish without a gap at the current rate. When the queue does run dry in the middle of a
//! clause the lane fades out and waits for the prebuffer to refill, which is
//! counted as an underrun ([`LaneStats::underruns`]) with the length of the silence
//! ([`LaneStats::gap_samples`]); an announced end of
//! stream ([`Mixer::end_translated`]) is not an underrun, even when it arrives after the
//! queue emptied. The original lane
//! places frames by timestamp, so lost frames become silence of the right
//! length and the two inputs stay aligned.
//!
//! # Threading and time
//!
//! No device and no clock: the mixer is driven by calls. A non-real-time
//! thread pushes frames as they arrive and calls [`Mixer::feed`] (or
//! [`Mixer::render`]) on its own cadence; time comes with the frames'
//! timestamps and from the sample count of the output.
//!
//! The sum of the two inputs is clamped to [-1, 1].

mod lane;
mod preroll;
mod ramp;

use std::time::Duration;

use prxvc_media::{AudioFrame, MonoTime, SampleRate};
use prxvc_ports::AudioSink;

use crate::AudioError;
use crate::call_pump::{Level, LevelMeter};
pub use lane::LaneStats;
use lane::{Lane, LaneConfig};
pub use preroll::{PrerollConfig, PrerollModel};
use ramp::GainRamp;
pub use ramp::db_to_linear;

/// Which inputs a listener hears.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MixMode {
    /// The translated speech only.
    Translation,
    /// The original, lowered, with the translation on top.
    Both,
    /// The original only.
    Original,
}

/// Settings of a mixer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MixerConfig {
    /// Output sample rate.
    pub output_rate: SampleRate,
    /// Mode at the start.
    pub mode: MixMode,
    /// Gain of the translated input in every mode that plays it.
    pub translated_gain_db: f32,
    /// Gain of the original in `Both` (`call.original_gain_db`).
    pub original_gain_db: f32,
    /// Gain of the original in `Original`.
    pub passthrough_gain_db: f32,
    /// Jitter buffer depth of the translated input
    /// (`pipeline.jitter_buffer_ms`).
    pub jitter_buffer: Duration,
    /// Prebuffer of the original input.
    pub original_buffer: Duration,
    /// Length of a mode switch.
    pub crossfade: Duration,
    /// Length of the fade a lane applies when it starts or when a stream ends.
    pub fade: Duration,
    /// Length of the fade-out of the translated lane while its stream is not announced as ended
    /// (it may run dry before the clause is complete): an unavoidable gap is a fade, not a cut.
    pub dry_fade: Duration,
    /// Adapt the translated lane's pre-roll to the measured real-time factor of synthesis
    /// (`pipeline.adaptive_preroll`).
    pub adaptive_preroll: bool,
    /// The most pre-roll the adaptation asks for (`pipeline.preroll_max_ms`).
    pub preroll_max: Duration,
    /// Most audio either input holds before it drops the excess.
    pub max_buffer: Duration,
}

impl Default for MixerConfig {
    fn default() -> Self {
        Self {
            output_rate: SampleRate::HZ_48000,
            mode: MixMode::Translation,
            translated_gain_db: 0.0,
            original_gain_db: -12.0,
            passthrough_gain_db: 0.0,
            jitter_buffer: Duration::from_millis(150),
            original_buffer: Duration::from_millis(20),
            crossfade: Duration::from_millis(20),
            fade: Duration::from_millis(5),
            dry_fade: Duration::from_millis(20),
            adaptive_preroll: true,
            preroll_max: Duration::from_millis(1500),
            max_buffer: Duration::from_secs(20),
        }
    }
}

/// Counters of both inputs.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MixerStats {
    /// The translated input.
    pub translated: LaneStats,
    /// The original input.
    pub original: LaneStats,
    /// Output samples produced so far.
    pub rendered_samples: u64,
    /// Level of the translated speech as it leaves the mixer (after its gain), last full window.
    pub translated_level: Level,
    /// Level of everything the mixer outputs, last full window.
    pub output_level: Level,
}

/// Length of the blocks [`Mixer::feed`] writes to a sink.
const FEED_BLOCK: Duration = Duration::from_millis(10);

/// The mixer of one direction. See the module docs.
pub struct Mixer {
    cfg: MixerConfig,
    mode: MixMode,
    translated: Lane,
    original: Lane,
    gain_translated: GainRamp,
    gain_original: GainRamp,
    crossfade_samples: usize,
    rendered: u64,
    seq: u64,
    translated_meter: LevelMeter,
    translated_level: Level,
    output_meter: LevelMeter,
    output_level: Level,
    scratch_translated: Vec<f32>,
    scratch_original: Vec<f32>,
}

impl Mixer {
    /// A mixer resting in `cfg.mode`.
    pub fn new(cfg: MixerConfig) -> Self {
        let rate = cfg.output_rate;
        let lane = |depth: Duration, timed: bool| {
            let adaptive = !timed && cfg.adaptive_preroll;
            Lane::new(LaneConfig {
                out_rate: rate.hz(),
                depth: rate.samples_in(depth),
                fade: rate.samples_in(cfg.fade),
                dry_fade: if timed { 0 } else { rate.samples_in(cfg.dry_fade) },
                capacity: rate.samples_in(cfg.max_buffer),
                timed,
                preroll: adaptive.then(|| PrerollConfig {
                    max_depth: rate.samples_in(cfg.preroll_max),
                    safety: 1.2,
                    default_len: rate.samples_in(Duration::from_secs(2)),
                    min_span: rate.samples_in(Duration::from_millis(100)) as u64,
                    max_stall: rate.samples_in(cfg.preroll_max.min(Duration::from_millis(600))),
                }),
            })
        };
        let (t, o) = gains_of(&cfg, cfg.mode);
        Self {
            cfg,
            mode: cfg.mode,
            translated: lane(cfg.jitter_buffer, false),
            original: lane(cfg.original_buffer, true),
            gain_translated: GainRamp::at(t),
            gain_original: GainRamp::at(o),
            crossfade_samples: rate.samples_in(cfg.crossfade),
            rendered: 0,
            seq: 0,
            translated_meter: LevelMeter::new(),
            translated_level: Level::default(),
            output_meter: LevelMeter::new(),
            output_level: Level::default(),
            scratch_translated: Vec::new(),
            scratch_original: Vec::new(),
        }
    }

    /// The output sample rate.
    pub fn output_rate(&self) -> SampleRate {
        self.cfg.output_rate
    }

    /// The mode the mixer is in or crossfading to.
    pub fn mode(&self) -> MixMode {
        self.mode
    }

    /// Switch to `mode`, crossfading from the gains reached now.
    pub fn set_mode(&mut self, mode: MixMode) {
        self.mode = mode;
        let (t, o) = gains_of(&self.cfg, mode);
        self.gain_translated.retarget(t, self.crossfade_samples);
        self.gain_original.retarget(o, self.crossfade_samples);
    }

    /// Queue a frame of translated speech.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when rate conversion fails.
    pub fn push_translated(&mut self, frame: &AudioFrame) -> Result<(), AudioError> {
        self.translated.push(frame)
    }

    /// Queue a frame of the original voice.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when rate conversion fails.
    pub fn push_original(&mut self, frame: &AudioFrame) -> Result<(), AudioError> {
        self.original.push(frame)
    }

    /// The translated stream has no more audio for now (end of an utterance
    /// or of the call): it plays out without waiting for the jitter buffer to
    /// fill, and running dry is not an underrun.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when the resampler fails.
    pub fn end_translated(&mut self) -> Result<(), AudioError> {
        self.translated.end_stream()
    }

    /// The original stream ended (the capture stopped).
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when the resampler fails.
    pub fn end_original(&mut self) -> Result<(), AudioError> {
        self.original.end_stream()
    }

    /// Drop the queued translated speech (skip, barge-in): the last few
    /// milliseconds fade out so that the cut is not a click.
    pub fn flush_translated(&mut self) {
        self.translated.flush();
    }

    /// Produce the next `out.len()` output samples.
    pub fn render(&mut self, out: &mut [f32]) {
        let n = out.len();
        self.scratch_translated.resize(n, 0.0);
        self.scratch_original.resize(n, 0.0);
        self.translated.next_block(&mut self.scratch_translated);
        self.original.next_block(&mut self.scratch_original);
        for (i, slot) in out.iter_mut().enumerate() {
            let t = self.gain_translated.next_gain() * self.scratch_translated[i];
            let o = self.gain_original.next_gain() * self.scratch_original[i];
            self.scratch_translated[i] = t;
            *slot = (t + o).clamp(-1.0, 1.0);
        }
        if let Some(level) = self
            .translated_meter
            .feed(&self.scratch_translated, self.cfg.output_rate.hz())
        {
            self.translated_level = level;
        }
        if let Some(level) = self.output_meter.feed(out, self.cfg.output_rate.hz()) {
            self.output_level = level;
        }
        self.rendered += n as u64;
    }

    /// Time of the next output sample on the mixer's own timeline (the
    /// sample count at the output rate, from zero).
    pub fn output_time(&self) -> MonoTime {
        MonoTime::ZERO.saturating_add(self.cfg.output_rate.duration_of(self.rendered as usize))
    }

    /// Render until `sink` holds `ahead` of audio, in blocks of 10 ms, and
    /// write them. Returns the output samples written. A write the sink
    /// refuses (full queue) ends the call.
    ///
    /// # Errors
    /// [`AudioError::InitializationFailed`] when a block is not a valid frame.
    pub fn feed(&mut self, sink: &mut dyn AudioSink, ahead: Duration) -> Result<usize, AudioError> {
        let rate = self.cfg.output_rate;
        let block = rate.samples_in(FEED_BLOCK).max(1);
        let missing = ahead.saturating_sub(sink.queued());
        let blocks = missing.as_nanos().div_ceil(FEED_BLOCK.as_nanos()) as usize;
        let mut written = 0;
        let mut samples = vec![0.0f32; block];
        for _ in 0..blocks {
            let ts = self.output_time();
            self.render(&mut samples);
            let frame = AudioFrame::new(samples.clone(), rate, ts, self.seq)
                .map_err(|e| AudioError::InitializationFailed(e.to_string()))?;
            self.seq += 1;
            let outcome = sink.write(&frame);
            written += outcome.accepted;
            if outcome.dropped > 0 {
                break;
            }
        }
        Ok(written)
    }

    /// Counters of both inputs.
    pub fn stats(&self) -> MixerStats {
        MixerStats {
            translated: self.translated.stats(),
            original: self.original.stats(),
            rendered_samples: self.rendered,
            translated_level: self.translated_level,
            output_level: self.output_level,
        }
    }
}

/// Linear gains of the translated and the original input in `mode`.
fn gains_of(cfg: &MixerConfig, mode: MixMode) -> (f32, f32) {
    let translated = db_to_linear(cfg.translated_gain_db);
    match mode {
        MixMode::Translation => (translated, 0.0),
        MixMode::Both => (translated, db_to_linear(cfg.original_gain_db)),
        MixMode::Original => (0.0, db_to_linear(cfg.passthrough_gain_db)),
    }
}

#[cfg(test)]
mod gap_tests;
#[cfg(test)]
mod tests;
