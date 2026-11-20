//! The driver's state as the settings window shows it: installed or not, what it needs, and the outcome of the last operation.
//!
//! Pure mapping from the module's own types ([`DriverStatus`], [`ActionState`]) to the plain view
//! model of `prxvc-desktop-ui`; the status itself is read from files only (no audio query, no
//! privileged command, no prompt).

use prxvc_desktop_ui::settings_data::{
    DriverOperationKind, DriverOperationView, DriverStateView, DriverView,
};

use super::model::{BundledDriver, DriverAction, DriverStatus, InstalledDriver};
use super::service::{ActionState, DriverOperation, DriverSnapshot, refresh_status};

/// Read the driver's status now and describe it with the outcome of the last operation.
pub(crate) fn driver_view() -> DriverView {
    view_of(&refresh_status(), cfg!(target_os = "macos"))
}

/// Describe a snapshot. `supported` is false on platforms without a driver.
pub(crate) fn view_of(snapshot: &DriverSnapshot, supported: bool) -> DriverView {
    let operation = operation_view(&snapshot.action);
    let Some(status) = &snapshot.status else {
        return DriverView {
            state: if supported {
                DriverStateView::Unknown
            } else {
                DriverStateView::Unsupported
            },
            operation,
            ..DriverView::default()
        };
    };
    DriverView {
        state: if supported {
            state_of(status)
        } else {
            DriverStateView::Unsupported
        },
        bundle_available: matches!(status.bundled, BundledDriver::Present(_)),
        installed_version: match &status.installed {
            InstalledDriver::Present(info) => Some(info.version.clone()),
            _ => None,
        },
        installed_variant: match &status.installed {
            InstalledDriver::Present(info) => Some(info.variant.as_str().to_owned()),
            _ => None,
        },
        operation,
    }
}

/// What the installed driver needs. The installed state decides first: the app's own copy
/// being absent (not run from the bundle) does not make an installed driver "not installed".
fn state_of(status: &DriverStatus) -> DriverStateView {
    match (&status.installed, status.action) {
        (InstalledDriver::Absent, _) => DriverStateView::NotInstalled,
        (InstalledDriver::Unreadable, _) | (_, DriverAction::Repair) => {
            DriverStateView::NeedsRepair
        }
        (_, DriverAction::Update) => DriverStateView::UpdateAvailable,
        (_, DriverAction::ChangeVariant) => DriverStateView::WrongVariant,
        (_, DriverAction::InstalledIsNewer) => DriverStateView::InstalledIsNewer,
        (_, DriverAction::UpToDate | DriverAction::BundleUnavailable | DriverAction::Install) => {
            DriverStateView::Installed
        }
    }
}

fn kind(operation: DriverOperation) -> DriverOperationKind {
    match operation {
        DriverOperation::Install => DriverOperationKind::Install,
        DriverOperation::Uninstall => DriverOperationKind::Uninstall,
    }
}

