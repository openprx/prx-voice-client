//! End-of-utterance decision, independent of the recogniser.
//!
//! The decision looks only at the speech segments the VAD reported and at how
//! much audio has been buffered, both measured in samples of the audio itself,
//! so it needs no wall clock and behaves the same however often the audio loop
//! polls: an utterance ends when the audio after the last speech segment is at
//! least `end_silence` long, or when it has run for `max_utterance`.

use std::time::Duration;

use crate::vad_window::{SpeechSegment, VAD_SAMPLE_RATE};

/// Parameters of the end-of-utterance decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointConfig {
    /// Silence after the last speech that ends the utterance.
    pub end_silence: Duration,
    /// Longest an utterance may run before it is cut regardless of speech.
    pub max_utterance: Duration,
}

/// Longest utterance before a forced cut.
pub const DEFAULT_MAX_UTTERANCE: Duration = Duration::from_secs(30);

impl EndpointConfig {
    /// `end_silence_ms` from the configuration, the default maximum length.
    pub fn from_end_silence_ms(end_silence_ms: u32) -> Self {
        Self {
            end_silence: Duration::from_millis(u64::from(end_silence_ms)),
            max_utterance: DEFAULT_MAX_UTTERANCE,
        }
    }
}

/// Why an utterance was finalised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndReason {
    /// The speaker was silent for `end_silence`.
    Silence,
    /// The utterance reached `max_utterance`.
    MaxLength,
}

/// What to do with the audio buffered so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endpoint {
    /// No speech in the buffer.
    NoSpeech,
    /// Speech is present and may continue: keep listening.
    Continue,
    /// The utterance is over: transcribe and send it.
    Finalize(EndReason),
}

fn samples(d: Duration) -> usize {
    (d.as_secs_f64() * VAD_SAMPLE_RATE as f64).round() as usize
}

/// Decide whether the utterance in a buffer of `buffered` samples with the
/// VAD `segments` is over.
pub fn decide(cfg: &EndpointConfig, buffered: usize, segments: &[SpeechSegment]) -> Endpoint {
    let (Some(first), Some(last)) = (segments.first(), segments.last()) else {
        return Endpoint::NoSpeech;
    };
    if buffered.saturating_sub(first.0) >= samples(cfg.max_utterance) {
        return Endpoint::Finalize(EndReason::MaxLength);
    }
    if buffered.saturating_sub(last.1) >= samples(cfg.end_silence) {
        return Endpoint::Finalize(EndReason::Silence);
    }
    Endpoint::Continue
}

/// Total duration of speech in `segments`.
pub fn speech_duration(segments: &[SpeechSegment]) -> Duration {
    let total: usize = segments.iter().map(|(s, e)| e.saturating_sub(*s)).sum();
    Duration::from_secs_f64(total as f64 / VAD_SAMPLE_RATE as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: usize = VAD_SAMPLE_RATE;
    fn cfg() -> EndpointConfig {
        EndpointConfig::from_end_silence_ms(800)
    }

    #[test]
    fn no_segments_means_no_speech() {
        assert_eq!(decide(&cfg(), 5 * SR, &[]), Endpoint::NoSpeech);
    }

    #[test]
    fn a_pause_shorter_than_the_threshold_does_not_end_the_utterance() {
        // Speech 1.0-2.0 s, buffer ends at 2.7 s: 700 ms of silence.
        assert_eq!(decide(&cfg(), 2 * SR + 700 * 16, &[(SR, 2 * SR)]), Endpoint::Continue);
    }

    #[test]
    fn silence_of_the_threshold_ends_the_utterance() {
        assert_eq!(
            decide(&cfg(), 2 * SR + 800 * 16, &[(SR, 2 * SR)]),
            Endpoint::Finalize(EndReason::Silence)
        );
    }

    #[test]
    fn speech_running_up_to_the_buffer_end_continues() {
        assert_eq!(decide(&cfg(), 3 * SR, &[(SR, 3 * SR)]), Endpoint::Continue);
    }

    #[test]
    fn the_threshold_counts_from_the_last_segment_not_the_first() {
        // Two segments with a 700 ms gap, then 500 ms of trailing silence.
        let segs = [(0, SR), (SR + 700 * 16, 2 * SR)];
        assert_eq!(decide(&cfg(), 2 * SR + 500 * 16, &segs), Endpoint::Continue);
    }

    #[test]
    fn an_over_long_utterance_is_cut_even_while_speech_continues() {
        let len = 31 * SR;
        assert_eq!(
            decide(&cfg(), len, &[(SR, len)]),
            Endpoint::Finalize(EndReason::MaxLength)
        );
    }

    #[test]
    fn speech_duration_sums_the_segments() {
        let d = speech_duration(&[(0, SR / 2), (SR, SR + SR / 4)]);
        assert_eq!(d, Duration::from_millis(750));
    }
}
