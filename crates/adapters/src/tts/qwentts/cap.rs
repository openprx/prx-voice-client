//! The reliability rules of voice-cloning section 0.3 as code: the generation cap derived from the
//! text, and the runaway decision.
//!
//! The engine sometimes never emits its end-of-speech token. It then generates until its frame
//! limit (2048 frames, 163.84 s of audio) at normal speed and still reports success. So every call
//! gets a cap computed from the text, and a call whose audio reaches that cap is a failure, never
//! a success. The constants come from the measured speaking rates of the TTS benchmark
//! (`bench/tts/REPORT.md` section 5): about 0.25 s per Chinese character and 0.28 s per English
//! word, times a safety factor of 3 [E, to be checked against the owner's real speech].

use prxvc_media::Lang;

/// Measured speaking time of one Chinese (or other CJK) character, in seconds.
pub const SECS_PER_CJK_CHAR: f64 = 0.25;
/// Measured speaking time of one English word, in seconds.
pub const SECS_PER_WORD: f64 = 0.28;
/// Safety factor between the expected duration and the cap.
pub const SAFETY_FACTOR: f64 = 3.0;
/// Codec frames per second of audio (one frame is 80 ms).
pub const FRAMES_PER_SEC: f64 = 12.5;
/// Samples in one codec frame at the engine's 24 kHz output rate.
pub const SAMPLES_PER_FRAME: usize = 1920;
/// The cap of a very short text, in frames (3.2 s).
pub const MIN_CAP_FRAMES: u32 = 40;
/// The engine's own default limit, which is never used: every call sets its own cap.
pub const ENGINE_DEFAULT_CAP_FRAMES: u32 = 2048;
/// A call whose audio comes within this many frames of its cap counts as having reached it.
pub const CAP_TOLERANCE_FRAMES: u32 = 1;
/// The longest text accepted for one call, in characters. A clause is far shorter; the limit keeps
/// the cap, and with it the per-request queue, bounded.
pub const MAX_TEXT_CHARS: usize = 1000;

/// What to do when a call runs away.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunawayRetry {
    /// Report the runaway as a failed request (the design's behaviour: the fallback layer decides).
    /// Audio streams out as it is produced.
    #[default]
    Off,
    /// Retry once with a different seed. Audio of an attempt is held back until the attempt ends
    /// inside its cap, because a retry after streaming would replay the clause from its start; the
    /// first chunk therefore arrives when the attempt ends. For uses that are not live (enrolment
    /// samples, long-form synthesis).
    Once,
    /// As `Once`, but only for a text short enough to sit at the minimum cap ([`MIN_CAP_FRAMES`],
    /// 3.2 s: a clause of about one second of speech, the kind on which the engine runs away, such
    /// as "Mm."). Such a clause is held back until it ends inside its cap, so a runaway is never
    /// heard, and retried once with another seed; the delay is at most a fraction of a second.
    /// Longer clauses stream as `Off` does. The live call uses this.
    ShortOnce,
}

impl RunawayRetry {
    /// Number of extra attempts after a runaway of a call whose cap is `cap_frames`.
    pub const fn extra_attempts(self, cap_frames: u32) -> u32 {
        match self {
            Self::Off => 0,
            Self::Once => 1,
            Self::ShortOnce => {
                if cap_frames <= MIN_CAP_FRAMES {
                    1
                } else {
                    0
                }
            }
        }
    }
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF   // kana
        | 0x3400..=0x4DBF // CJK extension A
        | 0x4E00..=0x9FFF // CJK unified ideographs
        | 0xAC00..=0xD7AF // hangul
        | 0xF900..=0xFAFF // compatibility ideographs
        | 0x20000..=0x2A6DF // CJK extension B
    )
}

/// Expected speaking time of `text`, in seconds, from the measured rates.
///
/// A CJK character and a digit count as 0.25 s each. A run of letters is one English word of 0.28 s;
/// in Chinese text, where Latin letters are mostly acronyms spelled out, it counts one word per two
/// letters. Punctuation and white space count nothing.
pub fn expected_secs(text: &str, lang: Lang) -> f64 {
    let mut secs = 0.0;
    let mut run = 0usize;
    let flush = |run: &mut usize, secs: &mut f64| {
        if *run > 0 {
            let words = match lang {
                Lang::En => 1,
                Lang::Zh => run.div_ceil(2),
            };
            *secs += words as f64 * SECS_PER_WORD;
            *run = 0;
        }
    };
    for c in text.chars() {
        if is_cjk(c) || c.is_numeric() {
            flush(&mut run, &mut secs);
            secs += SECS_PER_CJK_CHAR;
        } else if c.is_alphabetic() || (run > 0 && matches!(c, '\'' | '\u{2019}' | '-')) {
            run += 1;
        } else {
            flush(&mut run, &mut secs);
        }
    }
    flush(&mut run, &mut secs);
    secs
}

