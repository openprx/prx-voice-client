//! The process tap as a pump source: reads frames like any source and turns the tap's health events into pump events.
//!
//! The tap's supervisor thread reports stalls, the target quitting, failed
//! re-creations and the digital-zero watchdog as [`TapEvent`]s. A pump owns its
//! source on its own thread, so this wrapper drains those events on every read
//! and posts the ones the coordinator must know about to the pump's
//! [`EventQueue`]. The tap is created unmuted: the call application keeps
//! playing for the user.

use std::time::Duration;

use prxvc_media::AudioFrame;
use prxvc_ports::{AudioSource, EndpointError, EndpointHealth, SourceInfo};

use crate::call_pump::{EventQueue, PumpEvent};
use crate::core_audio_tap::CoreAudioTapSource;
use crate::tap_types::{TapConfig, TapError, TapEvent};

/// Translate one tap event; `None` for events that need no reaction.
pub fn map_tap_event(event: TapEvent) -> Option<PumpEvent> {
    match event {
        TapEvent::TargetGone => Some(PumpEvent::TargetGone),
        TapEvent::Started { restored: true, .. } => Some(PumpEvent::TargetBack),
        TapEvent::Stalled { idle_ms } => Some(PumpEvent::Stalled { idle_ms }),
        TapEvent::NoSignal { error } => Some(PumpEvent::NoSignal {
            detail: error.to_string(),
        }),
        TapEvent::SignalRestored => Some(PumpEvent::SignalRestored),
        TapEvent::RestartFailed { error } => Some(match error {
            TapError::PermissionDenied
            | TapError::NotInAppBundle
            | TapError::ConsentUndetermined => PumpEvent::CaptureDenied {
                detail: error.to_string(),
            },
            TapError::ProcessNotFound { .. } => PumpEvent::TargetGone,
            other => PumpEvent::CaptureFailed {
                detail: other.to_string(),
            },
        }),
        // The rebuild after silence is under way; `NoSignal` follows if it does not help.
        TapEvent::SilentWhilePlaying { silent_ms } => {
            tracing::info!(
                silent_ms,
                "tap delivers silence while the application plays; rebuilding"
            );
            None
        }
        TapEvent::Started {
            restored: false, ..
        } => None,
        TapEvent::FormatMismatch { dropped } => {
            tracing::warn!(
                dropped,
                "tap buffers with an unexpected format were dropped"
            );
            None
        }
    }
}

/// A [`CoreAudioTapSource`] that posts its health events to an [`EventQueue`].
pub struct TapPumpSource {
    inner: CoreAudioTapSource,
    events: EventQueue,
}

impl TapPumpSource {
    /// A source that taps as `config` says (unmuted unless the caller opts in).
    pub fn new(config: TapConfig, events: EventQueue) -> Self {
        Self {
            inner: CoreAudioTapSource::with_config(config),
            events,
        }
    }

    fn relay_events(&self) {
        while let Some(event) = self.inner.try_recv_event() {
            if let Some(mapped) = map_tap_event(event) {
                self.events.push(mapped);
            }
        }
    }
}

impl AudioSource for TapPumpSource {
    fn start(&mut self) -> Result<SourceInfo, EndpointError> {
        self.inner.start()
    }

    fn read(&mut self, out: &mut Vec<AudioFrame>, max_wait: Duration) -> Result<(), EndpointError> {
        let result = self.inner.read(out, max_wait);
        self.relay_events();
        result
    }

    fn health(&self) -> EndpointHealth {
        self.inner.health()
    }

    fn stop(&mut self) {
        self.inner.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_events_become_pump_events() {
        assert_eq!(
            map_tap_event(TapEvent::TargetGone),
            Some(PumpEvent::TargetGone)
        );
        assert_eq!(
            map_tap_event(TapEvent::Started {
                restored: true,
                bundle_form: true
            }),
            Some(PumpEvent::TargetBack)
        );
        assert_eq!(
            map_tap_event(TapEvent::Started {
                restored: false,
                bundle_form: true
            }),
            None
        );
        assert_eq!(
            map_tap_event(TapEvent::Stalled { idle_ms: 2_000 }),
            Some(PumpEvent::Stalled { idle_ms: 2_000 })
        );
        assert_eq!(
            map_tap_event(TapEvent::SignalRestored),
            Some(PumpEvent::SignalRestored)
        );
        assert_eq!(map_tap_event(TapEvent::FormatMismatch { dropped: 3 }), None);
        assert_eq!(
            map_tap_event(TapEvent::SilentWhilePlaying { silent_ms: 3_000 }),
            None
        );
    }

    #[test]
    fn the_zero_watchdog_error_carries_the_consent_instructions() {
        let error = TapError::NoSignal {
            target: "net.whatsapp.WhatsApp".into(),
        };
        match map_tap_event(TapEvent::NoSignal { error }) {
            Some(PumpEvent::NoSignal { detail }) => {
                assert!(
                    detail.contains("net.whatsapp.WhatsApp")
                        && detail.contains("System Audio Recording")
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn failed_rebuilds_are_classified_by_their_cause() {
        let denied = map_tap_event(TapEvent::RestartFailed {
            error: TapError::PermissionDenied,
        });
        assert!(matches!(denied, Some(PumpEvent::CaptureDenied { .. })));
        let gone = map_tap_event(TapEvent::RestartFailed {
            error: TapError::ProcessNotFound { target: "x".into() },
        });
        assert_eq!(gone, Some(PumpEvent::TargetGone));
        let other = map_tap_event(TapEvent::RestartFailed {
            error: TapError::CoreAudio {
                operation: "create".into(),
                status: -1,
            },
        });
        assert!(matches!(other, Some(PumpEvent::CaptureFailed { .. })));
    }
}
