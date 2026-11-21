//! Capability descriptor of a provider and the checks that decide whether it may serve the live path.

use std::fmt;

use prxvc_media::{Lang, SampleRate};

/// The configured identifier of a provider (`providers.active.*`).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderId(String);

impl ProviderId {
    /// An identifier from its configured text.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The identifier text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// How a provider produces its output (CS-2.1.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StreamingKind {
    /// Output is produced incrementally while input is still arriving or
    /// being processed.
    Native,
    /// Partial results are produced while input arrives by re-decoding the
    /// growing input of the open segment on a cadence (a model that is not
    /// incremental). Eligible as a live recogniser; it is not true incremental
    /// streaming, so [`ProviderDescriptor::require_native`] refuses it.
    Redecode,
    /// Output is produced per bounded segment (a whole clause in, a whole
    /// result out, no partial results while audio arrives): usable for a
    /// refine pass, not as a live stream.
    Segmented,
    /// The provider returns only a whole utterance. Never used on the live path.
    None,
}

/// Where a provider sends data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Egress {
    /// Nothing leaves the machine.
    Local,
    /// Text (transcripts, translations) is sent to `host`.
    Text {
        /// The remote host.
        host: String,
    },
    /// Audio is sent to `host`.
    Audio {
        /// The remote host.
        host: String,
    },
}

/// What a provider is and what it supports.
///
/// `languages` and `sample_rates` list what the provider accepts; an empty
/// list means the property does not apply (a VAD has no language) and every
/// value passes the corresponding check. For a recogniser `sample_rates` are
/// the input rates, for a synthesiser the output rates.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProviderDescriptor {
    /// The configured provider id.
    pub id: ProviderId,
    /// The model it runs, for display and traces.
    pub model: String,
    /// Whether and how it streams.
    pub streaming: StreamingKind,
    /// Whether the partial hypotheses of a recogniser carry a stable prefix
    /// that later hypotheses of the segment never retract (the final is not
    /// bound by it; see `AsrHypothesis::contradicts_stable`).
    pub stable_prefix: bool,
    /// Supported languages; empty when not language specific.
    pub languages: Vec<Lang>,
    /// Supported sample rates; empty when any rate is handled.
    pub sample_rates: Vec<SampleRate>,
    /// Where data goes.
    pub egress: Egress,
    /// Translation only: whether the provider uses the previous clauses of the conversation
    /// (`MtRequest::context`). `true` unless the provider declares otherwise.
    pub supports_context: bool,
    /// Translation only: whether the provider follows custom instructions and required term
    /// translations (`MtRequest::glossary`, `MtRequest::strict_language`). `true` unless the
    /// provider declares otherwise.
    pub supports_instructions: bool,
}

/// A provider does not meet a requirement.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum CapabilityError {
    /// The provider cannot stream and may not serve a live role.
    #[error("provider `{provider}` does not stream and cannot be used on the live path")]
    NotStreaming {
        /// The refused provider.
        provider: ProviderId,
    },
    /// The provider does not support a language.
    #[error("provider `{provider}` does not support language `{lang}`")]
    LanguageUnsupported {
        /// The refused provider.
        provider: ProviderId,
        /// The language it lacks.
        lang: Lang,
    },
    /// The provider does not support a sample rate.
    #[error("provider `{provider}` does not support {rate}")]
    SampleRateUnsupported {
        /// The refused provider.
        provider: ProviderId,
        /// The rate it lacks.
        rate: SampleRate,
    },
}

impl ProviderDescriptor {
    /// A local, language-independent, rate-independent provider.
    pub fn new(id: impl Into<String>, model: impl Into<String>, streaming: StreamingKind) -> Self {
        Self {
            id: ProviderId::new(id),
            model: model.into(),
            streaming,
            stable_prefix: false,
            languages: Vec::new(),
            sample_rates: Vec::new(),
            egress: Egress::Local,
            supports_context: true,
            supports_instructions: true,
        }
    }

    /// Declare whether a translation provider uses conversation context and follows custom
    /// instructions and required terms.
    #[must_use]
    pub fn with_text_guidance(mut self, context: bool, instructions: bool) -> Self {
        self.supports_context = context;
        self.supports_instructions = instructions;
        self
    }

    /// Declare that partial hypotheses carry a stable prefix.
    #[must_use]
    pub fn with_stable_prefix(mut self, stable_prefix: bool) -> Self {
        self.stable_prefix = stable_prefix;
        self
    }

    /// Set the supported languages.
    #[must_use]
    pub fn with_languages(mut self, languages: impl IntoIterator<Item = Lang>) -> Self {
        self.languages = languages.into_iter().collect();
        self
    }

    /// Set the supported sample rates.
    #[must_use]
    pub fn with_sample_rates(mut self, rates: impl IntoIterator<Item = SampleRate>) -> Self {
        self.sample_rates = rates.into_iter().collect();
        self
    }

    /// Set where the provider sends data.
    #[must_use]
    pub fn with_egress(mut self, egress: Egress) -> Self {
        self.egress = egress;
        self
    }

    /// The registry's gate for live roles: refuses a provider that cannot stream.
    ///
    /// `Segmented` and `Redecode` pass here because such a provider may serve
    /// a role that works per clause; a role that needs a live stream checks
    /// [`ProviderDescriptor::require_native`] (true incremental streaming) or
    /// [`ProviderDescriptor::require_partials`] (partial results while input
    /// arrives, however they are produced) itself.
    pub fn require_live(&self) -> Result<(), CapabilityError> {
        match self.streaming {
            StreamingKind::None => Err(self.not_streaming()),
            StreamingKind::Native | StreamingKind::Redecode | StreamingKind::Segmented => Ok(()),
        }
    }

