//! Tests of the driver status on parsed metadata and fixed device lists; no hardware is queried.

use super::*;

fn plist(version: &str, variant_key: Option<&str>) -> String {
    let variant = variant_key
        .map(|v| format!("<key>PrxTransportVariant</key><string>{v}</string>"))
        .unwrap_or_default();
    format!(
        "<plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>org.example.driver</string>\
         <key>CFBundleShortVersionString</key><string>{version}</string>\
         <key>CFBundleVersion</key><string>100</string>{variant}</dict></plist>"
    )
}

fn installed(version: &str, variant_key: Option<&str>) -> BundleState {
    BundleState::Installed(parse_bundle_info(plist(version, variant_key).as_bytes()).unwrap())
}

fn device(uid: &str, transport: InputTransport, alive: bool) -> DeviceEntry {
    DeviceEntry {
        uid: uid.to_owned(),
        name: uid.to_owned(),
        transport,
        has_input: uid == OWN_MIC_UID,
        has_output: uid != OWN_MIC_UID,
        is_alive: alive,
        is_hidden: uid != OWN_MIC_UID,
    }
}

fn both(transport: InputTransport) -> Vec<DeviceEntry> {
    vec![
        device(OWN_MIC_UID, transport, true),
        device(OWN_MIC_SINK_UID, transport, true),
    ]
}

#[test]
fn version_and_variant_come_from_the_plist() {
    let info = parse_bundle_info(plist("0.1.0-usb", Some("usb")).as_bytes()).unwrap();
    assert_eq!(info.version_string, "0.1.0-usb");
    assert_eq!(info.version, "0.1.0");
    assert_eq!(info.identifier.as_deref(), Some("org.example.driver"));
    assert_eq!(info.variant, DriverVariant::Usb);
    assert_eq!(info.build.as_deref(), Some("100"));
}

#[test]
fn the_variant_falls_back_to_the_version_suffix_then_to_virtual() {
    let by_suffix = parse_bundle_info(plist("0.1.0-builtin", None).as_bytes()).unwrap();
    assert_eq!(by_suffix.variant, DriverVariant::BuiltIn);
    let old = parse_bundle_info(plist("0.0.9", None).as_bytes()).unwrap();
    assert_eq!(
        (old.variant, old.version.as_str()),
        (DriverVariant::Virtual, "0.0.9")
    );
    // The key wins over a disagreeing suffix.
    let both = parse_bundle_info(plist("0.1.0-usb", Some("virtual")).as_bytes()).unwrap();
    assert_eq!(both.variant, DriverVariant::Virtual);
}

#[test]
fn an_unknown_variant_name_is_other_and_a_missing_version_is_an_error() {
    let info = parse_bundle_info(plist("1.0.0", Some("pci")).as_bytes()).unwrap();
    assert_eq!(info.variant, DriverVariant::Other);
    let no_version = b"<plist><dict><key>CFBundleName</key><string>x</string></dict></plist>";
    assert!(matches!(
        parse_bundle_info(no_version),
        Err(PlistError::Malformed(_))
    ));
}

#[test]
fn a_binary_plist_makes_the_bundle_unreadable_not_missing() {
    assert_eq!(parse_bundle_info(b"bplist00\x01"), Err(PlistError::Binary));
}

#[test]
fn a_healthy_usb_install_passes_every_check() {
    let status = DriverStatus::assess(
        installed("0.1.0-usb", Some("usb")),
        &both(InputTransport::Usb),
        "0.1.0",
    );
    assert!(status.microphone_loaded && status.sink_present && status.version_matches());
    assert_eq!(status.variant(), Some(DriverVariant::Usb));
    assert_eq!(status.check(None), Ok(()));
    assert_eq!(status.check(Some(DriverVariant::Usb)), Ok(()));
}

#[test]
fn no_bundle_is_driver_missing() {
    let status = DriverStatus::assess(BundleState::Missing, &[], "0.1.0");
    assert_eq!(status.check(None), Err(DriverIssue::DriverMissing));
    assert!(!status.microphone_loaded && status.variant().is_none());
}

