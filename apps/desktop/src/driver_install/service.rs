//! App wiring of the driver operations: UI commands, background execution, state and events.
//!
//! The UI pushes `RefreshDriverStatus`, `InstallDriver` or `UninstallDriver` through the
//! usual command path (`commands.rs` hands them to [`handle_command`]). Install and
//! uninstall wait for the user at the administrator prompt, so they run on their own
//! thread; the outcome is stored in a process-wide tracker and reported as an
//! `EventSnapshot` of type `DriverInstall`, never silently. The UI package reads
//! `snapshot` and renders the typed error code with translated text.

use parking_lot::Mutex;
use prxvc_desktop_ui::{EventSnapshot, SharedAppState};

use super::error::DriverInstallError;
use super::model::{BUNDLED_SUBPATH, DriverStatus};
use super::ops::{DriverInstaller, InstallOutcome, UninstallOutcome};
use super::system::{MacSystem, OsascriptRunner};
use crate::bundle;

/// UI command: re-read bundled and installed driver (no prompt, no audio query).
pub(crate) const CMD_REFRESH: &str = "RefreshDriverStatus";
/// UI command: install or update the bundled driver (administrator prompt).
pub(crate) const CMD_INSTALL: &str = "InstallDriver";
/// UI command: remove the installed driver (administrator prompt).
pub(crate) const CMD_UNINSTALL: &str = "UninstallDriver";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriverOperation {
    Install,
    Uninstall,
}

/// Where the last or running operation stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ActionState {
    Idle,
    Working(DriverOperation),
    Succeeded(DriverOperation),
    Cancelled(DriverOperation),
    Failed {
        operation: DriverOperation,
        code: &'static str,
        detail: String,
    },
}

/// What the UI reads: the operation state and the last status read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverSnapshot {
    pub action: ActionState,
    pub status: Option<DriverStatus>,
}

/// The state shared between the command thread, the worker and the UI.
pub(crate) struct Tracker(Mutex<DriverSnapshot>);

impl Tracker {
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(DriverSnapshot {
            action: ActionState::Idle,
            status: None,
        }))
    }

    /// Enter `Working` unless an operation is already running.
    pub(crate) fn begin(&self, operation: DriverOperation) -> bool {
        let mut state = self.0.lock();
        if matches!(state.action, ActionState::Working(_)) {
            return false;
        }
        state.action = ActionState::Working(operation);
        true
    }

    pub(crate) fn finish(
        &self,
        operation: DriverOperation,
        result: Result<(), &DriverInstallError>,
    ) {
        self.0.lock().action = match result {
            Ok(()) => ActionState::Succeeded(operation),
            Err(DriverInstallError::Cancelled) => ActionState::Cancelled(operation),
            Err(e) => ActionState::Failed {
                operation,
                code: e.code(),
                detail: e.to_string(),
            },
        };
    }

    pub(crate) fn set_status(&self, status: DriverStatus) {
        self.0.lock().status = Some(status);
    }

    pub(crate) fn snapshot(&self) -> DriverSnapshot {
        self.0.lock().clone()
    }
}

static TRACKER: Tracker = Tracker::new();

/// The current state for the UI.
/// Read the driver's status now (from files only) and return it with the last operation's
/// state. The settings window shows this through `view::driver_view`.
pub(crate) fn refresh_status() -> DriverSnapshot {
    TRACKER.set_status(installer().status());
    TRACKER.snapshot()
}

/// Handle a driver command from the UI; false when `cmd` is not one of them.
pub(crate) fn handle_command(shared: &SharedAppState, cmd: &str) -> bool {
    match cmd {
        CMD_REFRESH => {
            let status = installer().status();
            publish(shared, &describe_status(&status));
            TRACKER.set_status(status);
        }
        CMD_INSTALL => start(shared, DriverOperation::Install),
        CMD_UNINSTALL => start(shared, DriverOperation::Uninstall),
        _ => return false,
    }
    true
}

fn start(shared: &SharedAppState, operation: DriverOperation) {
    if !cfg!(target_os = "macos") {
        let error = DriverInstallError::Unsupported;
        TRACKER.finish(operation, Err(&error));
        publish(shared, &describe_failure(operation, &error));
        return;
    }
    if !TRACKER.begin(operation) {
        publish(shared, "another driver operation is still running");
        return;
    }
    publish(
        shared,
        &format!("{operation:?}: waiting for the administrator prompt"),
    );
    let shared = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("driver-install".into())
        .spawn({
            let shared = shared.clone();
            move || run_operation(&shared, operation)
        });
    if let Err(e) = spawned {
        let error = DriverInstallError::CopyFailed {
            step: "thread".into(),
            detail: e.to_string(),
        };
        TRACKER.finish(operation, Err(&error));
        publish(&shared, &describe_failure(operation, &error));
    }
}

