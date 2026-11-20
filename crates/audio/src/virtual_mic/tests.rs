//! Endpoint tests without hardware: device planning on fixed lists, the format check, and the sink behaviour through a detached playback sink.

use prxvc_media::{MonoTime, SampleRate};

use super::*;
use crate::driver_status::{BundleInfo, BundleState};
use crate::input_policy::{InputTransport, OWN_MIC_UID};

fn device(uid: &str, name: &str, input: bool, output: bool) -> DeviceEntry {
    DeviceEntry {
        uid: uid.to_owned(),
        name: name.to_owned(),
        transport: InputTransport::Usb,
        has_input: input,
        has_output: output,
        is_alive: true,
        is_hidden: false,
    }
}

fn driver_devices() -> Vec<DeviceEntry> {
    vec![
        device(OWN_MIC_UID, "PRX Voice Microphone", true, false),
        device(
            OWN_MIC_SINK_UID,
            "PRX Voice Microphone (Internal Sink)",
            false,
            true,
        ),
        device("BlackHole2ch_UID", "BlackHole 2ch", true, true),
    ]
}

fn healthy_status(devices: &[DeviceEntry]) -> DriverStatus {
    let bundle = BundleState::Installed(BundleInfo {
        version_string: "0.1.0-usb".to_owned(),
        version: "0.1.0".to_owned(),
        variant: DriverVariant::Usb,
        build: None,
    });
    DriverStatus::assess(bundle, devices, "0.1.0")
}

fn frame(samples: usize) -> AudioFrame {
    AudioFrame::new(vec![0.25; samples], SampleRate::HZ_48000, MonoTime::ZERO, 0).unwrap()
}

#[test]
fn empty_uid_means_our_sink_and_a_uid_is_trimmed() {
    let o = VirtualMicOptions::from_config("  ", None);
    assert_eq!(
        (o.effective_uid(), o.is_own_driver()),
        (OWN_MIC_SINK_UID, true)
    );
    let o = VirtualMicOptions::from_config(" BlackHole2ch_UID ", None);
    assert_eq!(
        (o.effective_uid(), o.is_own_driver()),
        ("BlackHole2ch_UID", false)
    );
}

#[test]
fn our_sink_is_planned_by_uid_not_by_name() {
    let devices = driver_devices();
    let planned = plan_device(
        &VirtualMicOptions::default(),
        &healthy_status(&devices),
        &devices,
    )
    .unwrap();
    assert_eq!(planned.uid, OWN_MIC_SINK_UID);
    assert_eq!(planned.name, "PRX Voice Microphone (Internal Sink)");
}

#[test]
fn the_driver_checks_run_for_our_sink() {
    let devices = driver_devices();
    let missing = DriverStatus::assess(BundleState::Missing, &[], "0.1.0");
    let r = plan_device(&VirtualMicOptions::default(), &missing, &devices);
    assert!(matches!(
        r,
        Err(VirtualMicError::Driver(DriverIssue::DriverMissing))
    ));

    let stale = DriverStatus::assess(healthy_status(&devices).bundle, &devices, "0.2.0");
    let r = plan_device(&VirtualMicOptions::default(), &stale, &devices);
    assert!(matches!(
        r,
        Err(VirtualMicError::Driver(DriverIssue::DriverStale { .. }))
    ));

    let options = VirtualMicOptions {
        sink_uid: String::new(),
        required_variant: Some(DriverVariant::Virtual),
    };
    let r = plan_device(&options, &healthy_status(&devices), &devices);
    assert!(matches!(
        r,
        Err(VirtualMicError::Driver(DriverIssue::WrongVariant {
            installed: DriverVariant::Usb,
            required: DriverVariant::Virtual
        }))
    ));
}

