//! Mapping of tap capture times (Core Audio host clock) onto the pipeline clock, so that frames carry the time they were captured rather than the time they were read.
//!
//! Pure: the caller supplies the host time, the current pipeline time and the
//! one calibration pair, so the mapping is tested with synthetic timestamps. On
//! macOS the host clock and the pipeline's `Instant`-based clock both count
//! uptime, so one calibration pair at tap start fixes their offset; a stamp that
//! lands implausibly far from the read time (a clock that does not behave that
//! way, or a stale slot) falls back to the read time and is counted.

use std::time::Duration;

use prxvc_media::MonoTime;

/// Largest distance between a mapped capture time and the read time that is
/// still believed. The ring holds about 1.5 s of audio, so anything beyond this
/// is a clock problem, not queueing.
pub const MAX_SKEW: Duration = Duration::from_secs(3);

/// Offset between the host clock and the pipeline clock, fixed by one pair of
/// readings taken at the same moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostTimeMap {
    host_ref_ns: u64,
    mono_ref: MonoTime,
}

impl HostTimeMap {
    /// Calibrate from a host time (nanoseconds) and a pipeline time read together.
    pub fn new(host_now_ns: u64, mono_now: MonoTime) -> Self {
        Self {
            host_ref_ns: host_now_ns,
            mono_ref: mono_now,
        }
    }

    /// The pipeline time of a host time, or `None` when it falls before the
    /// pipeline clock's origin.
    pub fn to_mono(&self, host_ns: u64) -> Option<MonoTime> {
        let mono_ref = i128::from(self.mono_ref.as_nanos());
        let mapped = mono_ref + i128::from(host_ns) - i128::from(self.host_ref_ns);
        u64::try_from(mapped).ok().map(MonoTime::from_nanos)
    }
}

/// Stamps frames with capture time: mapped when believable, the read time
/// otherwise, never earlier than the previous stamp and never later than the
/// read time.
#[derive(Debug, Clone)]
pub struct FrameStamper {
    map: HostTimeMap,
    max_skew: Duration,
    last: MonoTime,
    fallbacks: u64,
}

impl FrameStamper {
    /// A stamper using `map`, believing mapped times within [`MAX_SKEW`] of the read time.
    pub fn new(map: HostTimeMap) -> Self {
        Self {
            map,
            max_skew: MAX_SKEW,
            last: MonoTime::ZERO,
            fallbacks: 0,
        }
    }

    /// Frames stamped with the read time because the capture time was missing or implausible.
    pub fn fallbacks(&self) -> u64 {
        self.fallbacks
    }

    /// The stamp for a frame captured at `host_ns` (0 = unknown) and read at `now`.
    pub fn stamp(&mut self, host_ns: u64, now: MonoTime) -> MonoTime {
        let mapped = if host_ns == 0 {
            None
        } else {
            self.map.to_mono(host_ns)
        };
        let believed = mapped.filter(|m| {
            let skew = if *m >= now {
                m.saturating_since(now)
            } else {
                now.saturating_since(*m)
            };
            skew <= self.max_skew
        });
        let stamp = match believed {
            Some(m) => m.min(now),
            None => {
                self.fallbacks += 1;
                now
            }
        };
        let stamp = stamp.max(self.last);
        self.last = stamp;
        stamp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> MonoTime {
        MonoTime::from_millis(n)
    }

    /// Host clock 5 s ahead of the pipeline clock's origin.
    fn stamper() -> FrameStamper {
        FrameStamper::new(HostTimeMap::new(5_000_000_000 + 1_000_000_000, ms(1_000)))
    }

    #[test]
    fn host_times_map_by_the_calibrated_offset() {
        let map = HostTimeMap::new(6_000_000_000, ms(1_000));
        assert_eq!(map.to_mono(6_000_000_000), Some(ms(1_000)));
        assert_eq!(map.to_mono(6_250_000_000), Some(ms(1_250)));
        assert_eq!(map.to_mono(5_900_000_000), Some(ms(900)));
        // Before the pipeline clock's origin.
        assert_eq!(map.to_mono(4_000_000_000), None);
    }

    #[test]
    fn a_frame_gets_its_capture_time_not_the_read_time() {
        let mut s = stamper();
        // Captured at pipeline time 1_200 ms, read 90 ms later.
        assert_eq!(s.stamp(6_200_000_000, ms(1_290)), ms(1_200));
        assert_eq!(s.fallbacks(), 0);
    }

    #[test]
    fn consecutive_slots_of_one_cycle_keep_their_spacing() {
        let mut s = stamper();
        let first = s.stamp(6_200_000_000, ms(1_300));
        let second = s.stamp(6_200_000_000 + 21_333_333, ms(1_300));
        assert_eq!(
            second.saturating_since(first),
            Duration::from_nanos(21_333_333)
        );
    }

    #[test]
    fn unknown_host_time_falls_back_to_the_read_time_and_is_counted() {
        let mut s = stamper();
        assert_eq!(s.stamp(0, ms(1_500)), ms(1_500));
        assert_eq!(s.fallbacks(), 1);
    }

    #[test]
    fn an_implausible_mapping_falls_back() {
        let mut s = stamper();
        // 10 s in the past, and 10 s in the future.
        assert_eq!(s.stamp(6_000_000_000, ms(11_000)), ms(11_000));
        assert_eq!(s.stamp(26_000_000_000, ms(11_100)), ms(11_100));
        assert_eq!(s.fallbacks(), 2);
    }

    #[test]
    fn stamps_never_go_backwards_and_never_pass_the_read_time() {
        let mut s = stamper();
        assert_eq!(s.stamp(6_400_000_000, ms(1_450)), ms(1_400));
        // An older capture time than the previous stamp is held at it.
        assert_eq!(s.stamp(6_390_000_000, ms(1_460)), ms(1_400));
        // A capture time slightly after the read time is clamped to it.
        assert_eq!(s.stamp(6_700_000_000, ms(1_500)), ms(1_500));
    }

    #[test]
    fn capture_time_before_the_origin_falls_back() {
        let mut s = FrameStamper::new(HostTimeMap::new(6_000_000_000, ms(100)));
        assert_eq!(s.stamp(5_000_000_000, ms(200)), ms(200));
        assert_eq!(s.fallbacks(), 1);
    }
}
