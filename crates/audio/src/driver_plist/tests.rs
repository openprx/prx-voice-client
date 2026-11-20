//! Tests of the property-list reader on text fixtures, including the installed driver's own file.

use super::*;

/// The `Info.plist` of the installed `0.1.0-usb` bundle.
const USB_PLIST: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AudioServerPlugIn_MachServices</key>
	<array/>
	<key>CFBundleIdentifier</key>
	<string>org.openprx.prx-voice-client.microphone-driver</string>
	<key>CFBundleShortVersionString</key>
	<string>0.1.0-usb</string>
	<key>CFBundleVersion</key>
	<string>100</string>
	<key>CFPlugInFactories</key>
	<dict>
		<key>FFF57CFC-E8F2-486A-B820-32B2F2A24E5D</key>
		<string>PrxVoiceMicrophoneDriverCreate</string>
	</dict>
	<key>CFPlugInTypes</key>
	<dict>
		<key>443ABAB8-E7B3-491A-B985-BEB9187030DB</key>
		<array>
			<string>FFF57CFC-E8F2-486A-B820-32B2F2A24E5D</string>
		</array>
	</dict>
	<key>PrxTransportType</key>
	<string>usb </string>
	<key>PrxTransportVariant</key>
	<string>usb</string>
</dict>
</plist>"#;

#[test]
fn reads_the_top_level_strings() {
    let p = parse_plist(USB_PLIST.as_bytes()).unwrap();
    assert_eq!(p.get("CFBundleShortVersionString"), Some("0.1.0-usb"));
    assert_eq!(p.get("CFBundleVersion"), Some("100"));
    assert_eq!(p.get("PrxTransportVariant"), Some("usb"));
    assert_eq!(p.get("PrxTransportType"), Some("usb "));
}

#[test]
fn nested_values_are_not_top_level() {
    let p = parse_plist(USB_PLIST.as_bytes()).unwrap();
    assert_eq!(p.get("FFF57CFC-E8F2-486A-B820-32B2F2A24E5D"), None);
    assert_eq!(p.get("443ABAB8-E7B3-491A-B985-BEB9187030DB"), None);
}

#[test]
fn a_value_that_is_not_a_string_does_not_shift_the_pairs() {
    let text = "<plist><dict><key>A</key><true/><key>B</key><integer>3</integer>\
                <key>C</key><string>c</string></dict></plist>";
    let p = parse_plist(text.as_bytes()).unwrap();
    assert_eq!(
        (p.get("A"), p.get("B"), p.get("C")),
        (None, None, Some("c"))
    );
}

#[test]
fn entities_comments_and_empty_strings() {
    let text = "<plist><dict><!-- note --><key>A</key><string>x &amp; y &lt;1&gt;</string>\
                <key>B</key><string/></dict></plist>";
    let p = parse_plist(text.as_bytes()).unwrap();
    assert_eq!(p.get("A"), Some("x & y <1>"));
    assert_eq!(p.get("B"), Some(""));
}

#[test]
fn a_binary_plist_is_reported_as_such() {
    let mut data = b"bplist00".to_vec();
    data.extend_from_slice(&[0xd1, 0x01, 0x02]);
    assert_eq!(parse_plist(&data), Err(PlistError::Binary));
}

#[test]
fn malformed_text_is_an_error() {
    for text in [
        "",
        "hello",
        "<plist><dict><key>A</key>",
        "<plist><dict></dict></dict></plist>",
    ] {
        assert!(
            matches!(parse_plist(text.as_bytes()), Err(PlistError::Malformed(_))),
            "{text:?}"
        );
    }
    assert!(matches!(
        parse_plist(&[0xff, 0xfe, 0x00]),
        Err(PlistError::Malformed(_))
    ));
}

#[test]
fn an_empty_dictionary_parses_to_no_entries() {
    let p = parse_plist(b"<plist><dict/></plist>");
    assert!(p.is_ok());
}
