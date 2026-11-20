//! The real macOS implementations of the driver operations' system access and admin prompt.
//!
//! `MacSystem` reads property lists, runs `codesign --verify` and asks `system_profiler`
//! whether the device is listed. `OsascriptRunner` shows the system administrator prompt
//! through `osascript` and is reached only from the `InstallDriver` and `UninstallDriver`
//! commands; no test calls it.

use std::io;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use super::command::ADMIN_WRAPPER_SCRIPT;
use super::model::{DEVICE_NAME, parse_info};
use super::ops::{BundleRead, PrivilegedRunner, RunOutput, SystemProbe};

/// Read-only system queries.
pub(crate) struct MacSystem;

/// Runs a shell command as root after the system password prompt.
pub(crate) struct OsascriptRunner;

/// The XML text of `<bundle>/Contents/Info.plist`; a binary plist is converted by `plutil`.
fn read_plist_xml(bundle: &Path) -> io::Result<String> {
    let plist = bundle.join("Contents/Info.plist");
    let bytes = std::fs::read(&plist)?;
    if bytes.starts_with(b"bplist") {
        let out = Command::new("/usr/bin/plutil")
            .args(["-convert", "xml1", "-o", "-"])
            .arg(&plist)
            .output()?;
        if !out.status.success() {
            return Err(io::Error::other(
                "plutil could not convert the property list",
            ));
        }
        return String::from_utf8(out.stdout).map_err(io::Error::other);
    }
    String::from_utf8(bytes).map_err(io::Error::other)
}

/// Whether `system_profiler SPAudioDataType` output lists the microphone as a device
/// (a line that is exactly the device name and a colon).
pub(crate) fn profiler_lists_device(output: &str) -> bool {
    let wanted = format!("{DEVICE_NAME}:");
    output.lines().any(|line| line.trim() == wanted)
}

impl SystemProbe for MacSystem {
    fn read_bundle(&self, bundle: &Path) -> BundleRead {
        match read_plist_xml(bundle) {
            Ok(xml) => parse_info(&xml).map_or(BundleRead::Unreadable, BundleRead::Info),
            Err(e) if e.kind() == io::ErrorKind::NotFound => BundleRead::Missing,
            Err(_) => BundleRead::Unreadable,
        }
    }

    fn executable_exists(&self, executable: &Path) -> bool {
        executable.is_file()
    }

    fn verify_signature(&self, bundle: &Path) -> Result<(), String> {
        let out = Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict"])
            .arg(bundle)
            .output()
            .map_err(|e| format!("cannot run codesign: {e}"))?;
        if out.status.success() {
            return Ok(());
        }
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(stderr
            .lines()
            .next()
            .unwrap_or("codesign failed")
            .trim()
            .to_owned())
    }

    fn device_present(&self) -> Result<bool, String> {
        let out = Command::new("/usr/sbin/system_profiler")
            .arg("SPAudioDataType")
            .output()
            .map_err(|e| format!("cannot run system_profiler: {e}"))?;
        if !out.status.success() {
            return Err("system_profiler failed".to_owned());
        }
        Ok(profiler_lists_device(&String::from_utf8_lossy(&out.stdout)))
    }

    fn pause(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

impl PrivilegedRunner for OsascriptRunner {
    fn run(&self, shell_command: &str) -> io::Result<RunOutput> {
        let out = Command::new("/usr/bin/osascript")
            .arg("-e")
            .arg(ADMIN_WRAPPER_SCRIPT)
            .arg(shell_command)
            .output()?;
        Ok(RunOutput {
            success: out.status.success(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver_install::model::{DRIVER_BUNDLE_ID, DriverInfo, Variant};

    const PLIST: &str = "<?xml version=\"1.0\"?><plist><dict>\
        <key>CFBundleIdentifier</key><string>org.openprx.prx-voice-client.microphone-driver</string>\
        <key>CFBundleShortVersionString</key><string>0.1.0-usb</string>\
        <key>PrxTransportVariant</key><string>usb</string></dict></plist>";

    fn bundle_with(plist: &[u8]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("Contents")).expect("mkdir");
        std::fs::write(dir.path().join("Contents/Info.plist"), plist).expect("write");
        dir
    }

    #[test]
    fn reads_an_xml_bundle() {
        let dir = bundle_with(PLIST.as_bytes());
        assert_eq!(
            MacSystem.read_bundle(dir.path()),
            BundleRead::Info(DriverInfo {
                identifier: DRIVER_BUNDLE_ID.into(),
                version: "0.1.0-usb".into(),
                variant: Variant::Usb,
            })
        );
    }

    #[test]
    fn missing_and_garbage_bundles() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            MacSystem.read_bundle(&dir.path().join("nope")),
            BundleRead::Missing
        );
        assert_eq!(MacSystem.read_bundle(dir.path()), BundleRead::Missing);
        let garbage = bundle_with(b"not a plist");
        assert_eq!(
            MacSystem.read_bundle(garbage.path()),
            BundleRead::Unreadable
        );
        let non_utf8 = bundle_with(&[0xff, 0xfe, 0x00]);
        assert_eq!(
            MacSystem.read_bundle(non_utf8.path()),
            BundleRead::Unreadable
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn reads_a_binary_plist_through_plutil() {
        let dir = bundle_with(PLIST.as_bytes());
        let plist = dir.path().join("Contents/Info.plist");
        let status = Command::new("/usr/bin/plutil")
            .args(["-convert", "binary1"])
            .arg(&plist)
            .status()
            .expect("plutil");
        assert!(status.success());
        assert!(std::fs::read(&plist).expect("read").starts_with(b"bplist"));
        assert!(matches!(
            MacSystem.read_bundle(dir.path()),
            BundleRead::Info(_)
        ));
    }

    #[test]
    fn profiler_output_parsing() {
        let listed = "Audio:\n\n    Devices:\n\n        MacBook Pro Microphone:\n\n          Input Channels: 1\n\n        PRX Voice Microphone:\n\n          Input Channels: 1\n";
        assert!(profiler_lists_device(listed));
        assert!(!profiler_lists_device("        MacBook Pro Microphone:\n"));
        assert!(!profiler_lists_device(
            "        PRX Voice Microphone (Internal Sink):\n"
        ));
        assert!(!profiler_lists_device(""));
    }
}