fn operation_view(action: &ActionState) -> DriverOperationView {
    match action {
        ActionState::Idle => DriverOperationView::Idle,
        ActionState::Working(op) => DriverOperationView::Working(kind(*op)),
        ActionState::Succeeded(op) => DriverOperationView::Succeeded(kind(*op)),
        ActionState::Cancelled(op) => DriverOperationView::Cancelled(kind(*op)),
        ActionState::Failed {
            operation, code, ..
        } => DriverOperationView::Failed {
            operation: kind(*operation),
            code: (*code).to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::error::DriverInstallError;
    use super::super::model::{DriverInfo, Variant};
    use super::*;

    fn info(version: &str, variant: Variant) -> DriverInfo {
        DriverInfo {
            identifier: "id".into(),
            version: version.into(),
            variant,
        }
    }

    fn snapshot(bundled: BundledDriver, installed: InstalledDriver) -> DriverSnapshot {
        DriverSnapshot {
            action: ActionState::Idle,
            status: Some(DriverStatus::new(bundled, installed)),
        }
    }

    #[test]
    fn every_state_of_the_installed_driver_has_a_view() {
        let bundled = BundledDriver::Present(info("1.2.0-usb", Variant::Usb));
        let cases = [
            (InstalledDriver::Absent, DriverStateView::NotInstalled),
            (InstalledDriver::Unreadable, DriverStateView::NeedsRepair),
            (
                InstalledDriver::Present(info("1.1.0-usb", Variant::Usb)),
                DriverStateView::UpdateAvailable,
            ),
            (
                InstalledDriver::Present(info("1.2.0-virtual", Variant::Virtual)),
                DriverStateView::WrongVariant,
            ),
            (
                InstalledDriver::Present(info("1.2.0-usb", Variant::Usb)),
                DriverStateView::Installed,
            ),
            (
                InstalledDriver::Present(info("1.3.0-usb", Variant::Usb)),
                DriverStateView::InstalledIsNewer,
            ),
        ];
        for (installed, expected) in cases {
            let view = view_of(&snapshot(bundled.clone(), installed.clone()), true);
            assert_eq!(view.state, expected, "{installed:?}");
            assert!(view.bundle_available);
        }
    }

    #[test]
    fn an_installed_driver_stays_installed_when_the_app_carries_no_copy() {
        let view = view_of(
            &snapshot(
                BundledDriver::Missing,
                InstalledDriver::Present(info("1.2.0-usb", Variant::Usb)),
            ),
            true,
        );
        assert_eq!(view.state, DriverStateView::Installed);
        assert!(!view.bundle_available);
        assert_eq!(view.installed_version.as_deref(), Some("1.2.0-usb"));
        assert_eq!(view.installed_variant.as_deref(), Some("usb"));
        let view = view_of(
            &snapshot(BundledDriver::Missing, InstalledDriver::Absent),
            true,
        );
        assert_eq!(view.state, DriverStateView::NotInstalled);
        assert!(!view.bundle_available, "install stays disabled");
    }

    #[test]
    fn before_the_first_probe_the_state_is_unknown_and_without_a_driver_platform_unsupported() {
        let blank = DriverSnapshot {
            action: ActionState::Idle,
            status: None,
        };
        assert_eq!(view_of(&blank, true).state, DriverStateView::Unknown);
        assert_eq!(view_of(&blank, false).state, DriverStateView::Unsupported);
        let some = snapshot(BundledDriver::Missing, InstalledDriver::Absent);
        assert_eq!(view_of(&some, false).state, DriverStateView::Unsupported);
    }

    #[test]
    fn each_operation_outcome_maps_with_the_typed_error_code() {
        let mut snap = snapshot(BundledDriver::Missing, InstalledDriver::Absent);
        snap.action = ActionState::Working(DriverOperation::Install);
        assert_eq!(
            view_of(&snap, true).operation,
            DriverOperationView::Working(DriverOperationKind::Install)
        );
        snap.action = ActionState::Succeeded(DriverOperation::Uninstall);
        assert_eq!(
            view_of(&snap, true).operation,
            DriverOperationView::Succeeded(DriverOperationKind::Uninstall)
        );
        snap.action = ActionState::Cancelled(DriverOperation::Install);
        assert_eq!(
            view_of(&snap, true).operation,
            DriverOperationView::Cancelled(DriverOperationKind::Install)
        );
        for error in [
            DriverInstallError::BundleNotFound,
            DriverInstallError::SourceRejected("x".into()),
            DriverInstallError::AuthorizationFailed("x".into()),
            DriverInstallError::CopyFailed {
                step: "stage".into(),
                detail: "x".into(),
            },
            DriverInstallError::RestartFailed("x".into()),
            DriverInstallError::DeviceDidNotAppear,
            DriverInstallError::DeviceStillPresent,
            DriverInstallError::Unsupported,
        ] {
            snap.action = ActionState::Failed {
                operation: DriverOperation::Install,
                code: error.code(),
                detail: error.to_string(),
            };
            assert_eq!(
                view_of(&snap, true).operation,
                DriverOperationView::Failed {
                    operation: DriverOperationKind::Install,
                    code: error.code().to_owned()
                }
            );
        }
    }
}
