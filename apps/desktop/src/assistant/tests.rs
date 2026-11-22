//! The assistant host on a real session loop over the fakes of every port: the orb's states, the transcript, the issues, the History record and the microphone shared with call mode.
//! Nothing here opens a device, loads a model or touches the network.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use prxvc_config::AppConfig;
use prxvc_desktop_ui::call_view::{IssueAction, command as call_command};
use prxvc_desktop_ui::{AssistantIssue, SharedAppState};
use prxvc_media::{AudioFrame, Lang, ManualClock, MonoTime, PcmChunk, SampleRate};
use prxvc_ports::fakes::{
    AsrScript, DialogueScript, FakeAudioSource, FakeDialogue, FakeSpeechSynth, FakeStreamingAsr,
    FakeTraceSink, FakeVadProvider, TtsScript, VadScript,
};
use prxvc_ports::{ConversationId, DialogueError, VoiceSelection};
use prxvc_session::assistant::{AssistantDeps, AssistantParams, OpenGate, ReplyOutput, start};
use prxvc_storage::StorageManager;

use super::host::{AssistantHost, command};
use super::launch::{Launched, Launcher};
use crate::call_compose::CallControl;

const WATCHDOG: Duration = Duration::from_secs(8);

#[derive(Default)]
struct OutputProbe {
    flushes: AtomicUsize,
    drained: AtomicBool,
    ends: AtomicUsize,
}

/// The output: it reports itself drained when the stream ends (a device that plays at once), or
/// holds the reply until the test says so.
struct TestOutput(Arc<OutputProbe>, bool);

impl ReplyOutput for TestOutput {
    fn play(&mut self, _chunk: &PcmChunk) -> Result<(), String> {
        self.0.drained.store(false, Ordering::SeqCst);
        Ok(())
    }
    fn end_stream(&mut self) {
        self.0.ends.fetch_add(1, Ordering::SeqCst);
        if self.1 {
            self.0.drained.store(true, Ordering::SeqCst);
        }
    }
    fn flush(&mut self) {
        self.0.flushes.fetch_add(1, Ordering::SeqCst);
        self.0.drained.store(true, Ordering::SeqCst);
    }
    fn is_drained(&self) -> bool {
        self.0.drained.load(Ordering::SeqCst)
    }
}

/// What a test scripts about the session its launcher starts.
struct Script {
    asr_text: &'static str,
    reply: Vec<&'static str>,
    dialogue_error: Option<DialogueError>,
    /// The output reports "drained" by itself once the reply's audio is complete.
    auto_drain: bool,
}

impl Script {
    fn noon() -> Self {
        Self {
            asr_text: "what time is it",
            reply: vec!["It is noon."],
            dialogue_error: None,
            auto_drain: true,
        }
    }
}

enum Plan {
    Fail(Vec<AssistantIssue>),
    Run(Script),
}

struct TestLauncher {
    plan: Plan,
    out: Arc<OutputProbe>,
    launched: Arc<AtomicUsize>,
}

fn frames(n: usize) -> Vec<AudioFrame> {
    (0..n)
        .map(|i| {
            AudioFrame::new(
                vec![0.1; 512],
                SampleRate::HZ_16000,
                MonoTime::from_millis(i as u64 * 32),
                i as u64,
            )
            .expect("frame")
        })
        .collect()
}

impl Launcher for TestLauncher {
    fn launch(&mut self, _config: &AppConfig) -> Result<Launched, Vec<AssistantIssue>> {
        self.launched.fetch_add(1, Ordering::SeqCst);
        let script = match &self.plan {
            Plan::Fail(issues) => return Err(issues.clone()),
            Plan::Run(s) => s,
        };
        let clock = ManualClock::new();
        let mut dialogue_script = DialogueScript::tokens(script.reply.iter().copied());
        if let Some(e) = &script.dialogue_error {
            dialogue_script = dialogue_script.failing_open(e.clone());
        }
        let dialogue = FakeDialogue::new(clock.clone()).with_default_script(dialogue_script);
        let tts =
            FakeSpeechSynth::new(clock.clone()).with_default_script(TtsScript::chunks(2, 480));
        let vad = FakeVadProvider::new(VadScript::probabilities(
            [(0.0f32, 6usize), (0.9, 24), (0.0, 30)]
                .iter()
                .flat_map(|(p, n)| std::iter::repeat_n(*p, *n)),
        ));
        let asr = FakeStreamingAsr::new(
            clock.clone(),
            AsrScript::new(["what"], script.asr_text).then_round([], ""),
        );
        self.out.drained.store(true, Ordering::SeqCst);
        let deps = AssistantDeps {
            source: Box::new(FakeAudioSource::new(SampleRate::HZ_16000, frames(60))),
            vad: Arc::new(vad),
            asr: Arc::new(asr),
            dialogue: Arc::new(dialogue),
            tts: Arc::new(tts),
            output: Box::new(TestOutput(Arc::clone(&self.out), script.auto_drain)),
            gate: Box::new(OpenGate),
            clock: Arc::new(clock),
            trace: Arc::new(FakeTraceSink::new(false)),
        };
        let params = AssistantParams::new(
            ConversationId::new("host-test"),
            Lang::En,
            VoiceSelection::Preset {
                name: "preset-a".into(),
            },
            Duration::from_millis(600),
        );
        let handle = start(deps, params).expect("the fake session starts");
        Ok(Launched {
            handle,
            devices: None,
            on_end: None,
            language: "en".into(),
        })
    }
}

