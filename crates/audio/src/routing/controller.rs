//! The routing controller: enters and leaves call mode by switching the system default input, keeps the journal, recovers after a crash and reports changes while call mode is on.
//!
//! The decisions are in [`super::machine`] (changes while on), [`super::restore`]
//! (where the default goes back to) and [`super::journal`] (the file); this
//! file orders the steps and talks to the system through the
//! [`SystemAudioRouting`] port, the [`SystemProbe`] and the [`JournalStore`].
//!
//! The order that matters (pipeline-design 4.1 steps 3 to 7, 4.2):
//! preconditions, remember the current default, write the journal, switch,
//! read back. Leaving: restore, read back, delete the journal. The journal is
//! therefore on disk whenever the default input may be ours.

use prxvc_ports::routing::{DeviceUid, SystemAudioRouting};

use super::events::{DeviceRef, RoutingEvent};
use super::journal::{Journal, JournalRead, JournalStore, SCHEMA_VERSION};
use super::machine::{CallState, Phase, World, react};
use super::recovery::{RecoveryPlan, decide};
use super::report::{CallModeError, EnterOutcome, LeaveOutcome, RecoveryReport, Restored};
use super::restore::{input_candidates, plan_restore};
use super::system::SystemProbe;
use crate::device_directory::DeviceEntry;
use crate::driver_status::DriverVariant;
use crate::input_policy::{OWN_MIC_UID, choose_input};

/// What a call needs from the microphone side.
#[derive(Debug, Clone, Default)]
pub struct EnterRequest {
    /// The driver variant the call application needs (`None` accepts any).
    pub required_variant: Option<DriverVariant>,
    /// The configured physical microphone (UID or name); empty means the input policy's default.
    pub configured_input: String,
}

/// The routing controller.
pub struct RoutingController<R, S, J> {
    routing: R,
    system: S,
    journal: J,
    state: CallState,
    idle_window_start: u64,
    idle_fixes: u32,
}

/// Restores [`RoutingController::correct_idle_default`] makes in one window.
pub const IDLE_FIXES_PER_WINDOW: u32 = 3;
/// The length of that window, in seconds.
pub const IDLE_FIX_WINDOW_SECS: u64 = 60;

impl<R: SystemAudioRouting, S: SystemProbe, J: JournalStore> RoutingController<R, S, J> {
    /// A controller that is not in call mode.
    pub fn new(routing: R, system: S, journal: J) -> Self {
        Self {
            routing,
            system,
            journal,
            state: CallState::idle(),
            idle_window_start: 0,
            idle_fixes: 0,
        }
    }

    /// Where call mode stands.
    pub fn phase(&self) -> &Phase {
        &self.state.phase
    }

    /// The full state (for diagnostics).
    pub fn state(&self) -> &CallState {
        &self.state
    }

