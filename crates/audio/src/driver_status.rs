//! Reports the state of the virtual microphone driver: bundle installed, variant, version, loaded by the audio daemon, hidden sink present.
//!
//! The assessment ([`DriverStatus::assess`], [`DriverStatus::check`]) is pure:
//! it takes the bundle's metadata and a device list. The thin shell
//! ([`DriverStatus::detect`]) reads the installed `Info.plist` and asks the
//! device directory. Call mode readiness and the virtual-microphone endpoint
//! both use the same verdicts, [`DriverIssue`].
//!
//! The driver's transport variant comes from two places that can disagree after
//! a half-finished update: the installed bundle's `PrxTransportVariant` key and
//! the transport type the loaded device reports now. The live value wins when
//! present, because that is what other applications see.

use std::path::{Path, PathBuf};

use crate::device_directory::{DeviceEntry, find_by_uid, list_devices_with};
use crate::driver_plist::{PlistError, parse_plist};
use crate::input_policy::{InputTransport, OWN_MIC_SINK_UID, OWN_MIC_UID};

/// The driver version this app expects, as the part of the bundle's
/// `CFBundleShortVersionString` before the `-<variant>` suffix.
///
/// Read at build time from `driver/macos/VERSION` (this crate's `build.rs`), the
/// file the driver's own build stamps its bundle from, so the number is written
/// in one place only.
pub const EXPECTED_DRIVER_VERSION: &str = env!("PRXVC_DRIVER_VERSION");

/// Where the installer puts the bundle.
pub const DRIVER_BUNDLE_PATH: &str = "/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver";

/// How the driver presents itself to the system (build-time choice).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DriverVariant {
    /// Core Audio "virtual" transport.
    Virtual,
    /// Reports a USB transport (the variant that carried speech in a call).
    Usb,
    /// Reports a built-in transport.
    BuiltIn,
    /// Any other transport value.
    Other,
}

impl DriverVariant {
    /// The name the driver's build script and `Info.plist` use.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Virtual => "virtual",
            Self::Usb => "usb",
            Self::BuiltIn => "builtin",
            Self::Other => "other",
        }
    }

    fn parse(name: &str) -> Self {
        match name.trim() {
            "virtual" => Self::Virtual,
            "usb" => Self::Usb,
            "builtin" => Self::BuiltIn,
            _ => Self::Other,
        }
    }

    fn from_transport(t: InputTransport) -> Option<Self> {
        match t {
            InputTransport::Virtual => Some(Self::Virtual),
            InputTransport::Usb => Some(Self::Usb),
            InputTransport::BuiltIn => Some(Self::BuiltIn),
            InputTransport::Unknown => None,
            _ => Some(Self::Other),
        }
    }
}

impl std::fmt::Display for DriverVariant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the installed bundle's `Info.plist` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BundleInfo {
    /// `CFBundleShortVersionString` as written, e.g. `0.1.0-usb`.
    pub version_string: String,
    /// The version without the variant suffix, e.g. `0.1.0`.
    pub version: String,
    /// `PrxTransportVariant`; without the key, the suffix of the version
    /// string; without either, `virtual` (bundles older than the variants).
    pub variant: DriverVariant,
    /// `CFBundleVersion`, when present.
    pub build: Option<String>,
    /// `CFBundleIdentifier`, when present.
    pub identifier: Option<String>,
}

/// The bundle on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleState {
    /// No bundle at the install path.
    Missing,
    /// The bundle's metadata could not be read; the text says why.
    Unreadable(String),
    /// Installed.
    Installed(BundleInfo),
}

/// Parse the bytes of an installed `Info.plist`.
///
/// # Errors
/// [`PlistError::Binary`] for a binary plist, [`PlistError::Malformed`] for
/// bad text or a missing `CFBundleShortVersionString`.
pub fn parse_bundle_info(plist: &[u8]) -> Result<BundleInfo, PlistError> {
    let strings = parse_plist(plist)?;
    let version_string = strings
        .get("CFBundleShortVersionString")
        .ok_or(PlistError::Malformed(
            "CFBundleShortVersionString is missing",
        ))?
        .trim()
        .to_owned();
    let (version, suffix) = match version_string.split_once('-') {
        Some((v, s)) => (v.to_owned(), Some(s)),
        None => (version_string.clone(), None),
    };
    let variant = strings
        .get("PrxTransportVariant")
        .or(suffix)
        .map_or(DriverVariant::Virtual, DriverVariant::parse);
    Ok(BundleInfo {
        version_string,
        version,
        variant,
        build: strings.get("CFBundleVersion").map(str::to_owned),
        identifier: strings.get("CFBundleIdentifier").map(str::to_owned),
    })
}

/// What stands between the app and a working virtual microphone.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DriverIssue {
    /// No driver bundle is installed.
    #[error("the PRX Voice microphone driver is not installed")]
    DriverMissing,
    /// The bundle exists but its metadata is unreadable.
    #[error("the installed driver's metadata is unreadable: {0}")]
    BundleUnreadable(String),
    /// Installed, but the audio daemon has not loaded the microphone or its sink.
    #[error("the driver is installed but the audio service has not loaded `{uid}`")]
    NotLoaded {
        /// The UID that is absent.
        uid: String,
    },
    /// The device is present but reports itself not alive.
    #[error("the driver device `{uid}` is not alive")]
    NotAlive {
        /// The UID of the device.
        uid: String,
    },
    /// The installed version differs from the one this app expects.
    #[error("the installed driver is {installed}, this app expects {bundled}")]
    DriverStale {
        /// Installed version (without the variant suffix).
        installed: String,
        /// Version the app expects.
        bundled: String,
    },
    /// The installed variant is not the one the call app needs.
    #[error("the installed driver variant is {installed}, {required} is required")]
    WrongVariant {
        /// Installed variant.
        installed: DriverVariant,
        /// Required variant.
        required: DriverVariant,
    },
}

