//! Maps provider errors onto the fault classes the machines branch on, without carrying call content.

use prxvc_ports::{AsrError, MtError, TtsError, VadError};

use crate::fault::FaultKind;
use crate::pipeline::messages::StageFailure;

/// A recognition failure; `in_use` is false while the stream is being opened.
pub(crate) fn asr_failure(e: &AsrError, in_use: bool) -> StageFailure {
    let fault = match e {
        AsrError::Timeout => FaultKind::Timeout,
        AsrError::Unavailable { .. } if in_use => FaultKind::Crashed,
        AsrError::Unavailable { .. }
        | AsrError::UnsupportedLanguage(_)
        | AsrError::UnsupportedRate(_)
        | AsrError::Cancelled => FaultKind::Unavailable,
        AsrError::Internal(_) => FaultKind::Crashed,
        _ => FaultKind::Crashed,
    };
    StageFailure::new(fault, e.to_string())
}

/// A voice activity detection failure.
pub(crate) fn vad_failure(e: &VadError, in_use: bool) -> StageFailure {
    let fault = match e {
        VadError::Unavailable { .. } if !in_use => FaultKind::Unavailable,
        _ => FaultKind::Crashed,
    };
    StageFailure::new(fault, e.to_string())
}

/// A translation failure; `in_use` is false when the request could not be opened.
pub(crate) fn mt_failure(e: &MtError, in_use: bool) -> StageFailure {
    let fault = match e {
        e if e.is_timeout() => FaultKind::Timeout,
        MtError::Unavailable { .. } if in_use => FaultKind::Crashed,
        MtError::Unavailable { .. } | MtError::Cancelled => FaultKind::Unavailable,
        MtError::Rejected { .. } => FaultKind::Rejected,
        _ => FaultKind::Crashed,
    };
    StageFailure::new(fault, e.to_string())
}

/// A synthesis failure; `in_use` is false when the request could not be opened.
pub(crate) fn tts_failure(e: &TtsError, in_use: bool) -> StageFailure {
    let fault = match e {
        TtsError::Timeout => FaultKind::Timeout,
        TtsError::Unavailable { .. } if in_use => FaultKind::Crashed,
        TtsError::Unavailable { .. }
        | TtsError::Cancelled
        | TtsError::UnknownVoice(_)
        | TtsError::UnsupportedLanguage(_) => FaultKind::Unavailable,
        TtsError::Rejected { .. } => FaultKind::Rejected,
        _ => FaultKind::Crashed,
    };
    StageFailure::new(fault, e.to_string())
}

#[cfg(test)]
mod tests {
    use prxvc_ports::mt::MtPhase;

    use super::*;

    #[test]
    fn timeouts_with_and_without_a_phase_are_timeouts() {
        assert_eq!(
            mt_failure(&MtError::Timeout, true).fault,
            FaultKind::Timeout
        );
        let phased = MtError::TimedOut {
            phase: MtPhase::FirstToken,
        };
        assert_eq!(mt_failure(&phased, false).fault, FaultKind::Timeout);
        assert_eq!(
            tts_failure(&TtsError::Timeout, true).fault,
            FaultKind::Timeout
        );
        assert_eq!(
            asr_failure(&AsrError::Timeout, true).fault,
            FaultKind::Timeout
        );
    }

    #[test]
    fn an_unavailable_provider_is_unreachable_at_open_and_crashed_in_use() {
        let e = MtError::Unavailable {
            reason: "down".to_owned(),
        };
        assert_eq!(mt_failure(&e, false).fault, FaultKind::Unavailable);
        assert_eq!(mt_failure(&e, true).fault, FaultKind::Crashed);
        assert_eq!(
            mt_failure(&MtError::Rejected { status: 429 }, true).fault,
            FaultKind::Rejected
        );
    }
}