    /// Switch the system default input to our virtual microphone.
    ///
    /// # Errors
    /// A [`CallModeError`] for the first failed precondition, the journal, the
    /// switch, or a switch that did not take effect. In every case the default
    /// input is as it was and no journal remains (unless putting it back failed,
    /// in which case the journal stays so that recovery can finish).
    pub fn enter_call_mode(
        &mut self,
        request: &EnterRequest,
    ) -> Result<EnterOutcome, CallModeError> {
        if self.state.phase != Phase::Idle {
            return Err(CallModeError::AlreadyInCallMode);
        }
        match self.journal.read() {
            JournalRead::Absent => {}
            JournalRead::Valid(j) => {
                return Err(CallModeError::JournalPresent {
                    owner_pid: j.pid,
                    owner_alive: self.owner_alive(j.pid),
                });
            }
            JournalRead::Corrupt { .. } => return Err(CallModeError::JournalCorrupt),
        }

        let devices = self.devices()?;
        self.system
            .driver_status()
            .check(request.required_variant)
            .map_err(CallModeError::DriverNotReady)?;
        let current = self
            .routing
            .default_input()
            .map_err(CallModeError::Routing)?;
        let candidates = input_candidates(&devices, Some(&current));
        let physical = choose_input(&candidates, &request.configured_input)
            .map_err(CallModeError::NoPhysicalMicrophone)?
            .device;
        let physical_input = DeviceRef {
            uid: DeviceUid::new(physical.uid),
            name: physical.name,
        };

        let saved = (current.as_str() != OWN_MIC_UID).then(|| DeviceRef {
            name: name_of(&devices, &current),
            uid: current.clone(),
        });
        let journal = Journal {
            schema_version: SCHEMA_VERSION,
            saved_default_input_uid: saved.as_ref().map(|d| d.uid.as_str().to_owned()),
            saved_default_input_name: saved.as_ref().map(|d| d.name.clone()),
            our_mic_uid: OWN_MIC_UID.to_owned(),
            pid: self.system.own_pid(),
            started_at: self.system.unix_secs(),
        };
        self.journal
            .write(&journal)
            .map_err(|e| CallModeError::JournalWrite(e.0))?;

        let own = DeviceUid::new(OWN_MIC_UID);
        let switched = self
            .routing
            .set_default_input(&own)
            .map_err(CallModeError::SwitchFailed)
            .and_then(|()| match self.routing.default_input() {
                Ok(actual) if actual == own => Ok(()),
                Ok(actual) => Err(CallModeError::SwitchNotApplied {
                    expected: own.clone(),
                    actual: Some(actual),
                }),
                Err(_) => Err(CallModeError::SwitchNotApplied {
                    expected: own.clone(),
                    actual: None,
                }),
            });
        if let Err(error) = switched {
            self.abort_enter(saved.as_ref());
            return Err(error);
        }

        let restore_target = plan_restore(saved.as_ref(), &devices, Some(&own)).map(|p| p.target);
        self.state = CallState::entered(saved.clone(), restore_target);
        tracing::info!(saved = ?saved, "call mode entered: default input is now the virtual microphone");
        Ok(EnterOutcome {
            saved,
            physical_input,
        })
    }

    /// Put the default input back and end call mode. Calling it when call mode
    /// is not on does nothing (idempotent).
    ///
    /// The remembered device is restored if it still exists, otherwise the
    /// input policy's choice; never one of our devices. When the default input
    /// is no longer ours (the user chose another device) it is left alone.
    ///
    /// # Errors
    /// A [`CallModeError`] when the default input cannot be read, no device
    /// can be restored, or the restore fails or does not take effect. Call
    /// mode and the journal then stay, so the call can be retried.
    pub fn leave_call_mode(&mut self) -> Result<LeaveOutcome, CallModeError> {
        if self.state.phase == Phase::Idle {
            return Ok(LeaveOutcome::NotInCallMode);
        }
        let current = self
            .routing
            .default_input()
            .map_err(CallModeError::Routing)?;
        let outcome = if current.as_str() == OWN_MIC_UID {
            let restored = self.restore_default(self.state.saved.clone().as_ref())?;
            LeaveOutcome::Restored {
                restored,
                journal_cleared: self.clear_journal(),
            }
        } else {
            LeaveOutcome::LeftAlone {
                default: Some(current),
                journal_cleared: self.clear_journal(),
            }
        };
        self.state = CallState::idle();
        tracing::info!(?outcome, "call mode left");
        Ok(outcome)
    }

    /// Make our microphone the default input again after the user (or the
    /// system) changed it: the explicit action behind "Use PRX Voice
    /// Microphone again". Never called by the controller itself.
    ///
    /// # Errors
    /// [`CallModeError::NotInCallMode`], [`CallModeError::VirtualMicrophoneLost`],
    /// [`CallModeError::SwitchFailed`] or [`CallModeError::SwitchNotApplied`].
    pub fn reassert(&mut self) -> Result<(), CallModeError> {
        match self.state.phase {
            Phase::Idle => return Err(CallModeError::NotInCallMode),
            Phase::MicLost => return Err(CallModeError::VirtualMicrophoneLost),
            Phase::Active | Phase::Displaced { .. } => {}
        }
        if !own_mic_usable(&self.devices()?) {
            self.state.phase = Phase::MicLost;
            return Err(CallModeError::VirtualMicrophoneLost);
        }
        let own = DeviceUid::new(OWN_MIC_UID);
        self.routing
            .set_default_input(&own)
            .map_err(CallModeError::SwitchFailed)?;
        let actual = self.routing.default_input().ok();
        if actual.as_ref() != Some(&own) {
            return Err(CallModeError::SwitchNotApplied {
                expected: own,
                actual,
            });
        }
        self.state.phase = Phase::Active;
        Ok(())
    }

