//! The reliability rules of voice-cloning section 0.3 as code: the generation cap derived from the
//! text, and the runaway decision.
//!
//! The engine sometimes never emits its end-of-speech token. It then generates until its frame
//! limit (2048 frames, 163.84 s of audio) at normal speed and still reports success. So every call
//! gets a cap computed from the text, and a call whose audio reaches that cap is a failure, never
//! a success. The constants come from the measured speaking rates of the TTS benchmark
//! (`bench/tts/REPORT.md` section 5): about 0.25 s per Chinese character and 0.28 s per English
//! word, times a safety factor of 3, with headroom for a short text (a word or two is spoken with
//! a lead-in and a tail that the per-word rate does not cover). The factor and the headroom were
//! checked on 320 fragments of 1 to 10 words (`tests/qwentts_runaway_bench.rs`; the run prints the lengths).

use prxvc_media::Lang;

/// Measured speaking time of one Chinese (or other CJK) character, in seconds.
pub const SECS_PER_CJK_CHAR: f64 = 0.25;
/// Measured speaking time of one English word, in seconds.
pub const SECS_PER_WORD: f64 = 0.28;
/// Safety factor between the expected duration and the cap.
pub const SAFETY_FACTOR: f64 = 3.0;
/// Seconds added to the expected duration before the cap is taken: the cap is the larger of the
/// expected duration times [`SAFETY_FACTOR`] and the expected duration plus this headroom. It is
/// what keeps the cap of a text of one or two words above two seconds, where speech of that length ends (measured: the longest of 32 fragments of two words was 1.9 s, of three words 2.4 s).
pub const HEADROOM_SECS: f64 = 2.0;
/// Codec frames per second of audio (one frame is 80 ms).
pub const FRAMES_PER_SEC: f64 = 12.5;
/// The cap of attempt `attempt` (counting from 0) of a call with `retries` extra attempts allowed:
/// the text's cap, and for the last of two retries a doubled one, at least [`HOLD_CAP_FRAMES`]
/// but never past [`ENGINE_DEFAULT_CAP_FRAMES`].
pub fn attempt_cap_frames(cap_frames: u32, attempt: u32, retries: u32) -> u32 {
    if retries >= 2 && attempt >= 2 {
        cap_frames
            .saturating_mul(2)
            .max(HOLD_CAP_FRAMES.min(cap_frames.saturating_mul(4)))
            .min(ENGINE_DEFAULT_CAP_FRAMES)
    } else {
        cap_frames
    }
}

/// Samples in one codec frame at the engine's 24 kHz output rate.
pub const SAMPLES_PER_FRAME: usize = 1920;
/// The cap of a very short text, in frames (1.6 s).
pub const MIN_CAP_FRAMES: u32 = 20;
/// The longest cap, in frames (8 s), of a call that is held back and retried by
/// [`RunawayRetry::ShortOnce`]: a clause of up to about nine words. A longer clause streams as it
/// is produced and cannot be retried without playing its start twice.
pub const HOLD_CAP_FRAMES: u32 = 100;
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
    /// As `Once` for the first retry, with a third and last attempt at twice the cap (at least up
    /// to [`HOLD_CAP_FRAMES`]) before the clause is failed: speech that is simply slow (a rare name,
    /// 3.3 s for four words was measured) is cut by the cap exactly as a runaway is, and only a
    /// call that does not stop at twice the cap is one. Only for a text whose cap is at most [`HOLD_CAP_FRAMES`] (8 s: a clause of up
    /// to about nine words, the kind a call translates). Such a clause is held back until it ends
    /// inside its cap, so a runaway is never heard, and retried once with another seed; the delay
    /// is a fraction of a second. Longer clauses stream as `Off` does. The live call uses this.
    ShortOnce,
}

/// Which clauses [`RunawayRetry::ShortOnce`] holds back, by their length in speakable units
/// ([`speakable_units`]): words for English, characters for Chinese.
///
/// A clause with at most the limit of its language is held and retried on a runaway; a longer one
/// streams from its first chunk. A limit of 0 keeps the earlier rule for that language: hold every
/// clause whose cap is at most [`HOLD_CAP_FRAMES`] (up to about nine words or ten characters). A
/// clause whose cap is above [`HOLD_CAP_FRAMES`] is never held, whatever the limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HoldLimit {
    /// Most words of an English clause that is still held back (0: the cap rule).
    pub max_words: u32,
    /// Most characters of a Chinese clause that is still held back (0: the cap rule).
    pub max_chars: u32,
}

