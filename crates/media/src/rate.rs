//! A validated, non-zero audio sample rate and the arithmetic between sample counts and durations.

use std::fmt;
use std::num::NonZeroU32;
use std::time::Duration;

/// A sample rate in hertz; never zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SampleRate(NonZeroU32);

/// A sample rate of zero was requested.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("sample rate must be non-zero")]
pub struct InvalidSampleRate;

impl SampleRate {
    /// 16 kHz: the rate of recognition and voice activity detection.
    pub const HZ_16000: Self = Self(NonZeroU32::MIN.saturating_add(15_999));
    /// 24 kHz: the rate of the synthesiser's output.
    pub const HZ_24000: Self = Self(NonZeroU32::MIN.saturating_add(23_999));
    /// 48 kHz: the rate of the call app's devices.
    pub const HZ_48000: Self = Self(NonZeroU32::MIN.saturating_add(47_999));

    /// A rate of `hz` hertz.
    pub fn new(hz: u32) -> Result<Self, InvalidSampleRate> {
        NonZeroU32::new(hz).map(Self).ok_or(InvalidSampleRate)
    }

    /// The rate in hertz.
    pub const fn hz(self) -> u32 {
        self.0.get()
    }

    /// The playing time of `samples` mono samples at this rate.
    pub fn duration_of(self, samples: usize) -> Duration {
        let nanos = (samples as u128) * 1_000_000_000 / u128::from(self.hz());
        Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
    }

    /// The number of mono samples in `d`, rounded down.
    pub fn samples_in(self, d: Duration) -> usize {
        let samples = d.as_nanos() * u128::from(self.hz()) / 1_000_000_000;
        usize::try_from(samples).unwrap_or(usize::MAX)
    }
}

impl fmt::Display for SampleRate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} Hz", self.hz())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_is_rejected_and_constants_have_their_values() {
        assert_eq!(SampleRate::new(0), Err(InvalidSampleRate));
        assert_eq!(SampleRate::new(16_000), Ok(SampleRate::HZ_16000));
        assert_eq!(SampleRate::HZ_24000.hz(), 24_000);
        assert_eq!(SampleRate::HZ_48000.hz(), 48_000);
        assert_eq!(SampleRate::HZ_16000.to_string(), "16000 Hz");
    }

    #[test]
    fn samples_and_durations_convert_both_ways() {
        let r = SampleRate::HZ_16000;
        assert_eq!(r.duration_of(320), Duration::from_millis(20));
        assert_eq!(r.samples_in(Duration::from_millis(20)), 320);
        assert_eq!(r.samples_in(Duration::from_micros(62)), 0);
        assert_eq!(r.duration_of(0), Duration::ZERO);
    }
}
