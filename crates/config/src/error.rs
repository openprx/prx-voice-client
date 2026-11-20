//! The error type of the configuration subsystem.

/// Errors from the configuration subsystem.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("parse error: {0}")]
    ParseError(String),
    #[error("validation error: {0}")]
    ValidationError(String),
    #[error("migration error: {0}")]
    MigrationError(String),
}
