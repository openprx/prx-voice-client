//! Security abstractions for the prx-voice-client system.
//!
//! Provides credential storage traits, data sanitization rules, and utilities
//! for handling sensitive information safely.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors originating from the security subsystem.
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    /// The requested credential was not found.
    #[error("credential not found: {0}")]
    CredentialNotFound(String),

    /// Failed to persist a credential.
    #[error("credential store failed: {0}")]
    CredentialStoreFailed(String),

    /// A sanitization operation failed.
    #[error("sanitization error: {0}")]
    SanitizationError(String),
}

// ---------------------------------------------------------------------------
// Sanitization
// ---------------------------------------------------------------------------

/// Rules for sanitizing sensitive string data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SanitizeRule {
    /// Replace the entire value with `[REDACTED]`.
    RedactAll,
    /// Keep the first `visible_chars` characters and mask the rest.
    MaskPartial {
        /// Number of leading characters to keep visible.
        visible_chars: usize,
    },
    /// Replace the value with a deterministic hash representation.
    Hash,
}

/// Apply a [`SanitizeRule`] to the given input string.
///
/// # Examples
///
/// ```
/// use prxvc_security::{SanitizeRule, sanitize};
///
/// assert_eq!(sanitize("secret", &SanitizeRule::RedactAll), "[REDACTED]");
/// assert_eq!(sanitize("secret", &SanitizeRule::MaskPartial { visible_chars: 2 }), "se***");
/// ```
pub fn sanitize(input: &str, rule: &SanitizeRule) -> String {
    match rule {
        SanitizeRule::RedactAll => "[REDACTED]".to_owned(),
        SanitizeRule::MaskPartial { visible_chars } => {
            if *visible_chars >= input.len() {
                return input.to_owned();
            }
            let visible: String = input.chars().take(*visible_chars).collect();
            format!("{visible}***")
        }
        SanitizeRule::Hash => {
            // Simple non-cryptographic hash (djb2 variant).
            let hash = input.bytes().fold(5381u64, |acc, b| {
                acc.wrapping_mul(33).wrapping_add(u64::from(b))
            });
            format!("[HASH:{hash:08x}]")
        }
    }
}

// ---------------------------------------------------------------------------
// Credential store trait
// ---------------------------------------------------------------------------

/// Abstraction for secure credential storage.
///
/// Implementations may use the OS keychain, encrypted files, or in-memory
/// stores depending on the platform and security requirements.
pub trait CredentialStore: Send + Sync {
    /// Retrieve a stored credential by key.
    ///
    /// Returns `Ok(None)` if the key does not exist.
    fn get(&self, key: &str) -> Result<Option<String>, SecurityError>;

    /// Store or update a credential.
    fn set(&self, key: &str, value: &str) -> Result<(), SecurityError>;

    /// Delete a credential by key.
    ///
    /// Returns `Ok(true)` if the key existed and was removed, `Ok(false)` if
    /// the key was not present.
    fn delete(&self, key: &str) -> Result<bool, SecurityError>;
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

/// Convenience re-exports for downstream crates.
pub mod prelude {
    pub use crate::{CredentialStore, SanitizeRule, SecurityError, sanitize};
}
