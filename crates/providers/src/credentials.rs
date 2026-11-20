//! Credential references and the read side of the credential store.
//!
//! Configuration holds only a reference (`keychain:<id>`, `env:<VAR>` or empty; UX 4.8.4). The
//! registry reads the secret once, when it resolves an entry, through [`CredentialReader`]. The
//! keychain-backed reader is a later package; the default here reads environment references and
//! refuses keychain ones, so a missing store is a typed problem and never a silent empty key.

use std::fmt;

/// A parsed credential reference.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialRef {
    /// No key is needed or stored.
    None,
    /// `keychain:<id>`.
    Keychain(String),
    /// `env:<VAR>`.
    Env(String),
}

/// The reference text is not one of the three forms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadCredentialRef(pub String);

impl CredentialRef {
    /// Parse the text of `providers.entries.credential`.
    pub fn parse(text: &str) -> Result<Self, BadCredentialRef> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Self::None);
        }
        match text.split_once(':') {
            Some(("keychain", id)) if !id.is_empty() => Ok(Self::Keychain(id.to_owned())),
            Some(("env", var)) if !var.is_empty() => Ok(Self::Env(var.to_owned())),
            _ => Err(BadCredentialRef(
                "expected `keychain:<id>`, `env:<VAR>` or empty".to_owned(),
            )),
        }
    }
}

/// A secret. `Debug` and `Display` never show it.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret text, for building a request header only.
    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Why a secret could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CredentialReadError {
    /// The store is available and holds nothing under the reference.
    NotFound,
    /// The store could not be consulted; the text is for display and holds no secret.
    Unavailable(String),
}

/// Reads the secret a reference points to.
pub trait CredentialReader: Send + Sync {
    /// The secret of `reference`. Never called with [`CredentialRef::None`].
    fn read(&self, reference: &CredentialRef) -> Result<Secret, CredentialReadError>;
}

/// Reads `env:` references from the process environment and refuses `keychain:` ones.
type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct EnvCredentials {
    lookup: EnvLookup,
}

impl EnvCredentials {
    /// Read the real process environment.
    pub fn from_process() -> Self {
        Self {
            lookup: Box::new(|var| std::env::var(var).ok()),
        }
    }

    /// Read variables through `lookup` (for tests that must not touch the environment).
    pub fn with_lookup(lookup: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            lookup: Box::new(lookup),
        }
    }
}

impl fmt::Debug for EnvCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EnvCredentials")
    }
}

impl CredentialReader for EnvCredentials {
    fn read(&self, reference: &CredentialRef) -> Result<Secret, CredentialReadError> {
        match reference {
            CredentialRef::Env(var) => match (self.lookup)(var) {
                Some(v) if !v.trim().is_empty() => Ok(Secret::new(v)),
                _ => Err(CredentialReadError::NotFound),
            },
            CredentialRef::Keychain(_) => Err(CredentialReadError::Unavailable(
                "no system keychain is available in this build".to_owned(),
            )),
            CredentialRef::None => Err(CredentialReadError::NotFound),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn references_parse_in_the_three_forms_only() {
        assert_eq!(CredentialRef::parse(""), Ok(CredentialRef::None));
        assert_eq!(
            CredentialRef::parse("keychain:openai-mt"),
            Ok(CredentialRef::Keychain("openai-mt".into()))
        );
        assert_eq!(
            CredentialRef::parse("env:OPENAI_API_KEY"),
            Ok(CredentialRef::Env("OPENAI_API_KEY".into()))
        );
        assert!(CredentialRef::parse("sk-plain-secret").is_err());
        assert!(CredentialRef::parse("env:").is_err());
    }

    #[test]
    fn a_secret_never_formats() {
        let s = Secret::new("sk-very-secret");
        assert!(!format!("{s:?} {s}").contains("sk-very"));
    }

    #[test]
    fn the_default_reader_serves_env_and_refuses_the_keychain() {
        let r = EnvCredentials::with_lookup(|v| (v == "K").then(|| "value".to_owned()));
        assert_eq!(
            r.read(&CredentialRef::Env("K".into())).unwrap().expose(),
            "value"
        );
        assert_eq!(
            r.read(&CredentialRef::Env("OTHER".into())),
            Err(CredentialReadError::NotFound)
        );
        assert!(matches!(
            r.read(&CredentialRef::Keychain("x".into())),
            Err(CredentialReadError::Unavailable(_))
        ));
    }
}
