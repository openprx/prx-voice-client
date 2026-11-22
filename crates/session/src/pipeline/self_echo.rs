//! Our own translated speech coming back through the microphone.
//!
//! In speaker or same-room conditions the microphone of the outgoing direction hears the English
//! that our synthesiser just played to the other party (from the other phone's loudspeaker). The
//! recogniser reads it as English, and passing it through as "already in the listener's language"
//! would send the far-end echo back into the call. This module keeps a short history of the text
//! the outgoing lane synthesised and says when a recognised unit is that text again.
//!
//! The comparison is on normalised units (words, or one per CJK character): the share of the heard
//! units that follow in order in the recent own speech (longest common subsequence) must reach
//! [`SIMILARITY`], and at least [`MIN_UNITS`] units must have been heard, so that a short "okay"
//! of the speaker is never taken for the echo of an "okay" of ours.

use std::collections::VecDeque;
use std::time::Duration;

use prxvc_media::MonoTime;
use prxvc_media::text_units::units;

/// How long after a clause ended (played, skipped or failed) its text still counts as own speech.
pub(crate) const WINDOW: Duration = Duration::from_secs(15);
/// Share of the heard units that must be found in order in the own speech.
pub(crate) const SIMILARITY: f32 = 0.7;
/// Fewest heard units for a comparison.
pub(crate) const MIN_UNITS: usize = 3;

/// The text the lane spoke recently.
#[derive(Default)]
pub(crate) struct OwnSpeech {
    ended: VecDeque<(MonoTime, Vec<String>)>,
}

fn keys(text: &str) -> Vec<String> {
    units(text).into_iter().map(|u| u.key).collect()
}

impl OwnSpeech {
    /// A clause whose translated text was spoken (or queued to be) is over at `now`.
    pub(crate) fn remember(&mut self, now: MonoTime, text: &str) {
        let k = keys(text);
        if !k.is_empty() {
            self.ended.push_back((now, k));
        }
        self.forget(now);
    }

    fn forget(&mut self, now: MonoTime) {
        while self
            .ended
            .front()
            .is_some_and(|(at, _)| now.saturating_since(*at) > WINDOW)
        {
            self.ended.pop_front();
        }
    }

    /// Whether `heard` is the recent own speech again. `live` is the target text of the clauses
    /// that are still queued or playing.
    pub(crate) fn is_echo(&mut self, now: MonoTime, heard: &str, live: &[&str]) -> bool {
        self.forget(now);
        let heard = keys(heard);
        if heard.len() < MIN_UNITS {
            return false;
        }
        let mut own: Vec<&str> = Vec::new();
        for (_, k) in &self.ended {
            own.extend(k.iter().map(String::as_str));
        }
        let live: Vec<Vec<String>> = live.iter().map(|t| keys(t)).collect();
        for k in &live {
            own.extend(k.iter().map(String::as_str));
        }
        if own.is_empty() {
            return false;
        }
        let common = lcs(&heard, &own);
        common as f32 >= SIMILARITY * heard.len() as f32
    }
}

/// Length of the longest common subsequence.
fn lcs(a: &[String], b: &[&str]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diag = 0;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if x == y {
                diag + 1
            } else {
                row[j + 1].max(row[j])
            };
            diag = up;
        }
    }
    row[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> MonoTime {
        MonoTime::from_millis(s * 1000)
    }

    #[test]
    fn what_we_just_said_is_recognised_again_despite_small_errors() {
        let mut own = OwnSpeech::default();
        own.remember(
            at(10),
            "Thanks for joining the call. I would like to walk through the schedule.",
        );
        assert!(own.is_echo(at(14), "thanks for joining the call", &[]));
        assert!(own.is_echo(
            at(14),
            "Thanks for joining the cool I would like to walk through",
            &[]
        ));
        assert!(!own.is_echo(at(14), "Can you hear me now", &[]));
    }

    #[test]
    fn a_short_answer_is_never_taken_for_an_echo() {
        let mut own = OwnSpeech::default();
        own.remember(at(10), "Okay, that works for me.");
        assert!(!own.is_echo(at(11), "okay", &[]));
        assert!(!own.is_echo(at(11), "okay that", &[]));
    }

    #[test]
    fn the_window_ends_fifteen_seconds_after_the_clause_ended() {
        let mut own = OwnSpeech::default();
        own.remember(at(10), "see you at the station tomorrow morning");
        assert!(own.is_echo(at(24), "see you at the station", &[]));
        assert!(!own.is_echo(at(26), "see you at the station", &[]));
    }

    #[test]
    fn a_clause_that_is_still_queued_counts_as_own_speech() {
        let mut own = OwnSpeech::default();
        assert!(own.is_echo(
            at(1),
            "we should move the meeting",
            &["We should move the meeting to Thursday."]
        ));
        assert!(!own.is_echo(at(1), "we should move the meeting", &[]));
    }

    #[test]
    fn chinese_is_compared_per_character() {
        let mut own = OwnSpeech::default();
        own.remember(at(1), "我们明天在车站见面");
        assert!(own.is_echo(at(2), "我们明天在车站", &[]));
        assert!(!own.is_echo(at(2), "今天天气很好", &[]));
    }

    #[test]
    fn the_history_is_bounded_by_the_window() {
        let mut own = OwnSpeech::default();
        for i in 0..200 {
            own.remember(at(i), "a few words of speech here");
        }
        assert!(own.ended.len() <= 17, "{}", own.ended.len());
    }
}
