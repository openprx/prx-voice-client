//! Gain arithmetic of the mixer: decibels to linear and the smooth ramp between two gains.

use std::f32::consts::PI;

/// Linear amplitude factor of a gain in decibels.
pub fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// A gain that moves from one value to another along a raised cosine.
///
/// The curve has zero slope at both ends, so a ramp that starts or ends never
/// adds a corner to the signal, and a new target set while a ramp is running
/// starts from the current value, so the gain is continuous in every case.
/// For two correlated inputs crossfading from `(1, 0)` to `(0, 1)` the gains
/// sum to one at every sample.
#[derive(Debug, Clone)]
pub struct GainRamp {
    from: f32,
    to: f32,
    pos: usize,
    len: usize,
    current: f32,
}

impl GainRamp {
    /// A ramp resting at `gain`.
    pub fn at(gain: f32) -> Self {
        Self {
            from: gain,
            to: gain,
            pos: 0,
            len: 0,
            current: gain,
        }
    }

    /// Move to `target` over `len` samples, starting from the current value.
    pub fn retarget(&mut self, target: f32, len: usize) {
        if len == 0 {
            *self = Self::at(target);
            return;
        }
        self.from = self.current;
        self.to = target;
        self.pos = 0;
        self.len = len;
    }

    /// The gain for the next sample.
    pub fn next_gain(&mut self) -> f32 {
        if self.pos < self.len {
            self.pos += 1;
            let p = self.pos as f32 / self.len as f32;
            let shape = 0.5 - 0.5 * (PI * p).cos();
            self.current = self.from + (self.to - self.from) * shape;
        } else {
            self.current = self.to;
        }
        self.current
    }
}
