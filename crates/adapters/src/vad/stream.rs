//! A VAD engine with a speech gate, fed with arbitrary-length chunks of audio.

use std::time::Duration;

use prxvc_media::MonoTime;
use prxvc_ports::{VadEngine, VadError};

use super::gate::{GateParams, SpeechGate, VadEvent};

/// One direction's VAD: an engine scoring fixed-size frames and the gate that turns scores into
/// events on the pipeline clock.
///
/// Audio is fed in chunks of any length; incomplete frames wait for the next chunk. Time is
/// anchored at the first sample after construction or [`VadGate::reset`] (the capture time given
/// with that chunk) and advances by the sample count, so the events do not depend on how the chunks
/// are cut.
pub struct VadGate {
    engine: Box<dyn VadEngine>,
    gate: SpeechGate,
    pending: Vec<f32>,
    anchor: Option<MonoTime>,
    frames: u64,
    samples_fed: u64,
    last_probability: f32,
}

impl VadGate {
    /// A gate over `engine` with `params`.
    pub fn new(engine: Box<dyn VadEngine>, params: GateParams) -> Self {
        let rate = u64::from(engine.rate().hz());
        let frame = Duration::from_nanos(engine.frame_len() as u64 * 1_000_000_000 / rate.max(1));
        Self {
            engine,
            gate: SpeechGate::new(params, frame),
            pending: Vec::new(),
            anchor: None,
            frames: 0,
            samples_fed: 0,
            last_probability: 0.0,
        }
    }

    /// The probability of the most recent frame.
    pub fn last_probability(&self) -> f32 {
        self.last_probability
    }

    /// Whether a segment is open.
    pub fn is_speaking(&self) -> bool {
        self.gate.is_speaking()
    }

    /// Pipeline time of sample number `n` since the anchor.
    fn time_of(&self, anchor: MonoTime, samples: u64) -> MonoTime {
        let rate = u64::from(self.engine.rate().hz()).max(1);
        anchor.saturating_add(Duration::from_nanos(samples * 1_000_000_000 / rate))
    }

    /// Feed `audio`, whose first sample was captured at `ts`; returns the events it caused.
    pub fn feed(&mut self, audio: &[f32], ts: MonoTime) -> Result<Vec<VadEvent>, VadError> {
        let anchor = *self.anchor.get_or_insert(ts);
        self.samples_fed += audio.len() as u64;
        self.pending.extend_from_slice(audio);
        let frame_len = self.engine.frame_len();
        let mut events = Vec::new();
        let mut used = 0;
        while self.pending.len() - used >= frame_len {
            let frame = &self.pending[used..used + frame_len];
            let p = self.engine.probability(frame)?;
            used += frame_len;
            let start = self.time_of(anchor, self.frames * frame_len as u64);
            self.frames += 1;
            self.last_probability = p;
            events.extend(self.gate.push(p, start));
        }
        self.pending.drain(..used);
        Ok(events)
    }

    /// The input ended: close an open segment at the end of the audio fed so far.
    pub fn flush(&mut self) -> Option<VadEvent> {
        let anchor = self.anchor?;
        let now = self.time_of(anchor, self.samples_fed);
        self.pending.clear();
        self.gate.flush(now)
    }

    /// Forget all history (the start of a new call).
    pub fn reset(&mut self) {
        self.engine.reset();
        self.gate.reset();
        self.pending.clear();
        self.anchor = None;
        self.frames = 0;
        self.samples_fed = 0;
        self.last_probability = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use prxvc_ports::VadProvider;
    use prxvc_ports::fakes::vad::{FakeVadProvider, VadScript};

    use super::*;

    fn gate(probs: Vec<f32>) -> VadGate {
        let engine = FakeVadProvider::new(VadScript::probabilities(probs))
            .open()
            .expect("open");
        VadGate::new(engine, GateParams::default())
    }

    fn script() -> Vec<f32> {
        let mut p = vec![0.0; 10];
        p.extend(vec![0.9; 30]);
        p.extend(vec![0.0; 40]);
        p
    }

    #[test]
    fn events_do_not_depend_on_how_the_audio_is_cut() {
        let total = 80 * 512;
        let audio = vec![0.0f32; total];
        let run = |chunk: usize| {
            let mut g = gate(script());
            let mut events = Vec::new();
            for (i, part) in audio.chunks(chunk).enumerate() {
                let ts = MonoTime::from_nanos(1_000_000 + (i * chunk) as u64 * 62_500);
                events.extend(g.feed(part, ts).expect("feed"));
            }
            events
        };
        let whole = run(total);
        assert_eq!(whole.len(), 2);
        assert_eq!(run(512), whole);
        assert_eq!(run(160), whole);
        assert_eq!(run(333), whole);
        // Times are on the pipeline clock: the anchor is the first chunk's capture time.
        let VadEvent::SpeechStart { at, .. } = whole[0] else {
            panic!("expected a start");
        };
        assert_eq!(at.as_nanos(), 1_000_000 + 10 * 32_000_000 - 200_000_000);
    }

    #[test]
    fn reset_restarts_the_script_and_the_clock() {
        let mut g = gate(script());
        let audio = vec![0.0f32; 80 * 512];
        let first = g.feed(&audio, MonoTime::from_millis(5)).expect("feed");
        g.reset();
        assert!(!g.is_speaking());
        let second = g.feed(&audio, MonoTime::from_millis(5)).expect("feed");
        assert_eq!(first, second);
    }

    #[test]
    fn flush_ends_an_open_segment_at_the_end_of_the_audio() {
        let mut g = gate(vec![0.9; 40]);
        let events = g
            .feed(&vec![0.0f32; 40 * 512], MonoTime::ZERO)
            .expect("feed");
        assert_eq!(events.len(), 1);
        let Some(VadEvent::SpeechEnd { end, .. }) = g.flush() else {
            panic!("expected an end");
        };
        assert_eq!(end.as_nanos(), 40 * 32_000_000);
    }

    #[test]
    fn an_engine_failure_is_returned() {
        let provider = FakeVadProvider::new(
            VadScript::probabilities([0.0; 4]).failing_at(2, VadError::Internal("boom".to_owned())),
        );
        let mut g = VadGate::new(provider.open().expect("open"), GateParams::default());
        assert_eq!(
            g.feed(&vec![0.0f32; 4 * 512], MonoTime::ZERO),
            Err(VadError::Internal("boom".to_owned()))
        );
    }
}
