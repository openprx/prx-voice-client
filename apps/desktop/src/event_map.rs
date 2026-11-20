//! Event mapping: translates engine events into [`SharedAppState`] updates the UI renders.

use prxvc_audio::input_supervisor::AudioLoopEvent;
use prxvc_desktop_ui::{EventSnapshot, NetworkStatus, SharedAppState, TranscriptLine};

/// Map an [`Event`] from the engine to [`SharedAppState`] updates so the
/// egui UI can reflect the latest state.
pub(crate) fn handle_engine_event(event: &prxvc_protocol::Event, shared: &SharedAppState) {
    use prxvc_protocol::Event;

    // Build a concise event-type label from the variant name.
    let debug_repr = format!("{event:?}");
    let event_type = debug_repr
        .split('(')
        .next()
        .unwrap_or("Unknown")
        .split('{')
        .next()
        .unwrap_or("Unknown")
        .trim()
        .to_owned();

    shared.push_event(EventSnapshot {
        event_type,
        detail: debug_repr,
        timestamp: chrono::Utc::now().to_rfc3339(),
    });

    match event {
        Event::SessionStateChanged { to, .. } => {
            let active = to != "Completed" && to != "Failed";
            shared.update_session_state(to, active);
        }
        Event::InterimTranscriptProduced { text, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "user".into(),
                text: text.clone(),
                is_final: false,
            });
        }
        Event::FinalTranscriptProduced { text, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "user".into(),
                text: text.clone(),
                is_final: true,
            });
        }
        Event::AssistantReplyTextDelta { delta, .. } => {
            shared.add_transcript_line(TranscriptLine {
                role: "assistant".into(),
                text: delta.clone(),
                is_final: true,
            });
        }
        Event::DeviceListUpdated { devices } => {
            tracing::info!("devices updated: {} devices", devices.len());
        }
        Event::ErrorOccurred {
            code,
            message,
            recoverable,
        } => {
            shared.set_error(message, *recoverable);
            if code.contains("reconnect") {
                shared.set_network_status(NetworkStatus::Reconnecting);
            }
        }
        Event::DiagnosticsExported { path } => {
            shared.push_event(EventSnapshot {
                event_type: "DiagnosticsExported".into(),
                detail: format!("Exported to: {path}"),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        _ => {}
    }
}

/// Push an error event to the shared state for display in the UI.
pub(crate) fn push_error(shared: &SharedAppState, msg: &str) {
    shared.push_event(EventSnapshot {
        event_type: "Error".into(),
        detail: msg.to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

/// Show a microphone change: update the displayed input device and record the
/// change in the event log. A lost device is also surfaced as an error event.
pub(crate) fn handle_audio_event(event: &AudioLoopEvent, shared: &SharedAppState) {
    match event {
        AudioLoopEvent::InputOpened { device, reason, previous } => {
            shared.set_input_device(&device.name);
            let from = previous.as_ref().map_or_else(String::new, |p| format!(" (was \"{}\")", p.name));
            shared.push_event(EventSnapshot {
                event_type: "InputDeviceChanged".into(),
                detail: format!(
                    "Input device: \"{}\" [{:?}, {:?}, reason {:?}]{from}",
                    device.name, device.transport, device.uid, reason
                ),
                timestamp: chrono::Utc::now().to_rfc3339(),
            });
        }
        AudioLoopEvent::InputUnavailable { message } => {
            shared.set_input_device("");
            push_error(shared, &format!("Audio input: {message}"));
        }
    }
}
