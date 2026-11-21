//! Live check of the digital-zero watchdog: a tap without audio-capture consent on a playing application reports the silence event.
//!
//! Ignored by default (code standard CS-5.2.1: it needs the application, audio
//! playing in it and a missing consent). It runs the real tap without the
//! consent gate, so the system delivers exact zeros, and expects
//! `TapEvent::SilentWhilePlaying`, then `TapEvent::NoSignal` after the rebuild.
//!
//! Environment:
//! * `PRXVC_LIVE_TAP_TARGET`: bundle ID of an application that is playing audio
//!   now (required).
//! * `PRXVC_LIVE_TAP_WAIT_SECS`: how long to wait for each event (default 20).
//!
//! Run it from an application bundle that does NOT hold System Audio Recording
//! consent (the case under test; with consent the tap delivers real audio and no
//! event follows), for example through the bundle wrapper used for the tap probe.
//! It never requests consent and never mutes the target.

#[cfg(target_os = "macos")]
#[test]
#[ignore = "live: needs a playing application and an unconsented bundle (see the module docs)"]
fn live_tap_zero_detect() {
    use std::time::{Duration, Instant};

    use prxvc_audio::core_audio_tap::CoreAudioTap;
    use prxvc_audio::tap_types::{TapConfig, TapError, TapEvent};

    let target = std::env::var("PRXVC_LIVE_TAP_TARGET").expect("set PRXVC_LIVE_TAP_TARGET to a bundle ID");
    let wait = Duration::from_secs(
        std::env::var("PRXVC_LIVE_TAP_WAIT_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(20),
    );
    let (tap, mut frames) = CoreAudioTap::start_without_consent_check(TapConfig::for_bundle(target.clone()))
        .expect("tap starts (is the application running?)");

    let mut saw_silence = false;
    let mut saw_error = false;
    let deadline = Instant::now() + wait * 2;
    while Instant::now() < deadline && !saw_error {
        while frames.try_recv().is_some() {}
        while let Ok(event) = tap.events().try_recv() {
            match event {
                TapEvent::SilentWhilePlaying { .. } => saw_silence = true,
                TapEvent::NoSignal { error } => {
                    assert_eq!(error, TapError::NoSignal { target: target.clone() });
                    saw_error = true;
                }
                TapEvent::SignalRestored => panic!("real audio arrived: the bundle holds consent, nothing to detect"),
                _ => {}
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(saw_silence, "no SilentWhilePlaying event: is {target} playing audio and the bundle unconsented?");
    assert!(saw_error, "no NoSignal event after the rebuild");
}
