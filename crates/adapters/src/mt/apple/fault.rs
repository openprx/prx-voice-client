//! The typed failures of system translation, independent of how the call reached the framework.

use std::fmt;

use prxvc_ports::{MtError, MtPhase};

/// Why system translation cannot be used or a call failed. The variants are what the user can act
/// on (a missing pack, an old system); `message` carries the framework's own words for the log.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemFault {
    /// What kind of failure.
    pub kind: SystemFaultKind,
    /// A short English description for logs and the debug view.
    pub message: String,
}

/// The kinds of failure, in the order the issue list shows them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SystemFaultKind {
    /// The system is older than macOS 26: the framework's session API is not there.
    OsTooOld,
    /// The language pair is supported but its pack is not downloaded (System Settings, General,
    /// Language and Region, Translation Languages).
    PackNotInstalled,
    /// The framework does not translate this pair.
    PairUnsupported,
    /// No answer within the timeout.
    Timeout,
    /// The call was cancelled.
    Cancelled,
    /// The main thread does not run its run loop, which the framework needs (never in the app).
    NoMainLoop,
    /// Any other framework failure.
    Framework,
}

impl SystemFault {
    /// A fault of `kind` with `message`.
    pub fn new(kind: SystemFaultKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// The port's view of a failure of a running request. The pack, pair and system problems are
    /// found when the engine is built; here they can only mean the system changed under a call.
    pub fn to_port_error(&self) -> MtError {
        match self.kind {
            SystemFaultKind::Cancelled => MtError::Cancelled,
            SystemFaultKind::Timeout => MtError::TimedOut {
                phase: MtPhase::FirstToken,
            },
            _ => MtError::Unavailable {
                reason: self.to_string(),
            },
        }
    }
}

impl fmt::Display for SystemFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.kind {
            SystemFaultKind::OsTooOld => "system translation needs macOS 26 or later",
            SystemFaultKind::PackNotInstalled => "the system translation language pack is missing",
            SystemFaultKind::PairUnsupported => "system translation does not support this pair",
            SystemFaultKind::Timeout => "system translation timed out",
            SystemFaultKind::Cancelled => "system translation cancelled",
            SystemFaultKind::NoMainLoop => "the main thread does not run its run loop",
            SystemFaultKind::Framework => "system translation failed",
        };
        if self.message.is_empty() {
            f.write_str(what)
        } else {
            write!(f, "{what}: {}", self.message)
        }
    }
}

impl std::error::Error for SystemFault {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn running_failures_map_to_the_ports_typed_errors() {
        let f = |k| SystemFault::new(k, "m").to_port_error();
        assert_eq!(f(SystemFaultKind::Cancelled), MtError::Cancelled);
        assert!(f(SystemFaultKind::Timeout).is_timeout());
        assert!(matches!(
            f(SystemFaultKind::Framework),
            MtError::Unavailable { .. }
        ));
        assert!(matches!(
            f(SystemFaultKind::PackNotInstalled),
            MtError::Unavailable { .. }
        ));
    }

    #[test]
    fn the_text_names_the_cause_and_keeps_the_frameworks_message() {
        let text = SystemFault::new(SystemFaultKind::PackNotInstalled, "zh to en").to_string();
        assert!(text.contains("language pack") && text.contains("zh to en"));
        assert!(
            !SystemFault::new(SystemFaultKind::OsTooOld, "")
                .to_string()
                .ends_with(": ")
        );
    }
}
