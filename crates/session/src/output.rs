//! Audible-output vocabulary of a direction: which source a listener hears.
//!
//! Includes the configured fallback used while translation is not playing.

use prxvc_media::Direction;

/// The audible source of one direction.
///
/// For the incoming direction `Translation` is rendered according to
/// `call.local_output_mode` by the mixer (translation, both, original); that
/// choice is a mixer gain, not a machine state (pipeline-design 4.5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputSource {
    /// The synthesised translation.
    Translation,
    /// The original speech, passed through.
    Original,
    /// Silence.
    Nothing,
}

/// The configured fallback: `"original"` or `"silence"` in the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Passthrough {
    /// The listener hears the original speech.
    Original,
    /// The listener hears nothing.
    Silence,
}

impl Passthrough {
    /// The audible source this fallback selects.
    pub const fn source(self) -> OutputSource {
        match self {
            Self::Original => OutputSource::Original,
            Self::Silence => OutputSource::Nothing,
        }
    }
}

/// Why the audible source of a direction changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputCause {
    /// The direction finished starting and now translates.
    TranslationStarted,
    /// A pause or a suspension ended.
    TranslationResumed,
    /// The direction was paused (`call.*_when_paused`).
    Paused,
    /// The direction was suspended (sleep, lost endpoint); the failure policy applies.
    Suspended,
    /// The direction failed (`call.*_on_failure`).
    Failed,
    /// The direction is stopping.
    Stopping,
    /// The direction is down and its mixer returned to the default source.
    Stopped,
}

/// The two fallbacks of one direction, from `call.<direction>_when_paused` and `call.<direction>_on_failure`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OutputPolicy {
    /// What is heard while the direction is paused.
    pub when_paused: Passthrough,
    /// What is heard while the direction failed or is suspended.
    pub on_failure: Passthrough,
}

impl Default for OutputPolicy {
    /// Both `original` (the configuration defaults, D-P3).
    fn default() -> Self {
        Self {
            when_paused: Passthrough::Original,
            on_failure: Passthrough::Original,
        }
    }
}

/// Everything a direction machine needs besides its state: which direction it is and how to fall back.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DirectionConfig {
    /// The direction the machine runs.
    pub direction: Direction,
    /// The fallbacks.
    pub policy: OutputPolicy,
}

impl DirectionConfig {
    /// Configuration with the default fallbacks.
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            policy: OutputPolicy::default(),
        }
    }
}
