//! From recognised utterances to turns: drop, send, or merge.
//!
//! One spoken sentence can arrive as two utterances when the speaker pauses
//! mid-sentence. [`TurnPolicy`] decides, for every recognised utterance, what
//! the turn sent to the server is:
//!
//! - an utterance with no letters or digits, or with less speech than the
//!   minimum duration, is dropped (the duration rule does not apply to a
//!   continuation);
//! - an utterance that arrives within the merge window after the previous
//!   turn was sent, while the assistant has not started answering it, is a
//!   continuation: the turn is the previous text plus this one and the
//!   in-flight reply to the previous part must be cancelled first;
//! - otherwise it is a turn of its own.
//!
//! Time is passed in, so the policy is testable without a clock or audio.

use std::time::{Duration, Instant};

/// Parameters of [`TurnPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TurnPolicyConfig {
    /// How long after a turn was sent a new utterance still continues it.
    pub merge_window: Duration,
    /// Least speech an utterance needs to be a turn of its own.
    pub min_speech: Duration,
}

impl TurnPolicyConfig {
    /// Build from the millisecond values of the audio configuration.
    pub fn from_ms(merge_window_ms: u32, min_speech_ms: u32) -> Self {
        Self {
            merge_window: Duration::from_millis(u64::from(merge_window_ms)),
            min_speech: Duration::from_millis(u64::from(min_speech_ms)),
        }
    }
}

/// What to do with a recognised utterance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnAction {
    /// Not a turn: send nothing.
    Drop,
    /// Send `text` as a turn. With `cancel_previous`, the reply in flight to
    /// the previous turn must be cancelled before `text` (which then already
    /// contains the previous turn) is sent.
    Send {
        /// The text of the turn.
        text: String,
        /// Cancel the in-flight reply to the previous turn first.
        cancel_previous: bool,
    },
}

#[derive(Debug, Clone)]
struct Previous {
    text: String,
    sent_at: Instant,
}

/// Decides the turn for each recognised utterance.
#[derive(Debug, Clone)]
pub struct TurnPolicy {
    cfg: TurnPolicyConfig,
    previous: Option<Previous>,
}

impl TurnPolicy {
    /// A policy with no previous turn.
    pub fn new(cfg: TurnPolicyConfig) -> Self {
        Self { cfg, previous: None }
    }

    /// The assistant has started answering (or was cut off): a later
    /// utterance is a normal turn, never a continuation.
    pub fn assistant_started(&mut self) {
        self.previous = None;
    }

    /// Forget the previous turn (session ended, microphone gate reset).
    pub fn reset(&mut self) {
        self.previous = None;
    }

    /// Decide for an utterance recognised at `now` with `speech` of speech.
    pub fn on_utterance(&mut self, now: Instant, text: &str, speech: Duration) -> TurnAction {
        let text = text.trim();
        if !has_content(text) {
            return TurnAction::Drop;
        }
        let continuation = self
            .previous
            .as_ref()
            .filter(|p| now.saturating_duration_since(p.sent_at) <= self.cfg.merge_window);
        let (text, cancel_previous) = match continuation {
            Some(p) => (join_text(&p.text, text), true),
            None if speech < self.cfg.min_speech => return TurnAction::Drop,
            None => (text.to_owned(), false),
        };
        self.previous = Some(Previous { text: text.clone(), sent_at: now });
        TurnAction::Send { text, cancel_previous }
    }
}

/// Whether `text` has at least one letter or digit (not only punctuation).
pub fn has_content(text: &str) -> bool {
    text.chars().any(char::is_alphanumeric)
}

