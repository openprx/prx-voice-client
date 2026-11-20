//! Tests of the call-activity judgement on recorded fixtures, the merge of process objects, the debouncer and the watcher.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::watcher::{Debouncer, WatcherCore};
use super::*;

const OUR_UID: &str = "org.openprx.prx-voice-client.microphone";
const WHATSAPP: &str = "net.whatsapp.WhatsApp";

fn device(object_id: u32, uid: &str, name: &str, input: u32, output: u32) -> ProcessDevice {
    ProcessDevice { object_id, uid: uid.into(), name: name.into(), input_channels: input, output_channels: output }
}

fn our_mic() -> ProcessDevice {
    device(120, OUR_UID, "PRX Voice Microphone", 1, 0)
}

fn speakers() -> ProcessDevice {
    device(80, "BuiltInSpeakerDevice", "MacBook Pro Speakers", 0, 2)
}

fn builtin_mic() -> ProcessDevice {
    device(75, "BuiltInMicrophoneDevice", "MacBook Pro Microphone", 1, 0)
}

fn app(input: bool, output: bool, inputs: Vec<ProcessDevice>, outputs: Vec<ProcessDevice>) -> AppAudioActivity {
    AppAudioActivity {
        bundle_id: WHATSAPP.into(),
        present: true,
        pid: Some(23453),
        audio_running: input || output,
        input_running: input,
        output_running: output,
        input_devices: inputs,
        output_devices: outputs,
    }
}

/// Recorded: WhatsApp running and idle (procaudio, 2026-11-20 18:00).
fn fixture_idle() -> AppAudioActivity {
    app(false, false, vec![], vec![])
}

/// Recorded: a voice message recorded from our microphone (callapp REPORT, section J).
fn fixture_voice_message_on_our_microphone() -> AppAudioActivity {
    app(true, false, vec![our_mic()], vec![])
}

/// Recorded: the live call of 2026-11-20 (callapp REPORT, section I): input on
/// our microphone plus the speakers as the echo reference, output on the speakers.
fn fixture_live_call() -> AppAudioActivity {
    app(true, true, vec![our_mic(), speakers()], vec![speakers()])
}

/// A call on the built-in microphone, with the same echo-reference entry.
fn fixture_call_on_builtin_microphone() -> AppAudioActivity {
    app(true, true, vec![builtin_mic(), speakers()], vec![speakers()])
}

#[test]
fn idle_application_is_not_a_call() {
    let j = judge_call(&fixture_idle(), OUR_UID);
    assert_eq!(j, CallJudgement::Idle);
    assert!(!j.call_active() && !j.uses_our_microphone());
}

#[test]
fn voice_message_on_our_microphone_counts_as_active_on_it() {
    let j = judge_call(&fixture_voice_message_on_our_microphone(), OUR_UID);
    assert_eq!(j, CallJudgement::ActiveOnOurMicrophone { output_running: false });
    assert!(j.call_active() && j.uses_our_microphone());
}

#[test]
fn live_call_fixture_is_active_on_our_microphone_despite_the_echo_reference() {
    let j = judge_call(&fixture_live_call(), OUR_UID);
    assert_eq!(j, CallJudgement::ActiveOnOurMicrophone { output_running: true });
}

#[test]
fn live_call_without_our_microphone_in_the_list_is_not_ours_and_the_speakers_are_ignored() {
    // The same call with the echo reference only: nothing is a microphone.
    let a = app(true, true, vec![speakers()], vec![speakers()]);
    assert_eq!(judge_call(&a, OUR_UID), CallJudgement::ActiveOnOtherMicrophone { devices: vec![] });
}

#[test]
fn call_on_the_builtin_microphone_is_reported_separately_with_the_microphone_named() {
    let j = judge_call(&fixture_call_on_builtin_microphone(), OUR_UID);
    assert_eq!(j, CallJudgement::ActiveOnOtherMicrophone { devices: vec![builtin_mic()] });
    assert!(j.call_active() && !j.uses_our_microphone());
}

#[test]
fn application_not_running_is_not_running() {
    let j = judge_call(&AppAudioActivity::absent(WHATSAPP), OUR_UID);
    assert_eq!(j, CallJudgement::NotRunning);
    assert!(!j.call_active());
}

