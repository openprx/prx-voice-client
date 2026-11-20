//! Driver identity and the pure decision of what action the installed driver needs.

use std::path::{Path, PathBuf};

use prxvc_audio::driver_status::parse_bundle_info;

/// File name of the driver bundle, both inside the app and in the HAL directory.
pub(crate) const DRIVER_BUNDLE_NAME: &str = "PrxVoiceMicrophone.driver";
/// `CFBundleIdentifier` of the driver; a bundle with another identifier is never installed.
pub(crate) const DRIVER_BUNDLE_ID: &str = "org.openprx.prx-voice-client.microphone-driver";
/// Executable inside `Contents/MacOS`.
pub(crate) const DRIVER_EXECUTABLE: &str = "PrxVoiceMicrophoneDriver";
/// The system directory that holds audio server plug-ins.
pub(crate) const HAL_DIR: &str = "/Library/Audio/Plug-Ins/HAL";
/// Where the bundled driver sits below `Contents/Resources`.
pub(crate) const BUNDLED_SUBPATH: &str = "Drivers/PrxVoiceMicrophone.driver";

/// Where the driver is installed.
pub(crate) fn installed_path() -> PathBuf {
    Path::new(HAL_DIR).join(DRIVER_BUNDLE_NAME)
}

/// Hidden staging directory used while installing; removed by install and uninstall.
pub(crate) fn staging_path() -> PathBuf {
    Path::new(HAL_DIR).join(format!(".{DRIVER_BUNDLE_NAME}.staging"))
}

/// Transport type the driver reports to applications (`PrxTransportVariant`); the type and
/// the `Info.plist` reader are the ones of `prxvc-audio`.
pub(crate) use prxvc_audio::driver_status::DriverVariant as Variant;

/// What the `Info.plist` of a driver bundle says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverInfo {
    pub identifier: String,
    /// `CFBundleShortVersionString`, `X.Y.Z-<variant>`.
    pub version: String,
    pub variant: Variant,
}

/// Read identifier, version and variant from the bytes of an XML `Info.plist` with the
/// reader of `prxvc-audio`. A bundle built before variants existed has no
/// `PrxTransportVariant` key and is a virtual one.
///
/// # Errors
/// The reason the bytes do not describe a driver bundle.
pub(crate) fn parse_info(plist: &[u8]) -> Result<DriverInfo, String> {
    let info = parse_bundle_info(plist).map_err(|e| e.to_string())?;
    let identifier = info
        .identifier
        .ok_or_else(|| "CFBundleIdentifier is missing".to_owned())?;
    Ok(DriverInfo {
        identifier,
        version: info.version_string,
        variant: info.variant,
    })
}

/// `(major, minor, patch)` of `X.Y.Z` or `X.Y.Z-suffix`.
pub(crate) fn version_triple(version: &str) -> Option<(u32, u32, u32)> {
    let core = version.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let triple = (
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
        parts.next()?.parse().ok()?,
    );
    parts.next().is_none().then_some(triple)
}

/// The driver payload inside the app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BundledDriver {
    /// The app is not in a bundle, or the payload directory is absent.
    Missing,
    /// Present but not a usable driver bundle (unreadable plist, foreign identifier).
    Unusable,
    Present(DriverInfo),
}

/// The driver in `/Library/Audio/Plug-Ins/HAL`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstalledDriver {
    Absent,
    /// Something sits at our path but is not a readable bundle of ours.
    Unreadable,
    Present(DriverInfo),
}

/// What the user can be offered, given the bundled and the installed driver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DriverAction {
    /// Nothing installed: offer "Install".
    Install,
    /// Installed version is older: offer "Update". Never done silently.
    Update,
    /// Installed variant differs from the bundled one: offer "Update" (replaces the variant).
    ChangeVariant,
    /// The installed bundle cannot be read: offer "Repair" (reinstall).
    Repair,
    /// Installed and bundled agree: nothing to do.
    UpToDate,
    /// The installed driver is newer than the bundled one: nothing is offered.
    InstalledIsNewer,
    /// No usable driver payload in this app: no action is possible.
    BundleUnavailable,
}

/// The decision, pure.
pub(crate) fn decide(bundled: &BundledDriver, installed: &InstalledDriver) -> DriverAction {
    let BundledDriver::Present(bundled) = bundled else {
        return DriverAction::BundleUnavailable;
    };
    let installed = match installed {
        InstalledDriver::Absent => return DriverAction::Install,
        InstalledDriver::Unreadable => return DriverAction::Repair,
        InstalledDriver::Present(info) => info,
    };
    if installed.variant != bundled.variant {
        return DriverAction::ChangeVariant;
    }
    match (
        version_triple(&installed.version),
        version_triple(&bundled.version),
    ) {
        (Some(have), Some(want)) if have < want => DriverAction::Update,
        (Some(have), Some(want)) if have > want => DriverAction::InstalledIsNewer,
        (Some(_), Some(_)) => DriverAction::UpToDate,
        _ if installed.version == bundled.version => DriverAction::UpToDate,
        _ => DriverAction::Update,
    }
}

/// Everything the UI shows about the driver, without touching the audio system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DriverStatus {
    pub bundled: BundledDriver,
    pub installed: InstalledDriver,
    pub action: DriverAction,
}