    /// At application start: finish what a crashed run left behind.
    ///
    /// * Stale journal (its process is gone, or it is this process) and our
    ///   microphone is the default input: restore the default input.
    /// * Stale journal, default input already another device: delete the journal.
    /// * Journal of a running other process: leave everything alone.
    /// * No journal, our microphone is the default input: restore the input policy's choice (our
    ///   microphone is never the default outside call mode) and report it.
    /// * Unusable journal: move it aside; restore by policy when our
    ///   microphone is the default input.
    pub fn recover_if_needed(&mut self) -> RecoveryReport {
        if self.state.phase != Phase::Idle {
            return RecoveryReport::InCallMode;
        }
        let report = match self.recovery_plan() {
            RecoveryPlan::Nothing => RecoveryReport::Nothing,
            RecoveryPlan::OwnMicrophoneWithoutJournal => match self.restore_default(None) {
                Ok(restored) => RecoveryReport::OwnMicrophoneRestoredWithoutJournal { restored },
                Err(error) => RecoveryReport::Failed { error },
            },
            RecoveryPlan::OwnerAlive { pid } => RecoveryReport::OwnerAlive { pid },
            RecoveryPlan::Unreadable(e) => RecoveryReport::Failed {
                error: CallModeError::Routing(e),
            },
            RecoveryPlan::ClearStale { crashed_pid } => {
                self.clear_journal();
                RecoveryReport::StaleJournalCleared { crashed_pid }
            }
            RecoveryPlan::Restore { saved, crashed_pid } => {
                match self.restore_default(saved.as_ref()) {
                    Ok(restored) => {
                        self.clear_journal();
                        RecoveryReport::Restored {
                            restored,
                            crashed_pid,
                        }
                    }
                    Err(error) => RecoveryReport::Failed { error },
                }
            }
            RecoveryPlan::Corrupt { reason, restore } => self.recover_corrupt(reason, restore),
        };
        match &report {
            RecoveryReport::Nothing | RecoveryReport::InCallMode => {}
            RecoveryReport::Failed { error } => tracing::error!(%error, "routing recovery failed"),
            other => tracing::info!(report = ?other, "routing recovery"),
        }
        report
    }

    /// Outside call mode, put the default input back by policy when our virtual microphone has
    /// become the default (macOS picks it when the device that was the default is unplugged). Call
    /// it when the system reports a default-input change and now and then while idle; it acts only
    /// with no call mode and no journal, so it can never undo a call, and it stops after
    /// [`IDLE_FIXES_PER_WINDOW`] restores in [`IDLE_FIX_WINDOW_SECS`] seconds so that two
    /// parties changing the default cannot loop. `None`: nothing to do or not allowed.
    pub fn correct_idle_default(&mut self) -> Option<Result<Restored, CallModeError>> {
        if self.state.phase != Phase::Idle
            || !matches!(self.recovery_plan(), RecoveryPlan::OwnMicrophoneWithoutJournal)
        {
            return None;
        }
        let now = self.system.unix_secs();
        if now.saturating_sub(self.idle_window_start) >= IDLE_FIX_WINDOW_SECS {
            self.idle_window_start = now;
            self.idle_fixes = 0;
        }
        if self.idle_fixes >= IDLE_FIXES_PER_WINDOW {
            if self.idle_fixes == IDLE_FIXES_PER_WINDOW {
                self.idle_fixes += 1;
                tracing::warn!(
                    "the default input keeps returning to the PRX Voice Microphone outside call mode; not correcting it again for a while"
                );
            }
            return None;
        }
        self.idle_fixes += 1;
        let result = self.restore_default(None);
        match &result {
            Ok(restored) => tracing::info!(?restored, "the PRX Voice Microphone was the default input outside call mode; the policy's choice was restored"),
            Err(error) => tracing::error!(%error, "could not take the PRX Voice Microphone off the default input outside call mode"),
        }
        Some(result)
    }

    /// What [`recover_if_needed`](Self::recover_if_needed) would do now. Reads
    /// the journal and the default input; changes nothing.
    pub fn recovery_plan(&self) -> RecoveryPlan {
        decide(&self.journal.read(), &self.routing.default_input(), |pid| {
            self.owner_alive(pid)
        })
    }