#[test]
fn an_installed_bundle_the_daemon_has_not_loaded_is_not_loaded() {
    let status = DriverStatus::assess(installed("0.1.0-usb", None), &[], "0.1.0");
    assert_eq!(
        status.check(None),
        Err(DriverIssue::NotLoaded {
            uid: OWN_MIC_UID.to_owned()
        })
    );
    let only_mic = vec![device(OWN_MIC_UID, InputTransport::Usb, true)];
    let status = DriverStatus::assess(installed("0.1.0-usb", None), &only_mic, "0.1.0");
    assert_eq!(
        status.check(None),
        Err(DriverIssue::NotLoaded {
            uid: OWN_MIC_SINK_UID.to_owned()
        })
    );
}

#[test]
fn a_dead_device_is_reported() {
    let mut devices = both(InputTransport::Usb);
    devices[1].is_alive = false;
    let status = DriverStatus::assess(installed("0.1.0-usb", None), &devices, "0.1.0");
    assert_eq!(
        status.check(None),
        Err(DriverIssue::NotAlive {
            uid: OWN_MIC_SINK_UID.to_owned()
        })
    );
}

#[test]
fn any_version_difference_is_stale() {
    for found in ["0.0.9-usb", "0.2.0-usb"] {
        let status =
            DriverStatus::assess(installed(found, None), &both(InputTransport::Usb), "0.1.0");
        assert!(!status.version_matches());
        assert_eq!(
            status.check(None),
            Err(DriverIssue::DriverStale {
                installed: found.trim_end_matches("-usb").to_owned(),
                bundled: "0.1.0".to_owned()
            })
        );
    }
}

#[test]
fn the_required_variant_is_checked_against_the_live_transport_first() {
    // The bundle says virtual, the loaded device still reports USB (an update that
    // has not been picked up): applications see USB.
    let status = DriverStatus::assess(
        installed("0.1.0-virtual", Some("virtual")),
        &both(InputTransport::Usb),
        "0.1.0",
    );
    assert_eq!(status.variant(), Some(DriverVariant::Usb));
    assert_eq!(status.check(Some(DriverVariant::Usb)), Ok(()));
    assert_eq!(
        status.check(Some(DriverVariant::Virtual)),
        Err(DriverIssue::WrongVariant {
            installed: DriverVariant::Usb,
            required: DriverVariant::Virtual
        })
    );
}

#[test]
fn without_a_live_transport_the_bundle_variant_is_used() {
    let status = DriverStatus::assess(
        installed("0.1.0-virtual", None),
        &both(InputTransport::Unknown),
        "0.1.0",
    );
    assert_eq!(status.variant(), Some(DriverVariant::Virtual));
    assert_eq!(
        status.check(Some(DriverVariant::Usb)),
        Err(DriverIssue::WrongVariant {
            installed: DriverVariant::Virtual,
            required: DriverVariant::Usb
        })
    );
}

#[test]
fn detect_at_distinguishes_missing_unreadable_and_installed() {
    let dir = std::env::temp_dir().join(format!("prxvc-driver-status-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(read_bundle(&dir), BundleState::Missing);
    std::fs::create_dir_all(dir.join("Contents")).unwrap();
    assert!(matches!(read_bundle(&dir), BundleState::Unreadable(_)));
    std::fs::write(dir.join("Contents/Info.plist"), b"bplist00").unwrap();
    assert!(matches!(read_bundle(&dir), BundleState::Unreadable(why) if why.contains("binary")));
    std::fs::write(
        dir.join("Contents/Info.plist"),
        plist("0.1.0-usb", Some("usb")),
    )
    .unwrap();
    assert!(
        matches!(read_bundle(&dir), BundleState::Installed(i) if i.variant == DriverVariant::Usb)
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_expected_version_is_a_plain_triple() {
    assert_eq!(EXPECTED_DRIVER_VERSION.split('.').count(), 3);
    assert!(!EXPECTED_DRIVER_VERSION.contains('-'));
}

#[test]
fn the_expected_version_is_the_one_in_the_driver_version_file() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../driver/macos/VERSION");
    let file = std::fs::read_to_string(path).unwrap();
    assert_eq!(EXPECTED_DRIVER_VERSION, file.trim());
}
