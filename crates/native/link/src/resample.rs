//! Windowed-sinc sample-rate conversion for the spike's audio (24 kHz to 16 kHz and back).
//!
//! Plain and unoptimised on purpose: it only prepares test inputs. The product path uses the
//! workspace's resampler (`rubato`).

use std::f64::consts::PI;

/// Half-width of the sinc kernel in input samples at the lower of the two rates.
const HALF_TAPS: f64 = 24.0;

fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (PI * x).sin() / (PI * x)
    }
}

/// Convert `input` from `from_hz` to `to_hz`. The output has `round(len * to / from)` samples.
pub fn resample(input: &[f32], from_hz: u32, to_hz: u32) -> Vec<f32> {
    if from_hz == to_hz || input.is_empty() {
        return input.to_vec();
    }
    let ratio = f64::from(to_hz) / f64::from(from_hz);
    // Cut-off at the lower Nyquist; the kernel is stretched when downsampling.
    let scale = ratio.min(1.0);
    let half = HALF_TAPS / scale;
    let out_len = (input.len() as f64 * ratio).round() as usize;
    (0..out_len)
        .map(|n| {
            let centre = n as f64 / ratio;
            let lo = ((centre - half).ceil() as i64).max(0);
            let hi = ((centre + half).floor() as i64).min(input.len() as i64 - 1);
            let mut acc = 0.0f64;
            for i in lo..=hi {
                let d = i as f64 - centre;
                let window = 0.5 + 0.5 * (PI * d / half).cos();
                acc += f64::from(input[i as usize]) * sinc(d * scale) * window * scale;
            }
            acc as f32
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: u32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| (2.0 * PI * freq * i as f64 / f64::from(rate)).sin() as f32)
            .collect()
    }

    fn rms(x: &[f32]) -> f64 {
        (x.iter().map(|v| f64::from(*v).powi(2)).sum::<f64>() / x.len() as f64).sqrt()
    }

    #[test]
    fn output_length_follows_the_ratio() {
        assert_eq!(resample(&vec![0.0; 24000], 24000, 16000).len(), 16000);
        assert_eq!(resample(&vec![0.0; 16000], 16000, 24000).len(), 24000);
    }

    #[test]
    fn in_band_tone_keeps_its_level() {
        let out = resample(&sine(440.0, 24000, 24000), 24000, 16000);
        let mid = &out[2000..14000];
        assert!(
            (rms(mid) - std::f64::consts::FRAC_1_SQRT_2).abs() < 0.02,
            "rms {}",
            rms(mid)
        );
    }

    #[test]
    fn out_of_band_tone_is_removed() {
        // 10 kHz is above the 8 kHz Nyquist of the 16 kHz output.
        let out = resample(&sine(10000.0, 24000, 24000), 24000, 16000);
        assert!(rms(&out[2000..14000]) < 0.02);
    }

    #[test]
    fn same_rate_is_identity() {
        let x = sine(100.0, 16000, 100);
        assert_eq!(resample(&x, 16000, 16000), x);
    }
}
