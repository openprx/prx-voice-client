//! Events the routing controller reports to the call coordinator while call mode is active, and the device reference they carry.

use prxvc_ports::routing::DeviceUid;

/// A device by identity and display name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRef {
    /// The stable identifier.
    pub uid: DeviceUid,
    /// The name to show.
    pub name: String,
}

/// Something the coordinator or the UI must know about the routing.
///
/// The controller never reacts to any of these by changing the system on its
/// own: the user's choice of microphone is not fought (pipeline-design 4.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoutingEvent {
    /// The default input is no longer our virtual microphone and we did not do
    /// it (the user, or the system after a headset was connected). Other
    /// applications now hear this device instead of the translation. The UI
    /// offers "Use PRX Voice Microphone again", which calls
    /// `RoutingController::reassert`.
    DefaultInputDisplaced {
        /// The default input now.
        to: DeviceUid,
    },
    /// The default input is our virtual microphone again (the user selected it).
    OwnMicrophoneReselected,
    /// What leaving call mode will restore changed (the remembered device went
    /// away or came back).
    RestoreTargetChanged {
        /// The device that would be restored now; `None` when no physical input exists.
        target: Option<DeviceRef>,
    },
    /// Our virtual microphone is gone or not alive (audio daemon restart,
    /// driver removed). The coordinator leaves call mode.
    VirtualMicrophoneLost,
    /// The default output changed.
    DefaultOutputChanged {
        /// The default output now.
        to: DeviceUid,
    },
    /// The machine is going to sleep. Nothing is restored: the journal stays.
    SystemSleeping,
    /// The machine woke. `default_input_intact` is whether our virtual
    /// microphone is still the default input and present; when it is not, a
    /// [`RoutingEvent::DefaultInputDisplaced`] or
    /// [`RoutingEvent::VirtualMicrophoneLost`] follows.
    SystemWoke {
        /// Our microphone is present and still the default input.
        default_input_intact: bool,
    },
}
