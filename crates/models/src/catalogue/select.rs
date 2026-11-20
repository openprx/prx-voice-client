//! Candidate ordering and recommendation (design 4.5). Single
//! responsibility: which models fit a role, key and machine, in which order.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotSelectable {
    /// Live role, but the entry does not stream.
    NotStreaming,
}

#[derive(Clone, Debug)]
pub struct Candidate<'a> {
    pub entry: &'a ModelEntry,
    pub badge: Option<Badge>,
    pub selectable: bool,
    pub reason: Option<NotSelectable>,
}

impl RecommendRule {
    pub(super) fn matches(&self, key: &LangKey, hw: &HardwareInfo) -> bool {
        if let Some(langs) = &self.languages
            && !langs.iter().any(|l| l.matches(key))
        {
            return false;
        }
        if self.min_memory_gb.is_some_and(|m| hw.memory_gb < m) {
            return false;
        }
        if self.max_memory_gb.is_some_and(|m| hw.memory_gb > m) {
            return false;
        }
        if self.os.as_ref().is_some_and(|o| !o.contains(&hw.os)) {
            return false;
        }
        if self.arch.as_ref().is_some_and(|a| !a.contains(&hw.arch)) {
            return false;
        }
        true
    }
}

impl Catalogue {
    /// Candidates for a role and key (design 4.5): matching role and
    /// language, supported runtime, not hidden. Order: entries with a matching
    /// rule first (by rank), then the rest by display name. Live-role entries
    /// that do not stream are listed but not selectable.
    pub fn candidates(
        &self,
        role: Role,
        key: &LangKey,
        hw: &HardwareInfo,
        runtimes: &[&str],
    ) -> Vec<Candidate<'_>> {
        let mut out: Vec<(Option<u32>, Candidate<'_>)> = self
            .models
            .iter()
            .filter(|m| m.role == role && !m.is_hidden())
            .filter(|m| m.languages.iter().any(|l| l.matches(key)))
            .filter(|m| runtimes.contains(&m.runtime.name.as_str()))
            .map(|m| {
                let rule = m.best_rule(key, hw);
                let not_streaming = role.is_live() && m.streaming == Streaming::None;
                (
                    rule.map(|r| r.rank),
                    Candidate {
                        entry: m,
                        badge: rule.map(|r| r.badge),
                        selectable: !not_streaming,
                        reason: not_streaming.then_some(NotSelectable::NotStreaming),
                    },
                )
            })
            .collect();
        out.sort_by(|(ra, a), (rb, b)| {
            let ka = (ra.is_none(), ra.unwrap_or(0), &a.entry.display_name);
            let kb = (rb.is_none(), rb.unwrap_or(0), &b.entry.display_name);
            ka.cmp(&kb)
        });
        out.into_iter().map(|(_, c)| c).collect()
    }

    /// Recommendation for a fresh install: the first selectable candidate with
    /// the `recommended` badge, else the first with `smallest`, else the first
    /// selectable candidate.
    pub fn recommended(
        &self,
        role: Role,
        key: &LangKey,
        hw: &HardwareInfo,
        runtimes: &[&str],
    ) -> Option<&ModelEntry> {
        let cands = self.candidates(role, key, hw, runtimes);
        let pick = |badge: Badge| {
            cands
                .iter()
                .find(|c| c.selectable && c.badge == Some(badge))
        };
        pick(Badge::Recommended)
            .or_else(|| pick(Badge::Smallest))
            .or_else(|| cands.iter().find(|c| c.selectable))
            .map(|c| c.entry)
    }
}
