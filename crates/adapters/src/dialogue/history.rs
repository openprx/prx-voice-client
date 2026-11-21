//! The history of one conversation, bounded by turns and by tokens.

use std::collections::VecDeque;

use prxvc_ports::{DialogueMessage, DialogueRole};

use super::config::HistoryLimits;
use crate::mt::assess::estimate_tokens;

/// One kept message with its estimated size.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Entry {
    message: DialogueMessage,
    tokens: u32,
}

/// The messages of a conversation so far, oldest first.
///
/// A turn starts with a user message. The bounds drop the oldest turns whole, so the history
/// always starts with a user message and never with a reply to something that was dropped.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    entries: VecDeque<Entry>,
}

impl History {
    /// An empty history.
    pub fn new() -> Self {
        Self::default()
    }

    /// The messages kept.
    pub fn messages(&self) -> impl Iterator<Item = &DialogueMessage> {
        self.entries.iter().map(|e| &e.message)
    }

    /// How many user turns are kept.
    pub fn turns(&self) -> usize {
        self.entries
            .iter()
            .filter(|e| e.message.role == DialogueRole::User)
            .count()
    }

    /// Estimated tokens of everything kept.
    pub fn tokens(&self) -> u32 {
        self.entries.iter().map(|e| e.tokens).sum()
    }

    /// Append messages.
    pub fn push(&mut self, messages: impl IntoIterator<Item = DialogueMessage>) {
        for message in messages {
            let tokens = estimate_tokens(&message.text);
            self.entries.push_back(Entry { message, tokens });
        }
    }

    /// Drop the oldest turns until the history, together with a pending turn of `pending_turns`
    /// user turns and `pending_tokens` tokens that is not stored yet, is within `limits`. The
    /// pending turn itself is never dropped.
    pub fn trim(&mut self, limits: HistoryLimits, pending_turns: usize, pending_tokens: u32) {
        while !self.entries.is_empty()
            && (self.turns() + pending_turns > limits.max_turns
                || self.tokens().saturating_add(pending_tokens) > limits.max_tokens)
        {
            self.drop_oldest_turn();
        }
    }

    fn drop_oldest_turn(&mut self) {
        // The first entry is the start of the oldest turn; drop it and everything up to the next
        // user message.
        self.entries.pop_front();
        while self
            .entries
            .front()
            .is_some_and(|e| e.message.role != DialogueRole::User)
        {
            self.entries.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(h: &mut History, user: &str, reply: &str) {
        h.push([
            DialogueMessage::user(user),
            DialogueMessage {
                role: DialogueRole::Assistant,
                text: reply.into(),
            },
        ]);
    }

    fn limits(max_turns: usize, max_tokens: u32) -> HistoryLimits {
        HistoryLimits {
            max_turns,
            max_tokens,
        }
    }

    #[test]
    fn the_turn_bound_counts_the_pending_turn_and_drops_whole_turns_from_the_front() {
        let mut h = History::new();
        for i in 0..5 {
            turn(&mut h, &format!("q{i}"), &format!("a{i}"));
        }
        h.trim(limits(3, 10_000), 1, 1);
        assert_eq!(h.turns(), 2, "two kept plus the pending one makes three");
        let texts: Vec<_> = h.messages().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["q3", "a3", "q4", "a4"]);
    }

    #[test]
    fn the_token_bound_drops_the_oldest_turns_first() {
        let mut h = History::new();
        turn(&mut h, "alpha beta gamma", "delta epsilon zeta");
        turn(&mut h, "one two three", "four five six");
        let per_turn = h.tokens() / 2;
        h.trim(limits(100, per_turn + 1), 0, 0);
        let texts: Vec<_> = h.messages().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["one two three", "four five six"]);
    }

    #[test]
    fn a_pending_turn_larger_than_the_budget_empties_the_history_but_is_not_dropped() {
        let mut h = History::new();
        turn(&mut h, "old question", "old answer");
        h.trim(limits(12, 5), 1, 500);
        assert_eq!(h.turns(), 0);
        assert!(h.messages().next().is_none());
    }

    #[test]
    fn a_history_within_the_bounds_is_left_alone() {
        let mut h = History::new();
        turn(&mut h, "q", "a");
        let before = h.clone();
        h.trim(limits(12, 3_000), 1, 3);
        assert_eq!(h, before);
    }

    #[test]
    fn the_history_never_starts_with_a_reply() {
        let mut h = History::new();
        turn(&mut h, "q1", "a1");
        turn(&mut h, "q2", "a2");
        h.trim(limits(2, 10_000), 1, 1);
        assert_eq!(
            h.messages().next().map(|m| m.role),
            Some(DialogueRole::User)
        );
    }
}
