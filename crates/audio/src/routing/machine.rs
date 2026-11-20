//! Pure state machine of call mode: how the controller's phase and the events it reports follow from a system change and what the system looks like now.
//!
//! No Core Audio, no clock, no I/O: `react` takes the state, one change and a
//! snapshot of the world and returns the new state and the events to report.
//! The rule it implements (pipeline-design 4.4): when the default input leaves
//! our microphone without our doing it, report it and wait for the user; never
//! switch back by itself, so there is no loop to fight the user in.
//!
//! | Phase | Change | New phase | Events |
//! |---|---|---|---|
//! | `Idle` | anything | `Idle` | none |
//! | `Active` | our microphone not usable | `MicLost` | `VirtualMicrophoneLost` |
//! | `Active` | default input is another device | `Displaced` | `DefaultInputDisplaced` |
//! | `Displaced` | default input is ours again | `Active` | `OwnMicrophoneReselected` |
//! | `Displaced` | default input is a different other device | `Displaced` | `DefaultInputDisplaced` |
//! | `Active`, `Displaced` | device list changed, restore target differs | same | `RestoreTargetChanged` |
//! | `MicLost` | anything but sleep and wake | `MicLost` | none (the coordinator leaves) |
//! | any but `Idle` | sleep | same | `SystemSleeping` |
//! | any but `Idle` | wake | re-evaluated like a change | `SystemWoke`, then what the re-evaluation reports |

use prxvc_ports::routing::{DeviceUid, RoutingChange};

use super::events::{DeviceRef, RoutingEvent};
use crate::input_policy::OWN_MIC_UID;

/// Where call mode stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    /// Not in call mode; changes are ignored.
    Idle,
    /// Our microphone is the default input.
    Active,
    /// Call mode is on but the default input is another device (not our doing).
    Displaced {
        /// The default input now.
        to: DeviceUid,
    },
    /// Our microphone is gone; the coordinator is expected to leave call mode.
    MicLost,
}

/// The controller's state between events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallState {
    /// The phase.
    pub phase: Phase,
    /// The default input before call mode (`None` when it was already ours).
    pub saved: Option<DeviceRef>,
    /// What leaving would restore, as last reported.
    pub restore_target: Option<DeviceRef>,
}

impl CallState {
    /// Not in call mode.
    pub fn idle() -> Self {
        Self {
            phase: Phase::Idle,
            saved: None,
            restore_target: None,
        }
    }

    /// Call mode just entered: our microphone is the default input.
    pub fn entered(saved: Option<DeviceRef>, restore_target: Option<DeviceRef>) -> Self {
        Self {
            phase: Phase::Active,
            saved,
            restore_target,
        }
    }
}

/// What the system looks like when a change is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    /// The default input now; `None` when it could not be read.
    pub default_input: Option<DeviceUid>,
    /// Our microphone is present, alive and an input.
    pub own_mic_usable: bool,
    /// What leaving call mode would restore now.
    pub restore_target: Option<DeviceRef>,
}

/// Handle one change.
pub fn react(
    state: &CallState,
    change: &RoutingChange,
    world: &World,
) -> (CallState, Vec<RoutingEvent>) {
    let mut next = state.clone();
    let mut events = Vec::new();
    if state.phase == Phase::Idle {
        return (next, events);
    }
    match change {
        RoutingChange::Sleep => events.push(RoutingEvent::SystemSleeping),
        RoutingChange::Wake => {
            let mut after = Vec::new();
            evaluate(&mut next, world, &mut after);
            events.push(RoutingEvent::SystemWoke {
                default_input_intact: next.phase == Phase::Active,
            });
            events.append(&mut after);
        }
        RoutingChange::DefaultInputChanged(_) => evaluate(&mut next, world, &mut events),
        RoutingChange::DeviceListChanged => {
            evaluate(&mut next, world, &mut events);
            if matches!(next.phase, Phase::Active | Phase::Displaced { .. })
                && next.restore_target != world.restore_target
            {
                next.restore_target.clone_from(&world.restore_target);
                events.push(RoutingEvent::RestoreTargetChanged {
                    target: world.restore_target.clone(),
                });
            }
        }
        RoutingChange::DefaultOutputChanged(uid) => {
            events.push(RoutingEvent::DefaultOutputChanged { to: uid.clone() });
        }
    }
    (next, events)
}