    /// The gate for roles that must stream incrementally while input arrives
    /// (MT, TTS). Means true incremental streaming: a re-decoding recogniser
    /// does not pass.
    pub fn require_native(&self) -> Result<(), CapabilityError> {
        match self.streaming {
            StreamingKind::Native => Ok(()),
            StreamingKind::Redecode | StreamingKind::Segmented | StreamingKind::None => {
                Err(self.not_streaming())
            }
        }
    }

    /// The gate for the streaming recognition role: partial results are
    /// produced while audio arrives, incrementally or by re-decoding. A
    /// provider that only returns a final per segment does not pass (it may
    /// still serve the refine role, [`ProviderDescriptor::require_live`]).
    pub fn require_partials(&self) -> Result<(), CapabilityError> {
        if self.produces_partials() {
            Ok(())
        } else {
            Err(self.not_streaming())
        }
    }

    /// Whether partial results are produced while input arrives.
    pub fn produces_partials(&self) -> bool {
        matches!(
            self.streaming,
            StreamingKind::Native | StreamingKind::Redecode
        )
    }

    fn not_streaming(&self) -> CapabilityError {
        CapabilityError::NotStreaming {
            provider: self.id.clone(),
        }
    }

    /// Whether `lang` is supported.
    pub fn supports_lang(&self, lang: Lang) -> bool {
        self.languages.is_empty() || self.languages.contains(&lang)
    }

    /// Check that `lang` is supported.
    pub fn require_lang(&self, lang: Lang) -> Result<(), CapabilityError> {
        if self.supports_lang(lang) {
            Ok(())
        } else {
            Err(CapabilityError::LanguageUnsupported {
                provider: self.id.clone(),
                lang,
            })
        }
    }

    /// Check that both languages of a translation pair are supported.
    pub fn require_pair(&self, source: Lang, target: Lang) -> Result<(), CapabilityError> {
        self.require_lang(source)?;
        self.require_lang(target)
    }

    /// Whether `rate` is supported.
    pub fn supports_rate(&self, rate: SampleRate) -> bool {
        self.sample_rates.is_empty() || self.sample_rates.contains(&rate)
    }

    /// Check that `rate` is supported.
    pub fn require_rate(&self, rate: SampleRate) -> Result<(), CapabilityError> {
        if self.supports_rate(rate) {
            Ok(())
        } else {
            Err(CapabilityError::SampleRateUnsupported {
                provider: self.id.clone(),
                rate,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_non_streaming_provider_is_refused_for_the_live_path() {
        let d = ProviderDescriptor::new("batch-asr", "m", StreamingKind::None);
        assert_eq!(
            d.require_live(),
            Err(CapabilityError::NotStreaming {
                provider: ProviderId::new("batch-asr")
            })
        );
        assert!(d.require_native().is_err());
    }

    #[test]
    fn a_segmented_provider_is_live_capable_but_not_native() {
        let d = ProviderDescriptor::new("refine", "m", StreamingKind::Segmented);
        assert!(d.require_live().is_ok());
        assert!(d.require_native().is_err());
        assert!(
            ProviderDescriptor::new("s", "m", StreamingKind::Native)
                .require_native()
                .is_ok()
        );
    }

    #[test]
    fn a_redecoding_provider_has_partials_but_is_not_native() {
        let d =
            ProviderDescriptor::new("qwen", "m", StreamingKind::Redecode).with_stable_prefix(true);
        assert!(d.require_live().is_ok());
        assert!(d.require_partials().is_ok());
        assert!(d.produces_partials());
        assert!(d.stable_prefix);
        assert!(d.require_native().is_err(), "native means incremental");
    }

    #[test]
    fn partials_gate_refuses_finals_only_and_whole_utterance_providers() {
        for kind in [StreamingKind::Segmented, StreamingKind::None] {
            let d = ProviderDescriptor::new("p", "m", kind);
            assert!(d.require_partials().is_err());
            assert!(!d.produces_partials());
            assert!(!d.stable_prefix, "the flag is off unless declared");
        }
        assert!(
            ProviderDescriptor::new("n", "m", StreamingKind::Native)
                .require_partials()
                .is_ok()
        );
    }

    #[test]
    fn languages_and_rates_are_checked_against_the_lists() {
        let d = ProviderDescriptor::new("tts", "m", StreamingKind::Native)
            .with_languages([Lang::En])
            .with_sample_rates([SampleRate::HZ_24000]);
        assert!(d.require_lang(Lang::En).is_ok());
        assert!(matches!(
            d.require_lang(Lang::Zh),
            Err(CapabilityError::LanguageUnsupported { lang: Lang::Zh, .. })
        ));
        assert!(d.require_pair(Lang::En, Lang::Zh).is_err());
        assert!(d.require_rate(SampleRate::HZ_24000).is_ok());
        assert!(d.require_rate(SampleRate::HZ_16000).is_err());
    }

    #[test]
    fn empty_lists_mean_the_property_does_not_apply() {
        let d = ProviderDescriptor::new("vad", "m", StreamingKind::Native);
        assert!(d.require_pair(Lang::Zh, Lang::En).is_ok());
        assert!(d.require_rate(SampleRate::HZ_48000).is_ok());
        assert_eq!(d.egress, Egress::Local);
    }
}
