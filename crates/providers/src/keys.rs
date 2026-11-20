//! Typed role keys: the roles of the call pipeline and the language key each one is selected by.
//!
//! Configuration spells these as strings (`providers.active.mt."zh>en"`). The strings are parsed
//! here once; everything after this module matches on the enums (CS-2.2.2).

use std::fmt;
use std::str::FromStr;

use prxvc_media::{Direction, Lang};

/// A role of the call pipeline that a provider serves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CallRole {
    /// Voice activity detection.
    Vad,
    /// Streaming recognition.
    Asr,
    /// The refine pass of two-pass recognition.
    AsrRefine,
    /// Translation.
    Mt,
    /// Speech synthesis.
    Tts,
}

impl CallRole {
    /// The key of the role under `[providers.active]` and `[providers.fallback]`.
    pub const fn config_name(self) -> &'static str {
        match self {
            Self::Vad => "vad",
            Self::Asr => "asr",
            Self::AsrRefine => "asr_refine",
            Self::Mt => "mt",
            Self::Tts => "tts",
        }
    }
}

impl fmt::Display for CallRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.config_name())
    }
}

/// A role together with the language key it is resolved for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoleKey {
    /// The voice activity detector (one for the call; each direction opens its own engine).
    Vad,
    /// Streaming recognition of `Lang`.
    Asr(Lang),
    /// The refine pass for `Lang`.
    AsrRefine(Lang),
    /// Translation from `source` to `target`.
    Mt {
        /// Language spoken.
        source: Lang,
        /// Language produced.
        target: Lang,
    },
    /// Synthesis of `Lang`.
    Tts(Lang),
}

impl RoleKey {
    /// The role.
    pub const fn role(self) -> CallRole {
        match self {
            Self::Vad => CallRole::Vad,
            Self::Asr(_) => CallRole::Asr,
            Self::AsrRefine(_) => CallRole::AsrRefine,
            Self::Mt { .. } => CallRole::Mt,
            Self::Tts(_) => CallRole::Tts,
        }
    }

    /// The key as configuration spells it: `""`, `"zh"` or `"zh>en"`.
    pub fn config_key(self) -> String {
        match self {
            Self::Vad => String::new(),
            Self::Asr(l) | Self::AsrRefine(l) | Self::Tts(l) => l.code().to_owned(),
            Self::Mt { source, target } => format!("{source}>{target}"),
        }
    }
}

impl fmt::Display for RoleKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vad => f.write_str("vad"),
            other => write!(f, "{}[{}]", other.role(), other.config_key()),
        }
    }
}

/// The two languages of a call, resolved from `call.self_language` and `call.peer_language`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallLanguages {
    /// The local user's language.
    pub own: Lang,
    /// The remote party's language.
    pub peer: Lang,
}

impl CallLanguages {
    /// `(source, target)` of the direction: outgoing translates own into peer, incoming the reverse.
    pub const fn pair(self, direction: Direction) -> (Lang, Lang) {
        match direction {
            Direction::Outgoing => (self.own, self.peer),
            Direction::Incoming => (self.peer, self.own),
        }
    }
}

/// A configured language text that is not a language of the pipeline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadLanguage {
    /// The configuration key it came from.
    pub key: &'static str,
    /// The text found there.
    pub value: String,
}

/// Parse a configured language; empty text is `Ok(None)`.
pub(crate) fn parse_lang(key: &'static str, value: &str) -> Result<Option<Lang>, BadLanguage> {
    let value = value.trim();
    if value.is_empty() {
        return Ok(None);
    }
    Lang::from_str(value).map(Some).map_err(|_| BadLanguage {
        key,
        value: value.to_owned(),
    })
}

/// The other of the two pipeline languages.
pub(crate) const fn other(lang: Lang) -> Lang {
    match lang {
        Lang::Zh => Lang::En,
        Lang::En => Lang::Zh,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_keys_follow_the_configuration_spelling() {
        assert_eq!(RoleKey::Vad.config_key(), "");
        assert_eq!(RoleKey::Asr(Lang::Zh).config_key(), "zh");
        assert_eq!(
            RoleKey::Mt {
                source: Lang::Zh,
                target: Lang::En
            }
            .config_key(),
            "zh>en"
        );
        assert_eq!(
            RoleKey::AsrRefine(Lang::En).role().config_name(),
            "asr_refine"
        );
    }

    #[test]
    fn directions_swap_the_languages() {
        let l = CallLanguages {
            own: Lang::Zh,
            peer: Lang::En,
        };
        assert_eq!(l.pair(Direction::Outgoing), (Lang::Zh, Lang::En));
        assert_eq!(l.pair(Direction::Incoming), (Lang::En, Lang::Zh));
    }

    #[test]
    fn a_language_text_is_parsed_once() {
        assert_eq!(parse_lang("k", ""), Ok(None));
        assert_eq!(parse_lang("k", " zh-CN "), Ok(Some(Lang::Zh)));
        assert!(parse_lang("k", "fr").is_err());
    }
}