    /// Apply the system changes since the last call and return what to report.
    /// Never changes the system.
    pub fn poll(&mut self) -> Vec<RoutingEvent> {
        let changes = self.routing.poll_changes();
        if changes.is_empty() || self.state.phase == Phase::Idle {
            return Vec::new();
        }
        let world = self.world();
        let mut events = Vec::new();
        for change in &changes {
            let (next, mut produced) = react(&self.state, change, &world);
            self.state = next;
            events.append(&mut produced);
        }
        events
    }

    fn recover_corrupt(&mut self, reason: String, restore: bool) -> RecoveryReport {
        let preserved_at = match self.journal.quarantine(self.system.unix_secs()) {
            Ok(path) => Some(path),
            Err(e) => {
                tracing::warn!(error = %e.0, "could not move the unusable routing journal aside");
                None
            }
        };
        let restored = if restore {
            match self.restore_default(None) {
                Ok(r) => Some(r),
                Err(error) => return RecoveryReport::Failed { error },
            }
        } else {
            None
        };
        RecoveryReport::CorruptJournal {
            reason,
            preserved_at,
            restored,
        }
    }

    /// Restore the default input (our microphone is the default now) and read it back.
    fn restore_default(&self, saved: Option<&DeviceRef>) -> Result<Restored, CallModeError> {
        let devices = self.devices()?;
        let current = self
            .routing
            .default_input()
            .map_err(CallModeError::Routing)?;
        let plan =
            plan_restore(saved, &devices, Some(&current)).ok_or(CallModeError::NoRestoreTarget)?;
        self.routing
            .set_default_input(&plan.target.uid)
            .map_err(CallModeError::RestoreFailed)?;
        let actual = self.routing.default_input().ok();
        if actual.as_ref() != Some(&plan.target.uid) {
            return Err(CallModeError::RestoreNotApplied {
                expected: plan.target.uid,
                actual,
            });
        }
        Ok(Restored {
            to: plan.target,
            reason: plan.reason,
        })
    }

    /// Undo a failed entry: if the default input is ours, restore it; delete
    /// the journal unless the default input is still ours.
    fn abort_enter(&self, saved: Option<&DeviceRef>) {
        let still_ours = matches!(self.routing.default_input(), Ok(d) if d.as_str() == OWN_MIC_UID);
        if still_ours && let Err(error) = self.restore_default(saved) {
            tracing::error!(%error, "could not undo a failed switch; the journal stays for recovery");
            return;
        }
        self.clear_journal();
    }

    fn clear_journal(&self) -> bool {
        match self.journal.remove() {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(error = %e.0, "could not delete the routing journal");
                false
            }
        }
    }

    fn owner_alive(&self, pid: u32) -> bool {
        pid != self.system.own_pid() && self.system.process_alive(pid)
    }

    fn devices(&self) -> Result<Vec<DeviceEntry>, CallModeError> {
        self.system.devices().map_err(CallModeError::Routing)
    }

    fn world(&self) -> World {
        let default_input = self.routing.default_input().ok();
        match self.system.devices() {
            Ok(devices) => {
                let target =
                    plan_restore(self.state.saved.as_ref(), &devices, default_input.as_ref())
                        .map(|p| p.target);
                World {
                    default_input,
                    own_mic_usable: own_mic_usable(&devices),
                    restore_target: target,
                }
            }
            // The list is unreadable: no evidence that anything vanished.
            Err(_) => World {
                default_input,
                own_mic_usable: self.state.phase != Phase::MicLost,
                restore_target: self.state.restore_target.clone(),
            },
        }
    }
}

fn own_mic_usable(devices: &[DeviceEntry]) -> bool {
    devices
        .iter()
        .any(|d| d.uid == OWN_MIC_UID && d.has_input && d.is_alive)
}

fn name_of(devices: &[DeviceEntry], uid: &DeviceUid) -> String {
    devices
        .iter()
        .find(|d| d.uid == uid.as_str())
        .map_or_else(|| uid.as_str().to_owned(), |d| d.name.clone())
}

#[cfg(test)]
mod tests;