struct Rig {
    host: AssistantHost,
    shared: SharedAppState,
    call: CallControl,
    out: Arc<OutputProbe>,
    launched: Arc<AtomicUsize>,
    dir: tempfile::TempDir,
}

fn config_with(save_history: bool, save_transcript: bool) -> AppConfig {
    let mut c = AppConfig::default();
    c.privacy.save_history = save_history;
    c.privacy.save_transcript = save_transcript;
    c
}

fn rig(plan: Plan, config: AppConfig) -> Rig {
    let shared = SharedAppState::new();
    let call = CallControl::without_coordinator(&shared);
    let dir = tempfile::tempdir().expect("temporary directory");
    let storage = StorageManager::new(dir.path().join("sessions.db")).expect("storage");
    let out = Arc::new(OutputProbe::default());
    let launched = Arc::new(AtomicUsize::new(0));
    let launcher = TestLauncher {
        plan,
        out: Arc::clone(&out),
        launched: Arc::clone(&launched),
    };
    let host = AssistantHost::new(
        shared.clone(),
        call.clone(),
        Some(storage),
        Box::new(launcher),
        Box::new(move || config.clone()),
    );
    Rig {
        host,
        shared,
        call,
        out,
        launched,
        dir,
    }
}

impl Rig {
    /// Take the host's turns until `cond` holds; the watchdog only guards against a hung test.
    fn wait_until(&mut self, what: &str, cond: impl Fn(&Rig) -> bool) {
        let deadline = Instant::now() + WATCHDOG;
        while !cond(self) {
            assert!(
                Instant::now() < deadline,
                "timed out waiting for: {what}\nstate: {:?}\nissue: {:?}",
                self.shared.snapshot_for_ui().session_state,
                self.shared.snapshot_for_ui().assistant_issue
            );
            self.host.tick();
            std::thread::yield_now();
        }
    }

    fn state(&self) -> String {
        self.shared.snapshot_for_ui().session_state
    }

    fn lines(&self) -> Vec<(String, String, bool)> {
        self.shared
            .snapshot_for_ui()
            .transcript_lines
            .into_iter()
            .map(|l| (l.role, l.text, l.is_final))
            .collect()
    }

    fn history(&self) -> Vec<prxvc_storage::SessionRecord> {
        StorageManager::new(self.dir.path().join("sessions.db"))
            .expect("storage")
            .recent_sessions(10)
            .expect("history")
    }

    fn stop_and_wait(&mut self) {
        self.host.command(command::STOP);
        self.wait_until("the session to end", |r| !r.host.is_active());
    }
}

#[test]
fn a_session_that_cannot_start_shows_its_issue_and_gives_the_microphone_back() {
    let issue = AssistantIssue {
        key: "assistant.issue.dialogue_down".into(),
        detail: "the dialogue server at http://127.0.0.1:8080/v1 does not answer".into(),
        action: IssueAction::OpenAssistantSettings,
    };
    let mut r = rig(Plan::Fail(vec![issue.clone()]), AppConfig::default());
    r.host.command(command::START);
    assert_eq!(r.shared.snapshot_for_ui().assistant_issue, Some(issue));
    assert!(!r.host.is_active());
    assert!(
        !r.call.assistant_holds_microphone(),
        "the claim is not kept by a failed start"
    );
    assert!(!r.shared.snapshot_for_ui().session_active);
    // The issue stays until the user closes it.
    r.host.command(prxvc_desktop_ui::DISMISS_ISSUE);
    assert!(r.shared.snapshot_for_ui().assistant_issue.is_none());
}