/// Concatenate two pieces of one sentence: a space only between two words of
/// a space-separated script, none next to CJK text or punctuation.
pub fn join_text(first: &str, second: &str) -> String {
    let (a, b) = (first.trim_end(), second.trim_start());
    let needs_space = match (a.chars().last(), b.chars().next()) {
        (Some(x), Some(y)) => x.is_ascii_alphanumeric() && y.is_ascii_alphanumeric()
            || (x.is_ascii_punctuation() && y.is_ascii_alphanumeric()),
        _ => false,
    };
    if needs_space { format!("{a} {b}") } else { format!("{a}{b}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEECH: Duration = Duration::from_millis(1_500);

    fn policy() -> (TurnPolicy, Instant) {
        (TurnPolicy::new(TurnPolicyConfig::from_ms(4_000, 280)), Instant::now())
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn two_utterances_inside_the_window_make_one_merged_turn_with_one_cancel() {
        let (mut p, t0) = policy();
        let first = p.on_utterance(t0, "你可以說音樂", SPEECH);
        assert_eq!(first, TurnAction::Send { text: "你可以說音樂".into(), cancel_previous: false });
        let second = p.on_utterance(at(t0, 3_000), "英語嗎?", SPEECH);
        assert_eq!(
            second,
            TurnAction::Send { text: "你可以說音樂英語嗎?".into(), cancel_previous: true }
        );
    }

    #[test]
    fn utterances_outside_the_window_are_two_turns() {
        let (mut p, t0) = policy();
        p.on_utterance(t0, "hello there", SPEECH);
        let second = p.on_utterance(at(t0, 4_001), "what time is it", SPEECH);
        assert_eq!(
            second,
            TurnAction::Send { text: "what time is it".into(), cancel_previous: false }
        );
    }

    #[test]
    fn a_third_piece_extends_the_merged_turn() {
        let (mut p, t0) = policy();
        p.on_utterance(t0, "a", SPEECH);
        p.on_utterance(at(t0, 3_000), "b", SPEECH);
        let third = p.on_utterance(at(t0, 6_000), "c", SPEECH);
        assert_eq!(third, TurnAction::Send { text: "a b c".into(), cancel_previous: true });
    }

    #[test]
    fn no_merge_once_the_assistant_started_speaking() {
        let (mut p, t0) = policy();
        p.on_utterance(t0, "first part", SPEECH);
        p.assistant_started();
        let second = p.on_utterance(at(t0, 1_000), "interrupting", SPEECH);
        assert_eq!(
            second,
            TurnAction::Send { text: "interrupting".into(), cancel_previous: false }
        );
    }

    #[test]
    fn empty_and_punctuation_only_utterances_are_dropped() {
        let (mut p, t0) = policy();
        for text in ["", "   ", "...", "。", "?! ,", "…"] {
            assert_eq!(p.on_utterance(t0, text, SPEECH), TurnAction::Drop, "{text:?}");
        }
    }

    #[test]
    fn short_speech_is_dropped_unless_it_continues_a_turn() {
        let (mut p, t0) = policy();
        let short = Duration::from_millis(200);
        assert_eq!(p.on_utterance(t0, "嗯", short), TurnAction::Drop);
        // A dropped utterance does not open a merge window.
        assert_eq!(p.on_utterance(at(t0, 500), "嗯", short), TurnAction::Drop);

        p.on_utterance(at(t0, 1_000), "我想問", SPEECH);
        let tail = p.on_utterance(at(t0, 2_000), "嗎", short);
        assert_eq!(tail, TurnAction::Send { text: "我想問嗎".into(), cancel_previous: true });
    }

    #[test]
    fn a_dropped_utterance_does_not_disturb_a_pending_merge() {
        let (mut p, t0) = policy();
        p.on_utterance(t0, "first", SPEECH);
        assert_eq!(p.on_utterance(at(t0, 1_000), "...", SPEECH), TurnAction::Drop);
        let next = p.on_utterance(at(t0, 2_000), "second", SPEECH);
        assert_eq!(next, TurnAction::Send { text: "first second".into(), cancel_previous: true });
    }

    #[test]
    fn join_text_spaces_only_between_latin_words() {
        assert_eq!(join_text("hello", "world"), "hello world");
        assert_eq!(join_text("你好", "世界"), "你好世界");
        assert_eq!(join_text("hello,", "world"), "hello, world");
        assert_eq!(join_text("你好 ", " world"), "你好world");
    }
}