impl HoldLimit {
    /// The earlier rule: hold by the cap alone.
    pub const LEGACY: Self = Self {
        max_words: 0,
        max_chars: 0,
    };
    /// The largest limit that is honoured; a larger value is lowered to it.
    pub const MAX_UNITS: u32 = 20;

    /// Limits as configured, lowered to [`Self::MAX_UNITS`].
    pub fn clamped(max_words: u32, max_chars: u32) -> Self {
        Self {
            max_words: max_words.min(Self::MAX_UNITS),
            max_chars: max_chars.min(Self::MAX_UNITS),
        }
    }

    /// Whether a clause of `text` with the cap `cap_frames` is held back.
    pub fn holds(self, text: &str, lang: Lang, cap_frames: u32) -> bool {
        if cap_frames > HOLD_CAP_FRAMES {
            return false;
        }
        let limit = match lang {
            Lang::En => self.max_words,
            Lang::Zh => self.max_chars,
        };
        limit == 0 || speakable_units(text, lang) <= limit as usize
    }
}

impl Default for HoldLimit {
    fn default() -> Self {
        Self::LEGACY
    }
}

impl RunawayRetry {
    /// Extra attempts after a runaway of a clause `text` whose cap is `cap_frames`, with the hold
    /// rule `hold` (only [`Self::ShortOnce`] looks at it).
    pub fn extra_attempts_for(
        self,
        hold: HoldLimit,
        text: &str,
        lang: Lang,
        cap_frames: u32,
    ) -> u32 {
        match self {
            Self::ShortOnce if !hold.holds(text, lang, cap_frames) => 0,
            other => other.extra_attempts(cap_frames),
        }
    }

