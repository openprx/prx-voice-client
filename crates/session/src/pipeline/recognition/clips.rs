//! Recognition stage, audio clips kept for the refine pass: trimming, cutting and the position after a unit.

use std::collections::VecDeque;
use std::time::Duration;

use prxvc_media::{MonoTime, SampleRate};

use super::super::messages::ClauseSamples;
use super::OpenSegment;

/// The segment's position after a unit that ended at `end` (committed or dropped): what follows
/// starts there, and the audio before it is no longer kept for the refine pass.
pub(super) fn advance_past(open: &mut OpenSegment, end: MonoTime, pad: Duration) {
    open.last_commit_ts = end;
    open.carry.clear();
    open.first_partial_traced = false;
    let keep_from = end.as_nanos().saturating_sub(duration_nanos(pad));
    while open
        .clips
        .front()
        .is_some_and(|(ts, _)| ts.as_nanos() < keep_from)
    {
        open.clips.pop_front();
    }
}

pub(super) fn duration_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// Drop the oldest clips beyond `keep` of audio.
pub(super) fn trim_clips(
    clips: &mut VecDeque<(MonoTime, Vec<f32>)>,
    keep: Duration,
    rate: SampleRate,
) {
    let mut total: usize = clips.iter().map(|(_, s)| s.len()).sum();
    while rate.duration_of(total) > keep {
        match clips.pop_front() {
            Some((_, s)) => total -= s.len(),
            None => break,
        }
    }
}

/// The audio of `[start - pad, end + pad]` from the kept clips.
pub(super) fn clip_audio(
    clips: &VecDeque<(MonoTime, Vec<f32>)>,
    start: MonoTime,
    end: MonoTime,
    pad: Duration,
    rate: SampleRate,
) -> ClauseSamples {
    let from = start.as_nanos().saturating_sub(duration_nanos(pad));
    let to = end.saturating_add(pad).as_nanos();
    let mut samples = Vec::new();
    for (ts, s) in clips {
        let clip_end = ts.saturating_add(rate.duration_of(s.len())).as_nanos();
        if clip_end > from && ts.as_nanos() < to {
            samples.extend_from_slice(s);
        }
    }
    ClauseSamples { samples, rate }
}
