//! Level meter of a pump: peak and RMS of the audio that passed through, over fixed windows of audio time.
//!
//! The window is counted in samples, not wall time, so the meter behaves the
//! same for a live source and for a scripted one. A window that has not been
//! completed yet is not published, so a reader sees complete numbers only.

use std::sync::atomic::{AtomicU32, Ordering};

/// Length of one metering window.
pub const WINDOW_MS: u32 = 100;

/// Peak and RMS of one finished window, in linear amplitude (1.0 is full scale).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Level {
    /// Largest absolute sample.
    pub peak: f32,
    /// Root mean square.
    pub rms: f32,
}

/// Accumulates samples into windows.
#[derive(Debug, Default)]
pub struct LevelMeter {
    peak: f32,
    sum_squares: f64,
    count: usize,
    /// Samples per window; fixed by the first frame's rate.
    window: usize,
}

impl LevelMeter {
    /// An empty meter.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add `samples` captured at `rate_hz`; returns the level when a window
    /// completed (the last completed one if several did).
    pub fn feed(&mut self, samples: &[f32], rate_hz: u32) -> Option<Level> {
        if self.window == 0 {
            self.window = (rate_hz as usize * WINDOW_MS as usize / 1000).max(1);
        }
        let mut done = None;
        for &s in samples {
            let a = s.abs();
            if a > self.peak {
                self.peak = a;
            }
            self.sum_squares += f64::from(s) * f64::from(s);
            self.count += 1;
            if self.count >= self.window {
                done = Some(Level {
                    peak: self.peak,
                    rms: (self.sum_squares / self.count as f64).sqrt() as f32,
                });
                self.peak = 0.0;
                self.sum_squares = 0.0;
                self.count = 0;
            }
        }
        done
    }
}

/// A level that another thread can read: two atomics holding `f32` bits.
#[derive(Debug, Default)]
pub struct SharedLevel {
    peak: AtomicU32,
    rms: AtomicU32,
}

impl SharedLevel {
    /// Publish a level.
    pub fn store(&self, level: Level) {
        self.peak.store(level.peak.to_bits(), Ordering::Relaxed);
        self.rms.store(level.rms.to_bits(), Ordering::Relaxed);
    }

    /// The last published level (zero before the first window).
    pub fn load(&self) -> Level {
        Level {
            peak: f32::from_bits(self.peak.load(Ordering::Relaxed)),
            rms: f32::from_bits(self.rms.load(Ordering::Relaxed)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_reports_peak_and_rms_of_a_constant_signal() {
        let mut m = LevelMeter::new();
        // 100 ms at 1 kHz is 100 samples.
        let level = m.feed(&[0.5; 100], 1_000).expect("one window");
        assert!((level.peak - 0.5).abs() < 1e-6);
        assert!((level.rms - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_partial_window_is_not_published() {
        let mut m = LevelMeter::new();
        assert_eq!(m.feed(&[0.9; 99], 1_000), None);
        let level = m.feed(&[0.1], 1_000).expect("completed");
        assert!((level.peak - 0.9).abs() < 1e-6);
    }

    #[test]
    fn silence_is_zero_and_windows_restart() {
        let mut m = LevelMeter::new();
        assert_eq!(m.feed(&[0.7; 100], 1_000).map(|l| l.peak), Some(0.7));
        let level = m.feed(&[0.0; 100], 1_000).expect("second window");
        assert_eq!(level, Level::default());
    }

    #[test]
    fn shared_level_round_trips() {
        let s = SharedLevel::default();
        assert_eq!(s.load(), Level::default());
        s.store(Level { peak: 0.25, rms: 0.125 });
        assert_eq!(s.load(), Level { peak: 0.25, rms: 0.125 });
    }
}
