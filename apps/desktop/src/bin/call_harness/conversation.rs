//! Back-to-back utterances through one running pipeline: the recordings of a set are laid one after the other with a short gap, so that the translated speech of one is still playing when the next has been translated, as in a real conversation. Shows the queue wait (the time between a clause's synthesised audio being ready and its playout starting) and how long the translation runs on after the last speech.

use std::time::Duration;

use prxvc_media::SampleRate;
use prxvc_ports::TraceStage as S;

use crate::report::UtteranceRun;
use crate::source::Recording;

/// The recordings joined with `gap` of silence between them; `starts[i]` is where recording `i` begins (seconds) and `bounds` the first speech start and the last speech end (seconds), from each recording's own speech bounds.
pub(crate) struct Joined {
    pub recording: Recording,
    pub bounds: (f64, f64),
    pub spans: Vec<(f64, f64)>,
}

/// Join `parts` (recording and its speech start and end) with `gap` between them.
pub(crate) fn join(parts: &[(Recording, (f64, f64))], gap: Duration) -> Result<Joined, String> {
    let rate: SampleRate = parts.first().ok_or("no recordings")?.0.rate;
    let mut samples = Vec::new();
    let mut spans = Vec::new();
    for (i, (rec, bounds)) in parts.iter().enumerate() {
        if rec.rate != rate {
            return Err(format!("recording {i} is {}, the first is {rate}", rec.rate));
        }
        if i > 0 {
            samples.extend(std::iter::repeat_n(0.0, rate.samples_in(gap)));
        }
        let offset = samples.len() as f64 / f64::from(rate.hz());
        spans.push((offset + bounds.0, offset + bounds.1));
        samples.extend_from_slice(&rec.samples);
    }
    let bounds = (spans[0].0, spans[spans.len() - 1].1);
    Ok(Joined {
        recording: Recording { samples, rate },
        bounds,
        spans,
    })
}

/// One run per segment of the joined run: its clauses, with the speech start of its first clause.
pub(crate) fn split(run: &UtteranceRun) -> Vec<UtteranceRun> {
    let mut segments: Vec<u64> = run.clauses.iter().map(|c| c.clause.segment().seq()).collect();
    segments.dedup();
    segments
        .into_iter()
        .map(|seq| {
            let clauses: Vec<_> = run
                .clauses
                .iter()
                .filter(|c| c.clause.segment().seq() == seq)
                .cloned()
                .collect();
            let start = clauses
                .iter()
                .filter_map(|c| c.at(S::SpeechStart))
                .map(|t| t.as_nanos() as f64)
                .fold(f64::MAX, f64::min);
            let start = if start == f64::MAX { run.speech_start_ns } else { start };
            let audio_seconds = run
                .clauses
                .iter()
                .zip(&run.audio_seconds)
                .filter(|(c, _)| c.clause.segment().seq() == seq)
                .map(|(_, a)| *a)
                .collect();
            UtteranceRun {
                id: format!("{}#{seq}", run.id),
                clauses,
                audio_seconds,
                speech_start_ns: start,
                speech_end_ns: start,
                ..run.clone()
            }
        })
        .collect()
}

/// Wait between a clause's first synthesised audio and its playout starting, in seconds.
pub(crate) fn queue_waits(runs: &[UtteranceRun]) -> Vec<f64> {
    runs.iter()
        .filter_map(|r| r.clauses.first())
        .filter_map(|c| c.ms(S::TtsFirstChunk, S::PlayoutStart))
        .map(|ms| ms / 1000.0)
        .collect()
}

/// The lines printed after a conversation run.
pub(crate) fn summary(runs: &[UtteranceRun], whole: &UtteranceRun, last_speech_end_ns: f64) -> String {
    let waits = queue_waits(runs);
    let stat = |v: &[f64]| {
        crate::report::median_p95(v).map_or("-".to_owned(), |(m, p)| format!("median {m:.2}, p95 {p:.2}, max {:.2}", v.iter().copied().fold(0.0, f64::max)))
    };
    let last_end = whole
        .clauses
        .iter()
        .filter_map(|c| c.at(S::PlayoutEnd))
        .map(|t| t.as_nanos() as f64)
        .fold(0.0, f64::max);
    let played: f64 = whole.audio_seconds.iter().sum();
    format!(
        "conversation: {} segments, {} clauses, {:.1} s of translated audio before any speed-up\n\
         wait from first synthesised audio to playout start, first clause of each segment (s): {}\n\
         translated speech ended {:.2} s after the last speech ended",
        runs.len(),
        whole.clauses.len(),
        played,
        stat(&waits),
        (last_end - last_speech_end_ns) / 1e9
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(seconds: f64) -> Recording {
        Recording {
            samples: vec![0.1; (seconds * 16_000.0) as usize],
            rate: SampleRate::HZ_16000,
        }
    }

    #[test]
    fn recordings_are_joined_with_the_gap_and_their_speech_is_placed() {
        let parts = [(rec(2.0), (0.2, 1.8)), (rec(1.0), (0.1, 0.9))];
        let j = join(&parts, Duration::from_millis(900)).unwrap();
        assert!((j.recording.seconds() - 3.9).abs() < 1e-6);
        assert!((j.spans[0].0 - 0.2).abs() < 1e-6 && (j.spans[0].1 - 1.8).abs() < 1e-6);
        assert!((j.spans[1].0 - 3.0).abs() < 1e-6 && (j.spans[1].1 - 3.8).abs() < 1e-6);
        assert_eq!(j.bounds, (j.spans[0].0, j.spans[1].1));
        assert!(join(&[], Duration::ZERO).is_err());
    }

    #[test]
    fn recordings_at_different_rates_are_refused() {
        let other = Recording { samples: vec![0.0; 10], rate: SampleRate::HZ_48000 };
        assert!(join(&[(rec(1.0), (0.0, 1.0)), (other, (0.0, 0.1))], Duration::ZERO).is_err());
    }
}

