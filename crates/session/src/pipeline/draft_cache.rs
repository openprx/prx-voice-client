//! Draft cache: a small bounded map from the exact source text of a translation request to its finished translation, and the stream that replays one (incremental-retranslation 4.4, 5.3).

use std::collections::VecDeque;
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_ports::{CancellationToken, MtDelta, MtError, MtFlag, MtStream};

use crate::commit_policy::RetranslateParams;

/// Entries kept; the oldest is evicted beyond it.
pub(crate) const CAPACITY: usize = 32;

/// A finished translation: its text and the quality flags it ended with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cached {
    pub(crate) text: String,
    pub(crate) flags: Vec<MtFlag>,
}

/// Translations by exact source text, shared by the threads that make them and the one that speaks them.
/// The lock is held only to copy an entry in or out.
#[derive(Debug, Default)]
pub(crate) struct DraftCache {
    entries: Mutex<VecDeque<(String, Cached)>>,
}

impl DraftCache {
    /// The cache a direction uses: only when its clause translations continue the sentence (an
    /// engine without context). A draft made without context is not the same request for an
    /// engine that takes context.
    pub(crate) fn for_params(p: &RetranslateParams) -> Option<Arc<Self>> {
        p.continuation.then(|| Arc::new(Self::default()))
    }

    /// The translation of exactly `source`, if it is held.
    pub(crate) fn get(&self, source: &str) -> Option<Cached> {
        self.entries
            .lock()
            .iter()
            .find(|(s, _)| s == source)
            .map(|(_, c)| c.clone())
    }

    /// Hold `cached` as the translation of `source`, replacing an older one; the oldest entry goes
    /// when the cache is full.
    pub(crate) fn put(&self, source: &str, cached: Cached) {
        let mut entries = self.entries.lock();
        entries.retain(|(s, _)| s != source);
        entries.push_back((source.to_owned(), cached));
        while entries.len() > CAPACITY {
            entries.pop_front();
        }
    }

    /// Entries held.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.lock().len()
    }
}

/// The stream of a cached translation: its text as one delta, then `Done`, never flagged as suspect.
pub(crate) struct CachedStream {
    entry: Cached,
    sent: Sent,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sent {
    Nothing,
    Text,
    Done,
}

impl CachedStream {
    pub(crate) fn new(entry: Cached) -> Self {
        Self {
            entry,
            sent: Sent::Nothing,
        }
    }
}

impl MtStream for CachedStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<MtDelta, MtError>> {
        if cancel.is_cancelled() {
            return Some(Err(MtError::Cancelled));
        }
        match self.sent {
            Sent::Nothing => {
                self.sent = Sent::Text;
                Some(Ok(MtDelta::Text(self.entry.text.clone())))
            }
            Sent::Text => {
                self.sent = Sent::Done;
                Some(Ok(MtDelta::Done {
                    prompt_tokens: 0,
                    completion_tokens: 0,
                }))
            }
            Sent::Done => None,
        }
    }

    fn flags(&self) -> Vec<MtFlag> {
        if self.sent == Sent::Done {
            self.entry.flags.clone()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(text: &str) -> Cached {
        Cached {
            text: text.to_owned(),
            flags: Vec::new(),
        }
    }

    #[test]
    fn a_hit_is_one_text_then_done_with_the_flags_of_the_entry() {
        let mut stream = CachedStream::new(Cached {
            text: "hello there".to_owned(),
            flags: vec![MtFlag::TooLong],
        });
        let token = CancellationToken::new();
        assert!(stream.flags().is_empty(), "no flags before Done");
        assert!(matches!(
            stream.next(&token),
            Some(Ok(MtDelta::Text(t))) if t == "hello there"
        ));
        assert!(matches!(
            stream.next(&token),
            Some(Ok(MtDelta::Done { .. }))
        ));
        assert_eq!(stream.flags(), vec![MtFlag::TooLong]);
        assert!(stream.next(&token).is_none());
        assert_eq!(stream.suspect(), None);
    }

    #[test]
    fn a_cancelled_token_ends_the_stream_with_a_cancellation() {
        let mut stream = CachedStream::new(entry("x"));
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(stream.next(&token), Some(Err(MtError::Cancelled))));
    }

    #[test]
    fn the_key_is_the_exact_source_text() {
        let cache = DraftCache::default();
        cache.put("we should meet", entry("a"));
        assert_eq!(cache.get("we should meet"), Some(entry("a")));
        assert_eq!(cache.get("We should meet"), None);
        assert_eq!(cache.get("we should meet "), None);
    }

    #[test]
    fn a_second_put_replaces_the_entry_and_keeps_it_young() {
        let cache = DraftCache::default();
        cache.put("a", entry("1"));
        cache.put("b", entry("2"));
        cache.put("a", entry("3"));
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.get("a"), Some(entry("3")));
        for i in 0..CAPACITY - 1 {
            cache.put(&format!("k{i}"), entry("x"));
        }
        assert_eq!(cache.get("b"), None, "the oldest went first");
        assert!(cache.get("a").is_some());
    }

    #[test]
    fn the_oldest_entry_is_evicted_beyond_the_capacity() {
        let cache = DraftCache::default();
        for i in 0..=CAPACITY {
            cache.put(&format!("k{i}"), entry("x"));
        }
        assert_eq!(cache.len(), CAPACITY);
        assert_eq!(cache.get("k0"), None);
        assert!(cache.get("k1").is_some());
        assert!(cache.get(&format!("k{CAPACITY}")).is_some());
    }

    #[test]
    fn there_is_no_cache_unless_clause_translations_continue_the_sentence() {
        let off = RetranslateParams::disabled();
        assert!(DraftCache::for_params(&off).is_none());
        let with_context = RetranslateParams {
            enabled: true,
            continuation: false,
            ..off
        };
        assert!(DraftCache::for_params(&with_context).is_none());
        let without_context = RetranslateParams {
            enabled: true,
            continuation: true,
            ..off
        };
        assert!(DraftCache::for_params(&without_context).is_some());
    }
}