#[test]
fn input_running_with_an_empty_device_list_is_a_call_on_an_unknown_microphone() {
    let a = app(true, true, vec![], vec![speakers()]);
    assert_eq!(judge_call(&a, OUR_UID), CallJudgement::ActiveOnOtherMicrophone { devices: vec![] });
}

#[test]
fn our_microphone_is_never_taken_for_an_echo_reference_even_when_it_can_render() {
    // A loopback style device has output streams too.
    let ours = device(120, OUR_UID, "PRX Voice Microphone", 1, 1);
    let a = app(true, true, vec![ours.clone(), speakers()], vec![speakers(), ours]);
    assert_eq!(judge_call(&a, OUR_UID), CallJudgement::ActiveOnOurMicrophone { output_running: true });
}

#[test]
fn a_headset_used_for_input_and_output_is_ignored_as_a_microphone_but_the_call_is_not_ours() {
    let headset = device(90, "AirPods", "AirPods", 1, 1);
    let a = app(true, true, vec![headset.clone()], vec![headset]);
    let j = judge_call(&a, OUR_UID);
    assert_eq!(j, CallJudgement::ActiveOnOtherMicrophone { devices: vec![] });
    assert!(!j.uses_our_microphone());
}

#[test]
fn an_input_entry_equal_to_an_output_device_is_ignored_even_if_its_streams_are_unknown() {
    let in_entry = device(80, "BuiltInSpeakerDevice", "MacBook Pro Speakers", 0, 0);
    let a = app(true, true, vec![builtin_mic(), in_entry], vec![speakers()]);
    assert_eq!(judge_call(&a, OUR_UID), CallJudgement::ActiveOnOtherMicrophone { devices: vec![builtin_mic()] });
}

#[test]
fn an_empty_our_uid_never_matches() {
    let unnamed = device(1, "", "Some device", 1, 0);
    let a = app(true, false, vec![unnamed.clone()], vec![]);
    assert_eq!(judge_call(&a, ""), CallJudgement::ActiveOnOtherMicrophone { devices: vec![unnamed] });
}

#[test]
fn process_objects_of_one_application_merge() {
    let mut main = app(false, true, vec![], vec![speakers()]);
    let mut helper = app(true, false, vec![our_mic()], vec![speakers()]);
    helper.pid = Some(99);
    main.merge(helper);
    assert!(main.input_running && main.output_running && main.audio_running);
    assert_eq!(main.pid, Some(23453));
    assert_eq!(main.input_devices, vec![our_mic()]);
    assert_eq!(main.output_devices, vec![speakers()], "the same device is not listed twice");
}

#[test]
fn device_label_falls_back_to_the_uid() {
    assert_eq!(speakers().label(), "MacBook Pro Speakers");
    assert_eq!(device(1, "uid-1", "", 0, 0).label(), "uid-1");
}

// ---------------------------------------------------------------------------
// Debouncer and watcher core, with an injected clock
// ---------------------------------------------------------------------------

fn at(base: Instant, ms: u64) -> Instant {
    base + Duration::from_millis(ms)
}

#[test]
fn debouncer_reports_the_first_value_at_once() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(1));
    assert_eq!(d.feed(t0, 'a'), Some('a'));
    assert_eq!(d.feed(at(t0, 1_000), 'a'), None);
}

#[test]
fn debouncer_holds_a_change_until_it_persists() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(1));
    d.feed(t0, 'a');
    assert_eq!(d.feed(at(t0, 1_000), 'b'), None);
    assert_eq!(d.feed(at(t0, 1_999), 'b'), None);
    assert_eq!(d.feed(at(t0, 2_000), 'b'), Some('b'));
    assert_eq!(d.feed(at(t0, 3_000), 'b'), None);
}

#[test]
fn debouncer_drops_a_flap_and_restarts_the_hold_for_a_different_candidate() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::from_secs(1));
    d.feed(t0, 'a');
    assert_eq!(d.feed(at(t0, 1_000), 'b'), None);
    // Back to the reported value before the hold: nothing is ever reported.
    assert_eq!(d.feed(at(t0, 1_500), 'a'), None);
    assert_eq!(d.feed(at(t0, 2_000), 'b'), None);
    // A different candidate restarts the hold.
    assert_eq!(d.feed(at(t0, 2_500), 'c'), None);
    assert_eq!(d.feed(at(t0, 3_400), 'c'), None);
    assert_eq!(d.feed(at(t0, 3_500), 'c'), Some('c'));
}