/// A snapshot of the driver's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DriverStatus {
    /// The bundle on disk.
    pub bundle: BundleState,
    /// The audio daemon lists the microphone (by UID).
    pub microphone_loaded: bool,
    /// The microphone reports itself alive.
    pub microphone_alive: bool,
    /// The hidden sink is listed (by UID).
    pub sink_present: bool,
    /// The sink reports itself alive.
    pub sink_alive: bool,
    /// Variant read from the live transport type; `None` when not loaded or
    /// the platform does not say.
    pub live_variant: Option<DriverVariant>,
    /// The version the app expects ([`EXPECTED_DRIVER_VERSION`]).
    pub expected_version: String,
}

impl DriverStatus {
    /// Combine the bundle state and a device list into a status.
    pub fn assess(bundle: BundleState, devices: &[DeviceEntry], expected_version: &str) -> Self {
        let mic = find_by_uid(devices, OWN_MIC_UID);
        let sink = find_by_uid(devices, OWN_MIC_SINK_UID);
        Self {
            bundle,
            microphone_loaded: mic.is_some(),
            microphone_alive: mic.is_some_and(|d| d.is_alive),
            sink_present: sink.is_some(),
            sink_alive: sink.is_some_and(|d| d.is_alive),
            live_variant: mic.and_then(|d| DriverVariant::from_transport(d.transport)),
            expected_version: expected_version.to_owned(),
        }
    }

    /// Read the installed bundle and the live devices of this machine.
    ///
    /// A device list that cannot be read counts as "nothing loaded" and is
    /// not an error here; the verdict then says [`DriverIssue::NotLoaded`].
    pub fn detect() -> Self {
        Self::detect_at(Path::new(DRIVER_BUNDLE_PATH))
    }

    /// [`detect`](Self::detect) with the bundle at `bundle_dir`.
    pub fn detect_at(bundle_dir: &Path) -> Self {
        let devices = list_devices_with(&[OWN_MIC_UID, OWN_MIC_SINK_UID]).unwrap_or_default();
        Self::assess(read_bundle(bundle_dir), &devices, EXPECTED_DRIVER_VERSION)
    }

    /// The variant applications see: the live one, else the bundle's.
    pub fn variant(&self) -> Option<DriverVariant> {
        self.live_variant.or(match &self.bundle {
            BundleState::Installed(info) => Some(info.variant),
            _ => None,
        })
    }

    /// The installed version without the variant suffix, if a bundle is readable.
    pub fn installed_version(&self) -> Option<&str> {
        match &self.bundle {
            BundleState::Installed(info) => Some(info.version.as_str()),
            _ => None,
        }
    }

    /// Whether the installed version equals the expected one.
    pub fn version_matches(&self) -> bool {
        self.installed_version() == Some(self.expected_version.as_str())
    }

    /// The first problem that keeps the microphone from being usable, in the
    /// order a person would fix them: install, readable, loaded, alive,
    /// current, right variant. `required` is the variant the call app needs
    /// (`None` accepts any).
    ///
    /// # Errors
    /// The [`DriverIssue`] found.
    pub fn check(&self, required: Option<DriverVariant>) -> Result<(), DriverIssue> {
        let info = match &self.bundle {
            BundleState::Missing => return Err(DriverIssue::DriverMissing),
            BundleState::Unreadable(why) => return Err(DriverIssue::BundleUnreadable(why.clone())),
            BundleState::Installed(info) => info,
        };
        for (loaded, alive, uid) in [
            (self.microphone_loaded, self.microphone_alive, OWN_MIC_UID),
            (self.sink_present, self.sink_alive, OWN_MIC_SINK_UID),
        ] {
            if !loaded {
                return Err(DriverIssue::NotLoaded {
                    uid: uid.to_owned(),
                });
            }
            if !alive {
                return Err(DriverIssue::NotAlive {
                    uid: uid.to_owned(),
                });
            }
        }
        if info.version != self.expected_version {
            return Err(DriverIssue::DriverStale {
                installed: info.version.clone(),
                bundled: self.expected_version.clone(),
            });
        }
        if let (Some(required), Some(installed)) = (required, self.variant())
            && required != installed
        {
            return Err(DriverIssue::WrongVariant {
                installed,
                required,
            });
        }
        Ok(())
    }
}

fn read_bundle(dir: &Path) -> BundleState {
    let plist: PathBuf = dir.join("Contents").join("Info.plist");
    if !dir.exists() {
        return BundleState::Missing;
    }
    match std::fs::read(&plist) {
        Err(e) => BundleState::Unreadable(format!("{}: {e}", plist.display())),
        Ok(bytes) => match parse_bundle_info(&bytes) {
            Ok(info) => BundleState::Installed(info),
            Err(e) => BundleState::Unreadable(e.to_string()),
        },
    }
}

#[cfg(test)]
mod tests;
