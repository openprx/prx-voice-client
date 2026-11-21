//! `--vad-profile`: what the pipeline's VAD makes of each input, without running the pipeline. For the segment gate: how long the voiced runs are and how loud.

use prxvc_audio::resample::MonoResampler;
use prxvc_ports::VadProvider;

use crate::source::Recording;

/// The VAD's reading of one recording at one threshold.
#[derive(Debug, PartialEq)]
pub(crate) struct Profile {
    /// Frames at or above the threshold, in milliseconds.
    pub voiced_ms: f64,
    /// The longest run of such frames, in milliseconds.
    pub longest_run_ms: f64,
    /// Mean RMS of the frames of that longest run, dBFS (`None` when there is none).
    pub run_rms_dbfs: Option<f64>,
    /// Loudest frame RMS of the recording, dBFS.
    pub peak_frame_dbfs: f64,
}

fn dbfs(rms: f64) -> f64 {
    20.0 * rms.max(1e-9).log10()
}

/// Profile `rec` with `vad`, counting a frame as voiced from `threshold`.
pub(crate) fn profile(
    vad: &dyn VadProvider,
    rec: &Recording,
    threshold: f32,
) -> Result<Profile, String> {
    let mut engine = vad.open().map_err(|e| e.to_string())?;
    let rate = engine.rate();
    let samples = if rec.rate == rate {
        rec.samples.clone()
    } else {
        let mut r = MonoResampler::new(rec.rate.hz(), rate.hz()).map_err(|e| e.to_string())?;
        let mut s = r.stream_write(&rec.samples).map_err(|e| e.to_string())?;
        s.extend(r.stream_finish().map_err(|e| e.to_string())?);
        s
    };
    let n = engine.frame_len();
    let frame_ms = n as f64 * 1000.0 / f64::from(rate.hz());
    let mut voiced = 0usize;
    let (mut run, mut best, mut best_energy) = (0usize, 0usize, 0.0f64);
    let mut run_energy = 0.0f64;
    let mut peak = f64::MIN;
    for f in samples.chunks_exact(n) {
        let p = engine.probability(f).map_err(|e| e.to_string())?;
        let mean_sq = f.iter().map(|s| f64::from(*s).powi(2)).sum::<f64>() / n as f64;
        peak = peak.max(dbfs(mean_sq.sqrt()));
        if p >= threshold {
            voiced += 1;
            run += 1;
            run_energy += mean_sq;
            if run > best {
                best = run;
                best_energy = run_energy;
            }
        } else {
            run = 0;
            run_energy = 0.0;
        }
    }
    Ok(Profile {
        voiced_ms: voiced as f64 * frame_ms,
        longest_run_ms: best as f64 * frame_ms,
        run_rms_dbfs: (best > 0).then(|| dbfs((best_energy / best as f64).sqrt())),
        peak_frame_dbfs: if peak == f64::MIN { -120.0 } else { peak },
    })
}