#[test]
fn a_session_shows_the_turn_and_the_reply_and_is_saved_to_the_history() {
    let mut r = rig(Plan::Run(Script::noon()), config_with(true, true));
    r.host.command(command::CREATE);
    r.host.command(command::START);
    assert!(r.host.is_active());
    assert!(r.call.assistant_holds_microphone());
    assert!(r.shared.snapshot_for_ui().session_active);
    assert!(r.shared.snapshot_for_ui().session_id.is_some());

    r.wait_until("the reply is heard", |r| {
        r.lines()
            .iter()
            .any(|(role, text, fin)| role == "assistant" && *fin && text == "It is noon.")
            && r.state() == "Listening"
    });
    assert_eq!(
        r.lines(),
        [
            ("user".to_owned(), "what time is it".to_owned(), true),
            ("assistant".to_owned(), "It is noon.".to_owned(), true),
        ],
        "the user's line was interim while they spoke and is final now"
    );
    assert!(r.shared.snapshot_for_ui().assistant_issue.is_none());
    assert_eq!(r.launched.load(Ordering::SeqCst), 1);

    r.stop_and_wait();
    let snap = r.shared.snapshot_for_ui();
    assert!(!snap.session_active);
    assert_eq!(snap.session_state, "Idle");
    assert!(
        snap.transcript_lines.is_empty(),
        "the transcript goes with the session"
    );
    assert!(snap.session_id.is_none());
    assert!(
        !r.call.assistant_holds_microphone(),
        "the microphone is free again"
    );
    assert_eq!(
        snap.history_entries.len(),
        1,
        "the History window shows the session at once"
    );
    assert_eq!(snap.history_entries[0].title, "what time is it");
    let stored = r.history();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].turn_count, 1);
    assert_eq!(stored[0].state, "Completed");
    assert_eq!(
        stored[0].transcript_preview.as_deref(),
        Some("what time is it")
    );
}

#[test]
fn history_and_transcript_follow_the_privacy_settings() {
    // History off: nothing is stored.
    let mut r = rig(Plan::Run(Script::noon()), config_with(false, true));
    r.host.command(command::START);
    r.wait_until("a turn", |r| r.lines().len() >= 2);
    r.stop_and_wait();
    assert!(r.history().is_empty());
    assert!(r.shared.snapshot_for_ui().history_entries.is_empty());

    // Transcript off: the session is stored, without a word of it.
    let mut r = rig(Plan::Run(Script::noon()), config_with(true, false));
    r.host.command(command::START);
    r.wait_until("a turn", |r| r.lines().len() >= 2);
    r.stop_and_wait();
    let stored = r.history();
    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].turn_count, 1);
    assert_eq!(stored[0].transcript_preview, None);
    assert!(
        !r.shared.snapshot_for_ui().history_entries[0]
            .title
            .contains("what time")
    );
}

#[test]
fn a_dialogue_server_that_stops_answering_is_an_issue_and_the_session_stays_open() {
    let script = Script {
        dialogue_error: Some(DialogueError::Unavailable {
            reason: "cannot connect: connection refused or host unreachable".into(),
        }),
        ..Script::noon()
    };
    let mut r = rig(Plan::Run(script), config_with(true, true));
    r.host.command(command::START);
    r.wait_until("the issue", |r| {
        r.shared.snapshot_for_ui().assistant_issue.is_some()
    });
    let issue = r.shared.snapshot_for_ui().assistant_issue.expect("issue");
    assert_eq!(issue.key, "assistant.issue.dialogue_down");
    assert_eq!(issue.action, IssueAction::OpenAssistantSettings);
    assert!(issue.detail.contains("connection refused"));
    r.wait_until("listening again", |r| r.state() == "Listening");
    assert!(r.host.is_active(), "a failed turn does not end the session");
    r.stop_and_wait();
    let stored = r.history();
    assert_eq!(
        stored[0].turn_count, 1,
        "the turn that failed is in the record"
    );
}

#[test]
fn the_orb_cuts_the_reply_that_is_playing_and_the_session_goes_on() {
    let held = Script {
        auto_drain: false,
        ..Script::noon()
    };
    let mut r = rig(Plan::Run(held), config_with(true, true));
    r.host.command(command::START);
    // The output holds the reply: it does not report itself drained.
    r.wait_until("the reply plays", |r| r.state() == "AssistantSpeaking");
    r.host.command(command::INTERRUPT);
    r.wait_until("the reply is cut", |r| {
        r.out.flushes.load(Ordering::SeqCst) >= 1 && r.state() == "Listening"
    });
    assert!(r.host.is_active());
    let lines = r.lines();
    assert!(
        lines
            .iter()
            .any(|(role, _, fin)| role == "assistant" && *fin),
        "the reply line is closed: {lines:?}"
    );
    r.stop_and_wait();
}