    /// Number of extra attempts after a runaway of a call whose cap is `cap_frames`.
    pub const fn extra_attempts(self, cap_frames: u32) -> u32 {
        match self {
            Self::Off => 0,
            Self::Once => 1,
            Self::ShortOnce => {
                if cap_frames <= HOLD_CAP_FRAMES {
                    2
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

/// How many speakable units `text` has, in the sense of [`expected_secs`]: a CJK character, a digit
/// or a word each count one. For logs and traces, which carry lengths and never text.
pub fn speakable_units(text: &str, lang: Lang) -> usize {
    let mut units = 0;
    let mut run = 0usize;
    let flush = |run: &mut usize, units: &mut usize| {
        if *run > 0 {
            *units += match lang {
                Lang::En => 1,
                Lang::Zh => run.div_ceil(2),
            };
            *run = 0;
        }
    };
    for c in text.chars() {
        if is_cjk(c) || c.is_numeric() {
            flush(&mut run, &mut units);
            units += 1;
        } else if c.is_alphabetic() || (run > 0 && matches!(c, '\'' | '\u{2019}' | '-')) {
            run += 1;
        } else {
            flush(&mut run, &mut units);
        }
    }
    flush(&mut run, &mut units);
    units
}

/// The generation cap of a call, in codec frames: the larger of the expected duration times
/// [`SAFETY_FACTOR`] and the expected duration plus [`HEADROOM_SECS`], at least [`MIN_CAP_FRAMES`].
pub fn cap_frames(text: &str, lang: Lang) -> u32 {
    let expected = expected_secs(text, lang);
    let frames = (expected * SAFETY_FACTOR).max(expected + HEADROOM_SECS) * FRAMES_PER_SEC;
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
    fn short_text_gets_headroom_and_the_floor_is_well_below_the_old_three_seconds() {
        // No speakable text at all: the headroom alone, never below the floor.
        assert_eq!(cap_frames("", Lang::En), 25);
        assert!(cap_frames("...", Lang::En) >= MIN_CAP_FRAMES);
        // One word: 0.28 s + 2 s of headroom = 2.28 s, 29 frames.
        assert_eq!(cap_frames("hi", Lang::En), 29);
        // Two words: 0.56 s + 2 s = 2.56 s, 32 frames; the cap used to be 40 frames (3.2 s).
        assert_eq!(cap_frames("thank you", Lang::En), 32);
        assert_eq!(cap_frames("\u{4f60}\u{597d}", Lang::Zh), 32);
        assert!(f64::from(cap_frames("thank you", Lang::En)) / FRAMES_PER_SEC < 2.6);
    }

    #[test]
    fn units_count_words_characters_and_digits() {
        assert_eq!(speakable_units("Don't  stop, ok?!", Lang::En), 3);
        assert_eq!(speakable_units("\u{6211}\u{4eec}\u{7528} Rust", Lang::Zh), 5);
        assert_eq!(speakable_units("2026", Lang::En), 4);
        assert_eq!(speakable_units("...", Lang::En), 0);
    }

    #[test]
    fn the_cap_follows_the_text_between_the_headroom_and_the_factor() {
        // Four words: 1.12 s; the factor (3.36 s) is larger than the headroom (3.12 s).
        assert_eq!(cap_frames("we should go there", Lang::En), 42);
        // Three words: 0.84 s; the headroom gives 2.84 s, the factor 2.52 s.
        assert_eq!(cap_frames("see you later", Lang::En), 36);
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
    fn the_short_text_policy_retries_every_text_that_is_held_back() {
        let short = cap_frames("Mm.", Lang::En);
        let mid = cap_frames("We should meet at the office tomorrow", Lang::En);
        let long = cap_frames("We should meet at the office tomorrow morning and then go home to eat", Lang::En);
        assert!(short < 40);
        assert!(mid > short && mid <= HOLD_CAP_FRAMES);
        assert!(long > HOLD_CAP_FRAMES);
        assert_eq!(RunawayRetry::ShortOnce.extra_attempts(mid), 2, "a retry and a last attempt at twice the cap");
        assert_eq!(attempt_cap_frames(mid, 0, 2), mid);
        assert_eq!(attempt_cap_frames(mid, 1, 2), mid);
        assert!(attempt_cap_frames(mid, 2, 2) >= HOLD_CAP_FRAMES.min(mid * 4) && attempt_cap_frames(mid, 2, 2) >= mid * 2);
        assert_eq!(attempt_cap_frames(mid, 2, 1), mid, "Once never doubles");
        assert_eq!(RunawayRetry::ShortOnce.extra_attempts(short), 2);
        assert_eq!(RunawayRetry::ShortOnce.extra_attempts(long), 0);
    }

    #[test]
    fn the_hold_limit_holds_by_units_and_the_legacy_value_by_the_cap() {
        let hold = HoldLimit::clamped(3, 4);
        let held = |t, l| RunawayRetry::ShortOnce.extra_attempts_for(hold, t, l, cap_frames(t, l));
        assert_eq!(held("see you later", Lang::En), 2, "three words are held");
        assert_eq!(held("we should go there", Lang::En), 0, "four words stream");
        assert_eq!(held("\u{4f60}\u{597d}\u{5417}\u{ff1f}", Lang::Zh), 2, "four characters");
        assert_eq!(held("\u{6211}\u{4eec}\u{53bb}\u{4e86}\u{5427}", Lang::Zh), 0, "five characters");
        // The legacy value is the cap rule: up to nine words are held, ten are not.
        let legacy = |t, l| {
            RunawayRetry::ShortOnce.extra_attempts_for(HoldLimit::LEGACY, t, l, cap_frames(t, l))
        };
        assert_eq!(legacy("we should go there", Lang::En), 2);
        assert_eq!(legacy("we should meet at the office tomorrow", Lang::En), 2);
        let long = "we should meet at the office tomorrow morning and then go home to eat";
        assert_eq!(legacy(long, Lang::En), 0);
        // A limit never holds a clause whose cap is past the hold limit.
        assert!(!HoldLimit::clamped(20, 20).holds(long, Lang::En, HOLD_CAP_FRAMES + 1));
        // Other policies ignore the limit.
        assert_eq!(RunawayRetry::Off.extra_attempts_for(hold, "hi", Lang::En, 29), 0);
        assert_eq!(RunawayRetry::Once.extra_attempts_for(hold, "see you later and go", Lang::En, 50), 1);
    }

    #[test]
    fn the_hold_limit_is_clamped() {
        assert_eq!(HoldLimit::clamped(999, 999), HoldLimit { max_words: 20, max_chars: 20 });
        assert_eq!(HoldLimit::clamped(0, 0), HoldLimit::LEGACY);
        assert_eq!(HoldLimit::default(), HoldLimit::LEGACY);
    }
}