/// The generation cap of a call, in codec frames: the expected duration times [`SAFETY_FACTOR`],
/// at least [`MIN_CAP_FRAMES`].
pub fn cap_frames(text: &str, lang: Lang) -> u32 {
    let frames = expected_secs(text, lang) * SAFETY_FACTOR * FRAMES_PER_SEC;
    // The small offset keeps products like 10 * 0.28 * 3 * 12.5 = 105.00000000000001 at 105.
    let frames = (frames - 1e-9).ceil().max(0.0);
    let frames = if frames >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        frames as u32
    };
    frames.max(MIN_CAP_FRAMES)
}

/// Codec frames covered by `samples` samples, rounded up.
pub fn frames_of(samples: usize) -> u32 {
    u32::try_from(samples.div_ceil(SAMPLES_PER_FRAME)).unwrap_or(u32::MAX)
}

/// Whether `produced_samples` of audio mean the call has run away: it reached its cap (the engine
/// stops there and still reports success) or passed it.
pub fn is_runaway(produced_samples: usize, cap_frames: u32) -> bool {
    frames_of(produced_samples).saturating_add(CAP_TOLERANCE_FRAMES) >= cap_frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_gets_the_floor() {
        assert_eq!(cap_frames("hi", Lang::En), MIN_CAP_FRAMES);
        assert_eq!(cap_frames("", Lang::En), MIN_CAP_FRAMES);
        assert_eq!(cap_frames("\u{4f60}\u{597d}", Lang::Zh), MIN_CAP_FRAMES);
    }

    #[test]
    fn english_cap_is_words_times_rate_times_factor_in_frames() {
        let ten = "one two three four five six seven eight nine ten";
        // 10 words * 0.28 s * 3 * 12.5 frames per second.
        assert_eq!(cap_frames(ten, Lang::En), 105);
        assert!((expected_secs(ten, Lang::En) - 2.8).abs() < 1e-9);
    }

    #[test]
    fn chinese_cap_counts_characters() {
        let twenty = "\u{6211}".repeat(20);
        assert!((expected_secs(&twenty, Lang::Zh) - 5.0).abs() < 1e-9);
        // 20 * 0.25 * 3 * 12.5 = 187.5
        assert_eq!(cap_frames(&twenty, Lang::Zh), 188);
    }

    #[test]
    fn punctuation_and_spaces_cost_nothing_and_apostrophes_stay_in_a_word() {
        assert!((expected_secs("Don't  stop, ok?!", Lang::En) - 3.0 * 0.28).abs() < 1e-9);
        assert_eq!(expected_secs("...  ", Lang::En), 0.0);
    }

    #[test]
    fn digits_count_per_digit_and_latin_runs_in_chinese_count_per_two_letters() {
        assert!((expected_secs("2026", Lang::En) - 1.0).abs() < 1e-9);
        // "GPU" in Chinese text: two words; in English: one.
        assert!((expected_secs("GPU", Lang::Zh) - 2.0 * 0.28).abs() < 1e-9);
        assert!((expected_secs("GPU", Lang::En) - 0.28).abs() < 1e-9);
    }

    #[test]
    fn mixed_text_adds_both_rates() {
        let t = "\u{6211}\u{4eec}\u{7528} Rust \u{5199}";
        let want = 4.0 * SECS_PER_CJK_CHAR + SECS_PER_WORD * 2.0;
        assert!((expected_secs(t, Lang::Zh) - want).abs() < 1e-9);
    }

    #[test]
    fn the_cap_is_monotonic_in_the_text_length_and_never_the_engine_default() {
        let mut last = 0;
        for n in 1..300 {
            let text = "word ".repeat(n);
            let c = cap_frames(&text, Lang::En);
            assert!(c >= last);
            last = c;
        }
        assert_ne!(
            cap_frames("hello there", Lang::En),
            ENGINE_DEFAULT_CAP_FRAMES
        );
    }

    #[test]
    fn runaway_means_reaching_the_cap() {
        let cap = 100;
        assert!(!is_runaway(0, cap));
        assert!(!is_runaway(97 * SAMPLES_PER_FRAME, cap));
        assert!(
            is_runaway(99 * SAMPLES_PER_FRAME + 1, cap),
            "within tolerance of the cap"
        );
        assert!(is_runaway(100 * SAMPLES_PER_FRAME, cap));
        assert!(is_runaway(500 * SAMPLES_PER_FRAME, cap));
    }

    #[test]
    fn retry_policy_defaults_to_reporting_the_failure() {
        assert_eq!(RunawayRetry::default(), RunawayRetry::Off);
        assert_eq!(RunawayRetry::Off.extra_attempts(MIN_CAP_FRAMES), 0);
        assert_eq!(RunawayRetry::Once.extra_attempts(500), 1);
    }

    #[test]
    fn the_short_text_policy_retries_only_texts_at_the_minimum_cap() {
        let short = cap_frames("Mm.", Lang::En);
        let long = cap_frames("We should meet at the office tomorrow morning and then go home", Lang::En);
        assert_eq!(short, MIN_CAP_FRAMES);
        assert!(long > MIN_CAP_FRAMES);
        assert_eq!(RunawayRetry::ShortOnce.extra_attempts(short), 1);
        assert_eq!(RunawayRetry::ShortOnce.extra_attempts(long), 0);
    }
}