#[test]
fn the_orb_commands_of_an_idle_assistant_do_nothing_and_a_second_start_does_not_start_twice() {
    let mut r = rig(Plan::Run(Script::noon()), AppConfig::default());
    r.host.command(command::STOP);
    r.host.command(command::INTERRUPT);
    r.host.command("SomethingElse");
    r.host.tick();
    assert!(!r.host.is_active());
    r.host.command(command::START);
    r.host.command(command::START);
    assert_eq!(r.launched.load(Ordering::SeqCst), 1);
    r.stop_and_wait();
}

#[test]
fn an_assistant_session_is_refused_while_call_mode_runs() {
    let mut r = rig(Plan::Run(Script::noon()), AppConfig::default());
    let claim = r.call.hold_for_call();
    r.host.command(command::START);
    let issue = r
        .shared
        .snapshot_for_ui()
        .assistant_issue
        .expect("an issue");
    assert_eq!(issue.key, "assistant.issue.call_running");
    assert_eq!(issue.action, IssueAction::None);
    assert!(!r.host.is_active());
    assert_eq!(
        r.launched.load(Ordering::SeqCst),
        0,
        "nothing was prepared while the call runs"
    );
    drop(claim);
    r.host.command(command::START);
    assert!(r.host.is_active(), "after the call it may start");
    r.stop_and_wait();
}

#[test]
fn end_assistant_and_start_hands_the_microphone_to_call_mode_once_the_session_has_ended() {
    let mut r = rig(Plan::Run(Script::noon()), AppConfig::default());
    r.host.command(command::START);
    assert!(r.call.assistant_holds_microphone());
    // The start call mode was refused with, then the button.
    assert!(r.call.handle_command(call_command::START_TRANSLATED));
    assert_eq!(r.call.sent_commands(), [call_command::START_TRANSLATED]);
    super::run::pump_one(
        &mut r.host,
        &r.shared,
        &r.call,
        call_command::END_ASSISTANT_AND_START,
    );
    assert_eq!(
        r.call.sent_commands(),
        [call_command::START_TRANSLATED],
        "the armed start waits for the end of the session"
    );
    r.wait_until("the session to end", |r| !r.host.is_active());
    assert!(
        !r.call.assistant_holds_microphone(),
        "the claim is free when the start goes"
    );
    assert_eq!(
        r.call.sent_commands(),
        [call_command::START_TRANSLATED, "CallEndAssistantAndStart"]
    );
    // Once only.
    r.host.tick();
    assert_eq!(r.call.sent_commands().len(), 2);
}

#[test]
fn a_new_session_after_one_ended_starts_fresh() {
    let mut r = rig(Plan::Run(Script::noon()), config_with(true, true));
    r.host.command(command::START);
    r.wait_until("a turn", |r| r.lines().len() >= 2);
    r.stop_and_wait();
    r.host.command(command::START);
    assert!(r.host.is_active());
    r.wait_until("a turn", |r| r.lines().len() >= 2);
    r.stop_and_wait();
    assert_eq!(r.history().len(), 2, "each session is a record of its own");
    assert_eq!(r.launched.load(Ordering::SeqCst), 2);
}

/// A launcher that takes "seconds", during which the user clicks the orb again.
struct SlowLauncher {
    shared: SharedAppState,
    seen: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Launcher for SlowLauncher {
    fn launch(&mut self, _config: &AppConfig) -> Result<Launched, Vec<AssistantIssue>> {
        self.seen
            .lock()
            .expect("seen")
            .push(self.shared.snapshot_for_ui().session_state);
        // Three more clicks and an unrelated command while the engines load.
        for cmd in [
            command::CREATE,
            command::START,
            command::STOP,
            "ExportDiagnostics",
        ] {
            self.shared.push_command(cmd);
        }
        Err(Vec::new())
    }
}

#[test]
fn clicks_while_the_assistant_is_starting_are_ignored_and_the_orb_says_it_is_starting() {
    let shared = SharedAppState::new();
    let call = CallControl::without_coordinator(&shared);
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut host = AssistantHost::new(
        shared.clone(),
        call,
        None,
        Box::new(SlowLauncher {
            shared: shared.clone(),
            seen: Arc::clone(&seen),
        }),
        Box::new(AppConfig::default),
    );
    host.command(command::START);
    assert_eq!(
        seen.lock().expect("seen").as_slice(),
        ["Starting"],
        "visible from the first click"
    );
    assert_eq!(
        shared.take_commands(),
        vec!["ExportDiagnostics".to_owned()],
        "the extra start and stop were dropped, the other command kept"
    );
    // The start failed: the orb is idle again and nothing else is queued.
    let snap = shared.snapshot_for_ui();
    assert_eq!(
        (snap.session_state.as_str(), snap.session_active),
        ("Idle", false)
    );
    assert!(!host.is_active());
}
