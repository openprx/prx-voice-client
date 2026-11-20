//! Virtual microphone driver management from the app: compare, install, update and uninstall.
//!
//! The app bundle carries `PrxVoiceMicrophone.driver` (usb variant) under
//! `Contents/Resources/Drivers/`. This module decides what is needed (bundled
//! against installed version and variant), copies the bundle to
//! `/Library/Audio/Plug-Ins/HAL/` through the system administrator prompt, restarts the
//! audio daemon, and verifies that the device appears. Design: pipeline-design 8.2.
//!
//! Layering: `model` (values and decisions) and `command` (the exact privileged command
//! strings and the parsing of failures) are pure and unit tested; `ops` runs the
//! operations against two small traits (`SystemProbe`, `PrivilegedRunner`) with fakes in
//! the tests; `system` holds the real macOS implementations of those traits; `service`
//! is the app wiring (commands from the UI, background execution, state and events).
//!
//! Nothing here runs a privileged command in a test, and the real runner is reached only
//! through the `InstallDriver` and `UninstallDriver` UI commands.

mod command;
mod error;
mod model;
mod ops;
mod service;
mod system;

pub(crate) use service::handle_command;
