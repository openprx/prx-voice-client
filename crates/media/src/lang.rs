//! The languages the call pipeline translates between, parsed once from configuration text.

use std::fmt;
use std::str::FromStr;

/// A language of the pipeline.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {
    /// Chinese.
    Zh,
    /// English.
    En,
}

/// The text is not a language the pipeline knows.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("unknown language `{0}`")]
pub struct UnknownLang(pub String);

impl Lang {
    /// The two-letter code used in provider requests.
    pub const fn code(self) -> &'static str {
        match self {
            Self::Zh => "zh",
            Self::En => "en",
        }
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl FromStr for Lang {
    type Err = UnknownLang;

    /// Accepts a bare code (`zh`) or a locale (`zh-CN`, `en_US`), in any case.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let primary = s
            .split(['-', '_'])
            .next()
            .unwrap_or_default()
            .to_ascii_lowercase();
        match primary.as_str() {
            "zh" => Ok(Self::Zh),
            "en" => Ok(Self::En),
            _ => Err(UnknownLang(s.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_and_locales_parse() {
        assert_eq!("zh".parse(), Ok(Lang::Zh));
        assert_eq!("zh-CN".parse(), Ok(Lang::Zh));
        assert_eq!("EN_us".parse(), Ok(Lang::En));
        assert_eq!(Lang::Zh.to_string(), "zh");
    }

    #[test]
    fn unknown_and_empty_text_is_an_error() {
        assert_eq!("fr".parse::<Lang>(), Err(UnknownLang("fr".to_owned())));
        assert!("".parse::<Lang>().is_err());
    }
}
