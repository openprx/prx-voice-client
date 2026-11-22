//! Judgement of an application's audio activity: is a call active, and does it use our virtual microphone (pipeline-design PD-4.1.2).
//!
//! Pure. A call is active when the application's input runs. A call application
//! that uses voice processing lists its output device (the speakers) among its
//! input devices as the echo reference of that unit; that entry is not a
//! microphone and is ignored. The decision "uses our microphone" is only
//! whether our microphone's UID is among the input devices. When a call runs on
//! some other input the remote party hears the real microphone, which the UI
//! must show ([`CallJudgement::ActiveOnOtherMicrophone`]).
//!
//! Limit: a voice message recorded on our microphone looks the same as a call
//! on it (input running on our microphone); the judgement reports
//! `output_running` so the caller can tell them apart if it needs to.

use super::model::{AppAudioActivity, ProcessDevice};

/// What the activity of an application means for call mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallJudgement {
    /// The audio system does not know the application: it is not running.
    NotRunning,
    /// The application runs but does not record.
    Idle,
    /// The application records from our virtual microphone.
    ActiveOnOurMicrophone {
        /// Whether the application also plays audio (a call rather than a voice message).
        output_running: bool,
    },
    /// The application records, but not from our microphone: the remote party
    /// hears the real microphone.
    ActiveOnOtherMicrophone {
        /// The input devices that are microphones (echo-reference entries removed).
        /// Empty when the list holds only echo-reference entries or nothing.
        devices: Vec<ProcessDevice>,
    },
}

impl CallJudgement {
    /// A call (or recording) is active: the application's input runs.
    pub fn call_active(&self) -> bool {
        matches!(
            self,
            Self::ActiveOnOurMicrophone { .. } | Self::ActiveOnOtherMicrophone { .. }
        )
    }

    /// The active call uses our microphone.
    pub fn uses_our_microphone(&self) -> bool {
        matches!(self, Self::ActiveOnOurMicrophone { .. })
    }
}

/// Whether an input-list entry is the echo reference of the application's
/// voice-processing unit rather than a microphone: an output-capable device, or
/// one the application also uses for output. Our own microphone is never one.
pub fn is_echo_reference(
    device: &ProcessDevice,
    outputs: &[ProcessDevice],
    our_microphone_uid: &str,
) -> bool {
    if !our_microphone_uid.is_empty() && device.uid == our_microphone_uid {
        return false;
    }
    device.is_output_capable() || outputs.iter().any(|o| same_device(o, device))
}

fn same_device(a: &ProcessDevice, b: &ProcessDevice) -> bool {
    if a.uid.is_empty() || b.uid.is_empty() {
        a.object_id == b.object_id
    } else {
        a.uid == b.uid
    }
}

/// Judge `activity` against our virtual microphone's device UID.
pub fn judge_call(activity: &AppAudioActivity, our_microphone_uid: &str) -> CallJudgement {
    if !activity.present {
        return CallJudgement::NotRunning;
    }
    if !activity.input_running {
        return CallJudgement::Idle;
    }
    let ours = !our_microphone_uid.is_empty()
        && activity
            .input_devices
            .iter()
            .any(|d| d.uid == our_microphone_uid);
    if ours {
        return CallJudgement::ActiveOnOurMicrophone {
            output_running: activity.output_running,
        };
    }
    let devices = activity
        .input_devices
        .iter()
        .filter(|d| !is_echo_reference(d, &activity.output_devices, our_microphone_uid))
        .cloned()
        .collect();
    CallJudgement::ActiveOnOtherMicrophone { devices }
}