fn run_operation(shared: &SharedAppState, operation: DriverOperation) {
    let installer = installer();
    let (result, message) = match operation {
        DriverOperation::Install => match installer.install() {
            Ok(outcome) => (Ok(()), describe_install(&outcome)),
            Err(e) => {
                let message = describe_failure(operation, &e);
                (Err(e), message)
            }
        },
        DriverOperation::Uninstall => match installer.uninstall() {
            Ok(outcome) => (Ok(()), describe_uninstall(outcome)),
            Err(e) => {
                let message = describe_failure(operation, &e);
                (Err(e), message)
            }
        },
    };
    TRACKER.set_status(installer.status());
    TRACKER.finish(operation, result.as_ref().map(|_| ()));
    match &result {
        Ok(()) => tracing::info!("{message}"),
        Err(DriverInstallError::Cancelled) => tracing::info!("{message}"),
        Err(_) => tracing::warn!("{message}"),
    }
    publish(shared, &message);
}

fn installer() -> DriverInstaller<MacSystem, OsascriptRunner> {
    let bundled = bundle::resources_dir(bundle::current()).map(|dir| dir.join(BUNDLED_SUBPATH));
    DriverInstaller::new(MacSystem, OsascriptRunner, bundled)
}

fn publish(shared: &SharedAppState, detail: &str) {
    shared.push_event(EventSnapshot {
        event_type: "DriverInstall".into(),
        detail: detail.to_owned(),
        timestamp: chrono::Utc::now().to_rfc3339(),
    });
}

fn describe_status(status: &DriverStatus) -> String {
    format!(
        "status: bundled {:?}, installed {:?}, action {:?}",
        status.bundled, status.installed, status.action
    )
}

fn describe_install(outcome: &InstallOutcome) -> String {
    format!(
        "driver installed ({:?}): version {}, variant {}",
        outcome.action,
        outcome.installed.version,
        outcome.installed.variant.as_str()
    )
}

fn describe_uninstall(outcome: UninstallOutcome) -> String {
    match outcome {
        UninstallOutcome::Removed => "driver removed".to_owned(),
        UninstallOutcome::NotInstalled => "driver was not installed".to_owned(),
    }
}

fn describe_failure(operation: DriverOperation, error: &DriverInstallError) -> String {
    format!(
        "{operation:?} failed [{}]: {error}. {}",
        error.code(),
        error.advice()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_operation_runs_at_a_time() {
        let tracker = Tracker::new();
        assert!(tracker.begin(DriverOperation::Install));
        assert!(!tracker.begin(DriverOperation::Uninstall));
        tracker.finish(DriverOperation::Install, Ok(()));
        assert_eq!(
            tracker.snapshot().action,
            ActionState::Succeeded(DriverOperation::Install)
        );
        assert!(tracker.begin(DriverOperation::Uninstall));
    }

    #[test]
    fn outcomes_are_recorded_by_kind() {
        let tracker = Tracker::new();
        tracker.finish(
            DriverOperation::Install,
            Err(&DriverInstallError::Cancelled),
        );
        assert_eq!(
            tracker.snapshot().action,
            ActionState::Cancelled(DriverOperation::Install)
        );
        tracker.finish(
            DriverOperation::Uninstall,
            Err(&DriverInstallError::DeviceStillPresent),
        );
        assert_eq!(
            tracker.snapshot().action,
            ActionState::Failed {
                operation: DriverOperation::Uninstall,
                code: "device_still_present",
                detail: "the device is still listed after removal".into(),
            }
        );
    }

    #[test]
    fn failure_message_carries_code_reason_and_advice() {
        let message = describe_failure(
            DriverOperation::Install,
            &DriverInstallError::DeviceDidNotAppear,
        );
        assert!(message.contains("[device_did_not_appear]"));
        assert!(message.contains("did not appear"));
        assert!(message.contains("Restart the Mac"));
    }

    #[test]
    fn unknown_commands_are_not_handled() {
        let shared = SharedAppState::new();
        assert!(!handle_command(&shared, "StartSession"));
        assert!(shared.take_events().is_empty());
    }

    #[test]
    fn refresh_publishes_one_event_without_a_prompt() {
        let shared = SharedAppState::new();
        assert!(handle_command(&shared, CMD_REFRESH));
        let events = shared.take_events();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "DriverInstall");
        assert!(events[0].detail.starts_with("status:"));
        assert!(TRACKER.snapshot().status.is_some());
    }
}
