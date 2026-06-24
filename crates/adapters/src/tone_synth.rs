//! Tone-based test synthesizer: a sine tone whose length tracks the text.
//!
//! This is not real speech. It lets the end-to-end pipeline run and play
//! audible output without a model or network, which is useful for development
//! and tests. Clone vs default voice is signalled by a pitch change, so the
//! routing (Self→Peer uses the cloned voice) is audible in demos.

use prxvc_session::adapter::{AdapterError, SynthesizedAudio, Synthesizer};

/// Sine-tone test synthesizer.
#[derive(Debug, Clone, Copy)]
pub struct SineToneSynthesizer {
    /// Output sample rate.
    pub sample_rate: u32,
    /// Milliseconds of audio generated per character.
    pub ms_per_char: u32,
    /// Tone frequency for the cloned voice (Self→Peer).
    pub clone_freq_hz: f32,
    /// Tone frequency for the default voice (Peer→Self).
    pub default_freq_hz: f32,
}

impl Default for SineToneSynthesizer {
    fn default() -> Self {
        Self {
            sample_rate: 24_000,
            ms_per_char: 60,
            clone_freq_hz: 330.0,
            default_freq_hz: 220.0,
        }
    }
}

impl Synthesizer for SineToneSynthesizer {
    fn synthesize(
        &mut self,
        text: &str,
        _language: &str,
        clone_voice: bool,
    ) -> Result<SynthesizedAudio, AdapterError> {
        let chars = text.chars().count().max(1) as u32;
        let total_ms = chars * self.ms_per_char;
        let n = ((self.sample_rate as u64 * total_ms as u64) / 1000) as usize;
        let freq = if clone_voice { self.clone_freq_hz } else { self.default_freq_hz };

        let mut samples = Vec::with_capacity(n);
        let step = std::f32::consts::TAU * freq / self.sample_rate as f32;
        for i in 0..n {
            // Gentle 0.2 amplitude to avoid clipping when injected.
            samples.push(0.2 * (step * i as f32).sin());
        }
        Ok(SynthesizedAudio {
            samples,
            sample_rate: self.sample_rate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn length_tracks_text_and_rate() {
        let mut s = SineToneSynthesizer::default();
        let out = s.synthesize("abcde", "en", false).unwrap();
        assert_eq!(out.sample_rate, 24_000);
        // 5 chars * 60ms = 300ms @ 24k = 7200 samples.
        assert_eq!(out.samples.len(), 7_200);
    }

    #[test]
    fn clone_flag_changes_pitch() {
        let mut s = SineToneSynthesizer::default();
        let a = s.synthesize("aaaa", "en", true).unwrap();
        let b = s.synthesize("aaaa", "en", false).unwrap();
        // Same length, different waveform (different frequency).
        assert_eq!(a.samples.len(), b.samples.len());
        assert_ne!(a.samples, b.samples);
    }

    #[test]
    fn empty_text_still_produces_audio() {
        let mut s = SineToneSynthesizer::default();
        let out = s.synthesize("", "en", false).unwrap();
        assert!(!out.samples.is_empty());
    }
}
