//! Offline pass-through translator for tests and no-network demos.

use prxvc_session::adapter::{AdapterError, Translator};

/// A translator that does not translate: it returns the input tagged with the
/// target language, e.g. `"hello"` → `"[zh] hello"`. Useful to exercise the
/// pipeline without a model.
#[derive(Debug, Default, Clone, Copy)]
pub struct EchoTranslator;

impl Translator for EchoTranslator {
    fn translate(
        &mut self,
        text: &str,
        _source_lang: &str,
        target_lang: &str,
    ) -> Result<String, AdapterError> {
        Ok(format!("[{target_lang}] {text}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn echoes_with_target_tag() {
        let mut t = EchoTranslator;
        assert_eq!(t.translate("hello", "en", "zh").unwrap(), "[zh] hello");
    }
}