#[test]
fn a_third_party_uid_skips_the_driver_checks_and_needs_an_output() {
    let devices = driver_devices();
    let missing_driver = DriverStatus::assess(BundleState::Missing, &[], "0.1.0");
    let options = VirtualMicOptions::from_config("BlackHole2ch_UID", Some(DriverVariant::Virtual));
    let planned = plan_device(&options, &missing_driver, &devices).unwrap();
    assert_eq!(planned.name, "BlackHole 2ch");

    let absent = VirtualMicOptions::from_config("nothing", None);
    assert!(matches!(
        plan_device(&absent, &missing_driver, &devices),
        Err(VirtualMicError::DeviceMissing { uid }) if uid == "nothing"
    ));
    // An input-only device is not an output.
    let input_only = VirtualMicOptions::from_config(OWN_MIC_UID, None);
    assert!(matches!(
        plan_device(&input_only, &missing_driver, &devices),
        Err(VirtualMicError::DeviceMissing { .. })
    ));
}

#[test]
fn a_name_that_is_only_similar_never_matches() {
    let devices = vec![device(
        "x.sink.copy",
        "PRX Voice Microphone (Internal Sink)",
        false,
        true,
    )];
    let status = healthy_status(&driver_devices());
    assert!(plan_device(&VirtualMicOptions::default(), &status, &devices).is_err());
}

#[test]
fn only_our_sink_must_be_48k_mono() {
    let own = VirtualMicOptions::default();
    assert!(check_format(&own, 48_000, 1).is_ok());
    for (rate, channels) in [(44_100, 1), (48_000, 2)] {
        assert!(matches!(
            check_format(&own, rate, channels),
            Err(VirtualMicError::UnexpectedFormat { .. })
        ));
    }
    assert!(check_format(&VirtualMicOptions::from_config("x", None), 44_100, 2).is_ok());
}

#[test]
fn wrap_rejects_a_wrong_format_for_our_sink() {
    let r = VirtualMicSink::wrap(
        &VirtualMicOptions::default(),
        PlaybackSink::detached(48_000, 2),
    );
    assert!(matches!(
        r,
        Err(VirtualMicError::UnexpectedFormat {
            rate: 48_000,
            channels: 2
        })
    ));
}

#[test]
fn the_endpoint_queues_flushes_and_drains_through_the_playback_path() {
    let mut mic = VirtualMicSink::wrap(
        &VirtualMicOptions::default(),
        PlaybackSink::detached(48_000, 1),
    )
    .unwrap();
    assert_eq!(mic.uid(), OWN_MIC_SINK_UID);
    let outcome = mic.write(&frame(4_800));
    assert_eq!((outcome.accepted, outcome.dropped), (4_800, 0));
    assert_eq!(mic.queued(), Duration::from_millis(100));
    assert!(!mic.is_drained());
    // The device callback takes 40 ms.
    let mut out = vec![0.0; 1_920];
    mic.playback().buffer().pull(&mut out);
    assert_eq!(mic.queued(), Duration::from_millis(60));
    assert!(out.iter().all(|s| *s == 0.25));
    mic.flush();
    assert!(mic.is_drained() && mic.wait_drained(Duration::from_millis(1)));
    mic.end_stream();
    assert_eq!(mic.stats(), OutputStats::default());
    assert!(mic.health().running);
    mic.stop();
    assert!(!mic.health().running);
}

#[test]
fn an_unstarted_endpoint_drops_writes() {
    let sink = PlaybackSink::new(
        48_000,
        1,
        1,
        Box::new(|_, _| {
            Err(EndpointError::Unavailable {
                reason: "driver gone".to_owned(),
            })
        }),
    )
    .unwrap();
    let mut mic = VirtualMicSink::wrap(&VirtualMicOptions::default(), sink).unwrap();
    assert!(matches!(
        mic.start(),
        Err(EndpointError::Unavailable { .. })
    ));
    assert_eq!(mic.write(&frame(480)).dropped, 480);
    assert_eq!(
        mic.health().last_error.as_deref(),
        Some("endpoint is not started")
    );
    assert_eq!(mic.health().dropped_samples, 480);
}