#[test]
fn debouncer_with_zero_hold_reports_every_change() {
    let t0 = Instant::now();
    let mut d = Debouncer::new(Duration::ZERO);
    assert_eq!(d.feed(t0, 1), Some(1));
    assert_eq!(d.feed(t0, 2), Some(2));
}

struct Script(VecDeque<Result<AppAudioActivity, ActivityError>>);

impl ActivitySource for Script {
    fn read(&mut self) -> Result<AppAudioActivity, ActivityError> {
        match self.0.len() {
            0 => Err(ActivityError::Unsupported),
            1 => self.0[0].clone(),
            _ => self.0.pop_front().unwrap_or(Err(ActivityError::Unsupported)),
        }
    }
}

fn script(items: Vec<Result<AppAudioActivity, ActivityError>>) -> Box<Script> {
    Box::new(Script(items.into()))
}

#[test]
fn watcher_core_reports_the_baseline_then_a_debounced_call_start_and_end() {
    let t0 = Instant::now();
    let mut core = WatcherCore::new(
        script(vec![
            Ok(fixture_idle()),
            Ok(fixture_live_call()),
            Ok(fixture_live_call()),
            Ok(fixture_live_call()),
            Ok(fixture_idle()),
            Ok(fixture_idle()),
        ]),
        OUR_UID,
        Duration::from_secs(1),
    );
    let judgements: Vec<_> = (0..6u64)
        .map(|i| core.tick(at(t0, i * 1_000)))
        .map(|e| match e {
            Some(CallActivityEvent::Changed { judgement, .. }) => Some(judgement),
            Some(other) => panic!("unexpected {other:?}"),
            None => None,
        })
        .collect();
    assert_eq!(
        judgements,
        vec![
            Some(CallJudgement::Idle),
            None,
            Some(CallJudgement::ActiveOnOurMicrophone { output_running: true }),
            None,
            None,
            Some(CallJudgement::Idle),
        ]
    );
}

#[test]
fn watcher_core_does_not_report_a_one_poll_blip() {
    let t0 = Instant::now();
    let mut core = WatcherCore::new(
        script(vec![Ok(fixture_idle()), Ok(fixture_live_call()), Ok(fixture_idle()), Ok(fixture_idle())]),
        OUR_UID,
        Duration::from_secs(1),
    );
    let events: Vec<_> = (0..4u64).filter_map(|i| core.tick(at(t0, i * 1_000))).collect();
    assert_eq!(events.len(), 1, "only the baseline: {events:?}");
}

#[test]
fn watcher_core_reports_loss_of_detection_and_its_return() {
    let t0 = Instant::now();
    let mut core = WatcherCore::new(
        script(vec![Ok(fixture_idle()), Err(ActivityError::Unsupported), Err(ActivityError::Unsupported), Ok(fixture_idle()), Ok(fixture_idle())]),
        OUR_UID,
        Duration::from_secs(1),
    );
    let events: Vec<_> = (0..5u64).filter_map(|i| core.tick(at(t0, i * 1_000))).collect();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[1], CallActivityEvent::Unavailable { error: ActivityError::Unsupported }));
    assert!(matches!(&events[2], CallActivityEvent::Changed { judgement: CallJudgement::Idle, .. }));
}

#[test]
fn watcher_thread_delivers_events_and_stops_on_drop() {
    let source = script(vec![Ok(fixture_live_call())]);
    let config = WatcherConfig {
        our_microphone_uid: OUR_UID.into(),
        poll_interval: Duration::from_millis(1),
        hold: Duration::ZERO,
    };
    let watcher = CallActivityWatcher::spawn(source, config).expect("thread starts");
    let event = watcher.events().recv_timeout(Duration::from_secs(10)).expect("baseline event");
    assert!(matches!(
        event,
        CallActivityEvent::Changed { judgement: CallJudgement::ActiveOnOurMicrophone { output_running: true }, .. }
    ));
    assert_eq!(watcher.latest(), Some(event));
    assert_eq!(watcher.dropped_events(), 0);
    drop(watcher); // joins the thread: the test would hang here if it did not stop
}