impl DriverStatus {
    pub(crate) fn new(bundled: BundledDriver, installed: InstalledDriver) -> Self {
        let action = decide(&bundled, &installed);
        Self {
            bundled,
            installed,
            action,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(version: &str, variant: Variant) -> DriverInfo {
        DriverInfo {
            identifier: DRIVER_BUNDLE_ID.into(),
            version: version.into(),
            variant,
        }
    }

    fn present(version: &str, variant: Variant) -> (BundledDriver, InstalledDriver) {
        (
            BundledDriver::Present(info("0.2.0-usb", Variant::Usb)),
            InstalledDriver::Present(info(version, variant)),
        )
    }

    #[test]
    fn decision_table() {
        let bundled = BundledDriver::Present(info("0.2.0-usb", Variant::Usb));
        let cases = [
            (InstalledDriver::Absent, DriverAction::Install),
            (InstalledDriver::Unreadable, DriverAction::Repair),
            (
                InstalledDriver::Present(info("0.1.0-usb", Variant::Usb)),
                DriverAction::Update,
            ),
            (
                InstalledDriver::Present(info("0.2.0-usb", Variant::Usb)),
                DriverAction::UpToDate,
            ),
            (
                InstalledDriver::Present(info("0.10.0-usb", Variant::Usb)),
                DriverAction::InstalledIsNewer,
            ),
            (
                InstalledDriver::Present(info("0.2.0-virtual", Variant::Virtual)),
                DriverAction::ChangeVariant,
            ),
            (
                InstalledDriver::Present(info("9.9.9-virtual", Variant::Virtual)),
                DriverAction::ChangeVariant,
            ),
        ];
        for (installed, want) in cases {
            assert_eq!(decide(&bundled, &installed), want, "{installed:?}");
        }
    }

    #[test]
    fn no_action_without_a_payload() {
        let installed = InstalledDriver::Present(info("0.1.0-usb", Variant::Usb));
        for bundled in [BundledDriver::Missing, BundledDriver::Unusable] {
            assert_eq!(
                decide(&bundled, &installed),
                DriverAction::BundleUnavailable
            );
            assert_eq!(
                decide(&bundled, &InstalledDriver::Absent),
                DriverAction::BundleUnavailable
            );
        }
    }

    #[test]
    fn unparseable_versions_fall_back_to_string_equality() {
        let (bundled, same) = present("0.2.0-usb", Variant::Usb);
        assert_eq!(decide(&bundled, &same), DriverAction::UpToDate);
        let bundled = BundledDriver::Present(info("dev", Variant::Usb));
        let installed = InstalledDriver::Present(info("dev", Variant::Usb));
        assert_eq!(decide(&bundled, &installed), DriverAction::UpToDate);
        let installed = InstalledDriver::Present(info("other", Variant::Usb));
        assert_eq!(decide(&bundled, &installed), DriverAction::Update);
    }

    #[test]
    fn version_triples() {
        assert_eq!(version_triple("0.1.0-usb"), Some((0, 1, 0)));
        assert_eq!(version_triple("1.22.333"), Some((1, 22, 333)));
        assert_eq!(version_triple("1.2"), None);
        assert_eq!(version_triple("1.2.3.4"), None);
        assert_eq!(version_triple("a.b.c"), None);
        assert_eq!(version_triple(""), None);
    }

    fn plist(version: &str, variant: Option<&str>) -> String {
        let variant = variant
            .map(|v| format!("<key>PrxTransportVariant</key>\n<string>{v}</string>"))
            .unwrap_or_default();
        format!(
            "<dict><key>CFBundleIdentifier</key>\n    <string>{DRIVER_BUNDLE_ID}</string>\n\
             <key>CFBundleShortVersionString</key><string> {version} </string>{variant}</dict>"
        )
    }

    #[test]
    fn parses_identifier_version_and_variant() {
        assert_eq!(
            parse_info(plist("0.1.0-usb", Some("usb")).as_bytes()),
            Ok(info("0.1.0-usb", Variant::Usb))
        );
    }

    #[test]
    fn a_bundle_without_the_variant_key_is_virtual() {
        assert_eq!(
            parse_info(plist("0.1.0", None).as_bytes()),
            Ok(info("0.1.0", Variant::Virtual))
        );
    }

    #[test]
    fn rejects_incomplete_plists_and_reads_an_unknown_variant_as_other() {
        assert!(parse_info(b"<dict></dict>").is_err());
        assert!(
            parse_info(b"<dict><key>CFBundleShortVersionString</key><string>0.1.0</string></dict>")
                .unwrap_err()
                .contains("CFBundleIdentifier")
        );
        assert_eq!(
            parse_info(plist("0.1.0", Some("quantum")).as_bytes()),
            Ok(info("0.1.0", Variant::Other))
        );
        assert!(parse_info(b"bplist00").is_err());
    }

    #[test]
    fn variant_names_are_the_driver_build_names() {
        let names: Vec<_> = [
            Variant::Virtual,
            Variant::Usb,
            Variant::BuiltIn,
            Variant::Other,
        ]
        .map(Variant::as_str)
        .into();
        assert_eq!(names, ["virtual", "usb", "builtin", "other"]);
    }

    #[test]
    fn fixed_paths() {
        assert_eq!(
            installed_path(),
            Path::new("/Library/Audio/Plug-Ins/HAL/PrxVoiceMicrophone.driver")
        );
        assert_eq!(
            staging_path(),
            Path::new("/Library/Audio/Plug-Ins/HAL/.PrxVoiceMicrophone.driver.staging")
        );
    }
}
