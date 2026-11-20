//! The driver operations: status, install or update, and uninstall, against injectable system access.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::command::{classify_failure, install_command, uninstall_command, validate_source};
use super::error::DriverInstallError;
use super::model::{
    BundledDriver, DRIVER_BUNDLE_ID, DRIVER_EXECUTABLE, DriverAction, DriverInfo, DriverStatus,
    InstalledDriver, installed_path,
};

/// What reading a bundle's `Info.plist` found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BundleRead {
    Missing,
    Unreadable,
    Info(DriverInfo),
}

/// Result of a command run with administrator rights.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RunOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Read-only system access (and the clock), faked in tests.
pub(crate) trait SystemProbe {
    fn read_bundle(&self, bundle: &Path) -> BundleRead;
    fn executable_exists(&self, bundle: &Path) -> bool;
    /// `codesign --verify --strict` on the bundle; the error carries the reason.
    fn verify_signature(&self, bundle: &Path) -> Result<(), String>;
    /// Whether the audio system lists the microphone device.
    fn device_present(&self) -> Result<bool, String>;
    fn pause(&self, duration: Duration);
}

/// Runs one shell command with administrator rights (the system password prompt).
pub(crate) trait PrivilegedRunner {
    fn run(&self, shell_command: &str) -> io::Result<RunOutput>;
}

/// How long to wait for the device to appear or go away after the audio daemon restarts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Polling {
    pub attempts: u32,
    pub interval: Duration,
}

impl Default for Polling {
    fn default() -> Self {
        Self {
            attempts: 10,
            interval: Duration::from_secs(1),
        }
    }
}

/// What a successful install or update did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstallOutcome {
    /// The action that applied to the state before the operation.
    pub action: DriverAction,
    pub installed: DriverInfo,
}

/// What a successful uninstall did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UninstallOutcome {
    Removed,
    /// Nothing was installed; no prompt was shown.
    NotInstalled,
}

/// Status, install and uninstall of the driver.
pub(crate) struct DriverInstaller<S, R> {
    system: S,
    runner: R,
    bundled: Option<PathBuf>,
    installed: PathBuf,
    team_id: Option<String>,
    polling: Polling,
}

impl<S: SystemProbe, R: PrivilegedRunner> DriverInstaller<S, R> {
    /// `bundled` is the driver payload inside the app, when the app is bundled.
    pub(crate) fn new(system: S, runner: R, bundled: Option<PathBuf>) -> Self {
        Self {
            system,
            runner,
            bundled,
            installed: installed_path(),
            team_id: None,
            polling: Polling::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_polling(mut self, polling: Polling) -> Self {
        self.polling = polling;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_team_id(mut self, team: &str) -> Self {
        self.team_id = Some(team.to_owned());
        self
    }

    fn read_bundled(&self) -> BundledDriver {
        let Some(path) = &self.bundled else {
            return BundledDriver::Missing;
        };
        match self.system.read_bundle(path) {
            BundleRead::Missing => BundledDriver::Missing,
            BundleRead::Unreadable => BundledDriver::Unusable,
            BundleRead::Info(info) if info.identifier == DRIVER_BUNDLE_ID => {
                BundledDriver::Present(info)
            }
            BundleRead::Info(_) => BundledDriver::Unusable,
        }
    }

    fn read_installed(&self) -> InstalledDriver {
        match self.system.read_bundle(&self.installed) {
            BundleRead::Missing => InstalledDriver::Absent,
            BundleRead::Unreadable => InstalledDriver::Unreadable,
            BundleRead::Info(info) if info.identifier == DRIVER_BUNDLE_ID => {
                InstalledDriver::Present(info)
            }
            BundleRead::Info(_) => InstalledDriver::Unreadable,
        }
    }

    /// Bundled against installed, from the files only (no audio system query, no prompt).
    pub(crate) fn status(&self) -> DriverStatus {
        DriverStatus::new(self.read_bundled(), self.read_installed())
    }

    /// Install the bundled driver, or replace the installed one, then verify the device.
    pub(crate) fn install(&self) -> Result<InstallOutcome, DriverInstallError> {
        let status = self.status();
        let bundled = match &status.bundled {
            BundledDriver::Present(info) => info.clone(),
            BundledDriver::Missing => return Err(DriverInstallError::BundleNotFound),
            BundledDriver::Unusable => {
                return Err(DriverInstallError::SourceRejected(
                    "the bundled driver's Info.plist is unreadable or has another identifier"
                        .into(),
                ));
            }
        };
        let source = self
            .bundled
            .as_deref()
            .ok_or(DriverInstallError::BundleNotFound)?;
        let source_text = validate_source(source)?;
        if !self
            .system
            .executable_exists(&source.join("Contents/MacOS").join(DRIVER_EXECUTABLE))
        {
            return Err(DriverInstallError::SourceRejected(
                "the driver executable is missing from the bundle".into(),
            ));
        }
        self.system
            .verify_signature(source)
            .map_err(|why| DriverInstallError::SourceRejected(format!("signature: {why}")))?;

        let command = install_command(source_text, self.team_id.as_deref())?;
        self.run_privileged(&command)?;

        if !self.wait_for_device(true) {
            return Err(DriverInstallError::DeviceDidNotAppear);
        }
        match self.read_installed() {
            InstalledDriver::Present(info)
                if info.version == bundled.version && info.variant == bundled.variant =>
            {
                Ok(InstallOutcome {
                    action: status.action,
                    installed: info,
                })
            }
            other => Err(DriverInstallError::CopyFailed {
                step: "check".into(),
                detail: format!("the installed driver is not the bundled one: {other:?}"),
            }),
        }
    }

    /// Remove the installed driver, then verify that the device is gone.
    pub(crate) fn uninstall(&self) -> Result<UninstallOutcome, DriverInstallError> {
        if self.read_installed() == InstalledDriver::Absent {
            return Ok(UninstallOutcome::NotInstalled);
        }
        self.run_privileged(&uninstall_command())?;
        if self.read_installed() != InstalledDriver::Absent {
            return Err(DriverInstallError::CopyFailed {
                step: "remove".into(),
                detail: "the driver bundle is still in place".into(),
            });
        }
        if !self.wait_for_device(false) {
            return Err(DriverInstallError::DeviceStillPresent);
        }
        Ok(UninstallOutcome::Removed)
    }

    fn run_privileged(&self, command: &str) -> Result<(), DriverInstallError> {
        let output = self.runner.run(command).map_err(|e| {
            DriverInstallError::AuthorizationFailed(format!("cannot start the prompt: {e}"))
        })?;
        if output.success {
            Ok(())
        } else {
            Err(classify_failure(&output.stderr, &output.stdout))
        }
    }

    /// Poll until the device is listed (`want`) or unlisted (`!want`); false on timeout.
    fn wait_for_device(&self, want: bool) -> bool {
        for attempt in 0..self.polling.attempts {
            if attempt > 0 {
                self.system.pause(self.polling.interval);
            }
            if self.system.device_present() == Ok(want) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests;
