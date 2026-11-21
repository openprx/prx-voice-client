//! Catching up with a backlog without dropping anything: when the translated speech waits behind speech still being played, the clauses played next are sped up moderately (pitch kept) and their pauses cut short; when the queue is empty the playback returns to normal.
//!
//! The listener falls behind when the translated speech takes longer to say than the speaker took,
//! so that clauses queue up behind each other. The backlog rule skips stale clauses only far
//! beyond what is comfortable; this rule acts early and loses nothing. The decision is pure: the
//! queued audio (clauses waiting for their turn, by their real or estimated length) and the pace
//! now give the pace for the clauses released next. It has hysteresis, so that the speed does not
//! flutter around the threshold, and moves in steps of 0.05.

use std::time::Duration;

/// How playback is paced: the speed and whether pauses between clauses are cut short.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlayPace {
    /// 1.0 is normal.
    pub speed: f32,
    /// Pauses are cut short (anything is queued).
    pub tight: bool,
}

impl Default for PlayPace {
    fn default() -> Self {
        Self {
            speed: 1.0,
            tight: false,
        }
    }
}

/// `pipeline.catch_up`, `catch_up_after_ms` and `catch_up_max_speed`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CatchUpParams {
    /// Speed up at all.
    pub enabled: bool,
    /// Queued audio at which the speed-up starts.
    pub after: Duration,
    /// The fastest playback (1.25 by default).
    pub max_speed: f32,
}

impl Default for CatchUpParams {
    fn default() -> Self {
        Self {
            enabled: true,
            after: Duration::from_secs(2),
            max_speed: 1.25,
        }
    }
}

/// Queued audio at which pauses are cut short, and below which they are normal again.
const TIGHT_FROM: Duration = Duration::from_millis(300);
const TIGHT_UNTIL: Duration = Duration::from_millis(100);

/// The pace for the clauses released next, given the audio `queued` behind the one being released.
pub fn decide(params: &CatchUpParams, queued: Duration, now: PlayPace) -> PlayPace {
    let tight = if now.tight {
        queued > TIGHT_UNTIL
    } else {
        queued >= TIGHT_FROM
    };
    let max = params.max_speed.clamp(1.0, 2.0);
    let active = params.enabled && max > 1.0;
    let enter = params.after;
    let stay = params.after / 2;
    let speeding = if now.speed > 1.0 { queued >= stay } else { queued >= enter };
    let speed = if active && speeding {
        // From just above 1 where it starts to the maximum at twice that queue.
        let ramp = (queued.as_secs_f32() / (2.0 * enter.as_secs_f32().max(0.001))).clamp(0.0, 1.0);
        let raw = 1.0 + (max - 1.0) * ramp.max(0.4);
        ((raw * 20.0).round() / 20.0).clamp(1.05_f32.min(max), max)
    } else {
        1.0
    };
    PlayPace { speed, tight }
}

/// Shortest time a clause must have been synthesising before its rate is believed.
const RATE_MIN_ELAPSED: Duration = Duration::from_millis(300);
/// How many times faster than real time the producer must be before a clause that is still being
/// synthesised is sped up: playing at the fastest speed (at most 2.0, 1.25 by default) consumes
/// audio that much faster, so the margin keeps the lane from running dry.
const RATE_MARGIN: f32 = 1.5;

/// Whether a clause still being synthesised is produced comfortably faster than it is played at a
/// sped-up pace: it has `synthesised` audio after `elapsed` since its first chunk. Never true on
/// too short an observation, so a speed-up can not open a gap.
pub fn producer_comfortably_ahead(synthesised: Duration, elapsed: Duration) -> bool {
    elapsed >= RATE_MIN_ELAPSED && synthesised.as_secs_f32() >= RATE_MARGIN * elapsed.as_secs_f32()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_producer_is_believed_only_when_it_was_seen_long_enough_and_is_well_ahead() {
        let ms = Duration::from_millis;
        assert!(super::producer_comfortably_ahead(ms(2_000), ms(500)), "four times real time");
        assert!(!super::producer_comfortably_ahead(ms(200), ms(100)), "too short an observation");
        assert!(!super::producer_comfortably_ahead(ms(600), ms(500)), "barely faster than playing");
        assert!(!super::producer_comfortably_ahead(ms(100), ms(600)), "slower than real time");
    }

    use super::*;

    fn s(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn nothing_queued_is_normal_pace() {
        let p = decide(&CatchUpParams::default(), s(0), PlayPace::default());
        assert_eq!(p, PlayPace::default());
    }

    #[test]
    fn pauses_are_cut_short_from_a_little_queue_and_the_speed_waits_for_more() {
        let p = decide(&CatchUpParams::default(), s(800), PlayPace::default());
        assert!(p.tight);
        assert_eq!(p.speed, 1.0);
    }

    #[test]
    fn the_speed_rises_with_the_queue_and_never_passes_the_maximum() {
        let params = CatchUpParams::default();
        let a = decide(&params, s(2_000), PlayPace::default()).speed;
        let b = decide(&params, s(3_000), PlayPace::default()).speed;
        let c = decide(&params, s(30_000), PlayPace::default()).speed;
        assert!(a > 1.0 && a < b && b < c, "{a} {b} {c}");
        assert_eq!(c, 1.25);
    }

    #[test]
    fn it_returns_to_normal_with_hysteresis() {
        let params = CatchUpParams::default();
        let fast = PlayPace { speed: 1.2, tight: true };
        assert!(decide(&params, s(1_200), fast).speed > 1.0, "still above half the start");
        assert_eq!(decide(&params, s(900), fast).speed, 1.0);
        assert_eq!(decide(&params, s(1_200), PlayPace::default()).speed, 1.0, "not yet entering");
        assert!(decide(&params, s(250), PlayPace { speed: 1.0, tight: true }).tight);
        assert!(!decide(&params, s(50), PlayPace { speed: 1.0, tight: true }).tight);
    }

    #[test]
    fn switched_off_it_never_speeds_up_but_pauses_still_shorten() {
        let off = CatchUpParams { enabled: false, ..CatchUpParams::default() };
        let p = decide(&off, s(20_000), PlayPace::default());
        assert_eq!(p.speed, 1.0);
        assert!(p.tight);
        let flat = CatchUpParams { max_speed: 1.0, ..CatchUpParams::default() };
        assert_eq!(decide(&flat, s(20_000), PlayPace::default()).speed, 1.0);
    }
}