/// Compare the phase with the world and report what differs. Idempotent: the
/// same world reports nothing the second time.
fn evaluate(state: &mut CallState, world: &World, events: &mut Vec<RoutingEvent>) {
    if state.phase == Phase::MicLost {
        return;
    }
    if !world.own_mic_usable {
        state.phase = Phase::MicLost;
        events.push(RoutingEvent::VirtualMicrophoneLost);
        return;
    }
    let Some(default) = &world.default_input else {
        // Unreadable: no evidence either way, so no event.
        return;
    };
    let ours = default.as_str() == OWN_MIC_UID;
    match (&state.phase, ours) {
        (Phase::Active, false) => {
            state.phase = Phase::Displaced {
                to: default.clone(),
            };
            events.push(RoutingEvent::DefaultInputDisplaced {
                to: default.clone(),
            });
        }
        (Phase::Displaced { to }, false) if to != default => {
            state.phase = Phase::Displaced {
                to: default.clone(),
            };
            events.push(RoutingEvent::DefaultInputDisplaced {
                to: default.clone(),
            });
        }
        (Phase::Displaced { .. }, true) => {
            state.phase = Phase::Active;
            events.push(RoutingEvent::OwnMicrophoneReselected);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(s: &str) -> DeviceUid {
        DeviceUid::new(s)
    }

    fn dev(s: &str) -> DeviceRef {
        DeviceRef {
            uid: uid(s),
            name: s.to_owned(),
        }
    }

    fn world(default: &str, usable: bool, target: &str) -> World {
        World {
            default_input: Some(uid(default)),
            own_mic_usable: usable,
            restore_target: Some(dev(target)),
        }
    }

    fn active() -> CallState {
        CallState::entered(Some(dev("builtin")), Some(dev("builtin")))
    }

    fn input(to: &str) -> RoutingChange {
        RoutingChange::DefaultInputChanged(uid(to))
    }

    #[test]
    fn idle_ignores_everything() {
        for change in [
            RoutingChange::Sleep,
            RoutingChange::Wake,
            RoutingChange::DeviceListChanged,
            input("x"),
        ] {
            let (next, events) = react(&CallState::idle(), &change, &world("x", false, "y"));
            assert_eq!(next, CallState::idle());
            assert!(events.is_empty());
        }
    }

    #[test]
    fn our_own_switch_notification_changes_nothing() {
        let (next, events) = react(
            &active(),
            &input(OWN_MIC_UID),
            &world(OWN_MIC_UID, true, "builtin"),
        );
        assert_eq!(next, active());
        assert!(events.is_empty());
    }

    #[test]
    fn a_default_change_away_from_us_is_reported_once_and_not_fought() {
        let w = world("headset", true, "builtin");
        let (displaced, events) = react(&active(), &input("headset"), &w);
        assert_eq!(displaced.phase, Phase::Displaced { to: uid("headset") });
        assert_eq!(
            events,
            vec![RoutingEvent::DefaultInputDisplaced { to: uid("headset") }]
        );
        // The same notice again reports nothing.
        let (again, events) = react(&displaced, &input("headset"), &w);
        assert_eq!(again, displaced);
        assert!(events.is_empty());
    }

    #[test]
    fn a_second_other_device_is_reported_and_choosing_ours_ends_the_displacement() {
        let (d1, _) = react(&active(), &input("a"), &world("a", true, "builtin"));
        let (d2, events) = react(&d1, &input("b"), &world("b", true, "builtin"));
        assert_eq!(
            events,
            vec![RoutingEvent::DefaultInputDisplaced { to: uid("b") }]
        );
        let (back, events) = react(
            &d2,
            &input(OWN_MIC_UID),
            &world(OWN_MIC_UID, true, "builtin"),
        );
        assert_eq!(back.phase, Phase::Active);
        assert_eq!(events, vec![RoutingEvent::OwnMicrophoneReselected]);
    }

    #[test]
    fn a_vanished_microphone_is_reported_once_and_wins_over_a_default_change() {
        let w = world("builtin", false, "builtin");
        let (lost, events) = react(&active(), &input("builtin"), &w);
        assert_eq!(lost.phase, Phase::MicLost);
        assert_eq!(events, vec![RoutingEvent::VirtualMicrophoneLost]);
        let (still, events) = react(&lost, &RoutingChange::DeviceListChanged, &w);
        assert_eq!(still.phase, Phase::MicLost);
        assert!(events.is_empty());
    }

    #[test]
    fn a_device_list_change_updates_the_restore_target() {
        let (next, events) = react(
            &active(),
            &RoutingChange::DeviceListChanged,
            &world(OWN_MIC_UID, true, "usb"),
        );
        assert_eq!(next.restore_target, Some(dev("usb")));
        assert_eq!(
            events,
            vec![RoutingEvent::RestoreTargetChanged {
                target: Some(dev("usb"))
            }]
        );
        let (_, events) = react(
            &next,
            &RoutingChange::DeviceListChanged,
            &world(OWN_MIC_UID, true, "usb"),
        );
        assert!(events.is_empty());
    }

    #[test]
    fn sleep_is_reported_and_wake_reevaluates() {
        let (_, events) = react(
            &active(),
            &RoutingChange::Sleep,
            &world(OWN_MIC_UID, true, "builtin"),
        );
        assert_eq!(events, vec![RoutingEvent::SystemSleeping]);

        let (next, events) = react(
            &active(),
            &RoutingChange::Wake,
            &world(OWN_MIC_UID, true, "builtin"),
        );
        assert_eq!(next.phase, Phase::Active);
        assert_eq!(
            events,
            vec![RoutingEvent::SystemWoke {
                default_input_intact: true
            }]
        );

        let (next, events) = react(
            &active(),
            &RoutingChange::Wake,
            &world("other", true, "builtin"),
        );
        assert_eq!(next.phase, Phase::Displaced { to: uid("other") });
        assert_eq!(
            events,
            vec![
                RoutingEvent::SystemWoke {
                    default_input_intact: false
                },
                RoutingEvent::DefaultInputDisplaced { to: uid("other") }
            ]
        );

        let (next, events) = react(
            &active(),
            &RoutingChange::Wake,
            &world("other", false, "builtin"),
        );
        assert_eq!(next.phase, Phase::MicLost);
        assert_eq!(
            events,
            vec![
                RoutingEvent::SystemWoke {
                    default_input_intact: false
                },
                RoutingEvent::VirtualMicrophoneLost
            ]
        );
    }

    #[test]
    fn an_unreadable_default_input_reports_nothing() {
        let w = World {
            default_input: None,
            own_mic_usable: true,
            restore_target: Some(dev("builtin")),
        };
        let (next, events) = react(&active(), &input("x"), &w);
        assert_eq!(next, active());
        assert!(events.is_empty());
    }

    #[test]
    fn the_default_output_is_passed_through() {
        let (_, events) = react(
            &active(),
            &RoutingChange::DefaultOutputChanged(uid("speakers")),
            &world(OWN_MIC_UID, true, "builtin"),
        );
        assert_eq!(
            events,
            vec![RoutingEvent::DefaultOutputChanged {
                to: uid("speakers")
            }]
        );
    }
}
