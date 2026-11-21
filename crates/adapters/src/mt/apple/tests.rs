//! Deterministic tests of system translation against a fake operating-system service.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{
    CancellationToken, Egress, MtDelta, MtError, MtFlag, MtRequest, StreamingKind, Translator,
};

use super::*;

/// What the fake session does with the next `translate`.
enum Reply {
    Text(String),
    Fault(SystemFaultKind),
    /// Block until `cancel` is called, then fail as the bridge does.
    UntilCancel,
    /// Block until the test opens the gate.
    UntilGate(String),
}

#[derive(Default)]
struct Gate {
    open: Mutex<bool>,
    changed: Condvar,
}

impl Gate {
    fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
    fn wait(&self) {
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
}

#[derive(Default)]
struct FakeSession {
    script: Mutex<VecDeque<Reply>>,
    calls: Mutex<Vec<String>>,
    prepares: AtomicUsize,
    cancels: AtomicUsize,
    cancelled: Gate,
    gate: Gate,
    started: Gate,
}

impl FakeSession {
    fn with(replies: Vec<Reply>) -> Arc<Self> {
        let s = Self::default();
        *s.script.lock().unwrap() = replies.into();
        Arc::new(s)
    }
}

impl SystemSession for FakeSession {
    fn prepare(&self, _timeout: Duration) -> Result<(), SystemFault> {
        self.prepares.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn translate(&self, text: &str, _timeout: Duration) -> Result<String, SystemFault> {
        self.calls.lock().unwrap().push(text.to_owned());
        self.started.release();
        let reply = self
            .script
            .lock()
            .unwrap()
            .pop_front()
            .expect("scripted reply");
        match reply {
            Reply::Text(t) => Ok(t),
            Reply::Fault(k) => Err(SystemFault::new(k, "scripted")),
            Reply::UntilCancel => {
                self.cancelled.wait();
                Err(SystemFault::new(SystemFaultKind::Cancelled, "cancelled"))
            }
            Reply::UntilGate(t) => {
                self.gate.wait();
                Ok(t)
            }
        }
    }

    fn cancel(&self) {
        self.cancels.fetch_add(1, Ordering::SeqCst);
        self.cancelled.release();
    }
}

struct FakeApi {
    os: Result<(), SystemFault>,
    open: Result<Arc<FakeSession>, SystemFault>,
    opened: Mutex<Vec<(String, String, Strategy)>>,
}

impl FakeApi {
    fn ok(session: Arc<FakeSession>) -> Self {
        Self {
            os: Ok(()),
            open: Ok(session),
            opened: Mutex::default(),
        }
    }
}

impl SystemTranslationApi for FakeApi {
    fn os_support(&self) -> Result<(), SystemFault> {
        self.os.clone()
    }
    fn availability(&self, _s: &str, _t: &str) -> Result<Availability, SystemFault> {
        Ok(Availability::Installed)
    }
    fn open(
        &self,
        source: &str,
        target: &str,
        strategy: Strategy,
    ) -> Result<Arc<dyn SystemSession>, SystemFault> {
        self.opened
            .lock()
            .unwrap()
            .push((source.into(), target.into(), strategy));
        self.open.clone().map(|s| s as Arc<dyn SystemSession>)
    }
}

fn engine(session: &Arc<FakeSession>, source: Lang, target: Lang) -> AppleSystemMt {
    AppleSystemMt::open(
        &FakeApi::ok(Arc::clone(session)),
        SystemTranslationConfig::new(source, target),
    )
    .expect("opens")
}

fn request(text: &str, source: Lang, target: Lang) -> MtRequest {
    MtRequest {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
        source,
        target,
        text: text.to_owned(),
        context: Vec::new(),
        glossary: Vec::new(),
        strict_language: false,
    }
}

fn zh_en(text: &str) -> MtRequest {
    request(text, Lang::Zh, Lang::En)
}

#[test]
fn a_translation_arrives_as_one_text_then_done() {
    let session = FakeSession::with(vec![Reply::Text("The meeting is at 3:15 p.m.".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("下午三点十五分开会")).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(MtDelta::Text("The meeting is at 3:15 p.m.".into())))
    );
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(MtDelta::Done {
            prompt_tokens: 0,
            completion_tokens: 0
        }))
    );
    assert_eq!(stream.next(&cancel), None);
    assert_eq!(stream.next(&cancel), None, "None forever");
    assert!(stream.flags().is_empty());
    assert!(stream.suspect().is_none());
}

#[test]
fn the_pairs_language_tags_and_strategy_reach_the_service() {
    let session = FakeSession::with(vec![]);
    let api = FakeApi::ok(session);
    let mut cfg = SystemTranslationConfig::new(Lang::Zh, Lang::En);
    cfg.strategy = Strategy::LowLatency;
    AppleSystemMt::open(&api, cfg).unwrap();
    let mut cfg = SystemTranslationConfig::new(Lang::En, Lang::Zh);
    cfg.strategy = Strategy::HighFidelity;
    AppleSystemMt::open(&api, cfg).unwrap();
    assert_eq!(
        *api.opened.lock().unwrap(),
        vec![
            ("zh-Hans".into(), "en".into(), Strategy::LowLatency),
            ("en".into(), "zh-Hans".into(), Strategy::HighFidelity)
        ]
    );
}

#[test]
fn the_descriptor_is_truthful() {
    let mt = engine(&FakeSession::with(vec![]), Lang::Zh, Lang::En);
    let d = mt.descriptor();
    assert_eq!(d.id.as_str(), PROVIDER_ID);
    assert_eq!(
        d.streaming,
        StreamingKind::Segmented,
        "whole text, not token streaming"
    );
    assert!(
        d.require_live().is_ok(),
        "admitted on the live translation role"
    );
    assert!(
        d.require_native().is_err(),
        "and not claimed to be token streaming"
    );
    assert!(!d.supports_context, "no context");
    assert!(
        !d.supports_instructions,
        "no custom instructions or glossary"
    );
    assert_eq!(d.egress, Egress::Local);
    assert!(d.require_pair(Lang::Zh, Lang::En).is_ok());
}

#[test]
fn an_engine_exists_only_for_a_pair_that_can_translate() {
    let session = FakeSession::with(vec![]);
    for kind in [
        SystemFaultKind::OsTooOld,
        SystemFaultKind::PackNotInstalled,
        SystemFaultKind::PairUnsupported,
        SystemFaultKind::Framework,
    ] {
        let api = FakeApi {
            os: Ok(()),
            open: Err(SystemFault::new(kind, "x")),
            opened: Mutex::default(),
        };
        let err = AppleSystemMt::open(&api, SystemTranslationConfig::new(Lang::Zh, Lang::En))
            .unwrap_err();
        assert_eq!(err.kind, kind);
    }
    let api = FakeApi {
        os: Err(SystemFault::new(SystemFaultKind::OsTooOld, "")),
        open: Ok(session),
        opened: Mutex::default(),
    };
    let err =
        AppleSystemMt::open(&api, SystemTranslationConfig::new(Lang::Zh, Lang::En)).unwrap_err();
    assert_eq!(err.kind, SystemFaultKind::OsTooOld);
    assert!(
        api.opened.lock().unwrap().is_empty(),
        "an old system never opens a session"
    );
}

#[test]
fn warm_up_prepares_and_translates_one_phrase_in_the_source_language() {
    let session = FakeSession::with(vec![
        Reply::Text("Hello.".into()),
        Reply::Text("你好。".into()),
    ]);
    let mt = engine(&session, Lang::En, Lang::Zh);
    mt.warm_up().unwrap();
    assert_eq!(session.prepares.load(Ordering::SeqCst), 1);
    assert_eq!(*session.calls.lock().unwrap(), vec!["Hello.".to_owned()]);
    let session = FakeSession::with(vec![Reply::Text("Hello.".into())]);
    engine(&session, Lang::Zh, Lang::En).warm_up().unwrap();
    assert_eq!(*session.calls.lock().unwrap(), vec!["你好。".to_owned()]);
}

#[test]
fn a_request_for_another_pair_or_an_empty_clause_is_refused() {
    let mt = engine(&FakeSession::with(vec![]), Lang::Zh, Lang::En);
    assert!(matches!(
        mt.translate(request("hello", Lang::En, Lang::Zh)),
        Err(MtError::Unavailable { .. })
    ));
    assert!(matches!(
        mt.translate(zh_en("   ")),
        Err(MtError::Internal(_))
    ));
}

#[test]
fn context_glossary_and_strictness_are_ignored_and_only_the_text_is_sent() {
    let session = FakeSession::with(vec![Reply::Text("Fine.".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut req = zh_en("好的");
    req.context = vec![("a".into(), "b".into())];
    req.glossary = vec![("项目".into(), "project".into())];
    req.strict_language = true;
    let mut stream = mt.translate(req).unwrap();
    let cancel = CancellationToken::new();
    while stream.next(&cancel).is_some() {}
    assert_eq!(*session.calls.lock().unwrap(), vec!["好的".to_owned()]);
}

#[test]
fn an_empty_result_is_a_failure_and_a_wrong_language_result_is_a_flag() {
    let session = FakeSession::with(vec![Reply::Text("  ".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("你好")).unwrap();
    let cancel = CancellationToken::new();
    assert!(matches!(
        stream.next(&cancel),
        Some(Err(MtError::Internal(_)))
    ));
    assert_eq!(stream.next(&cancel), None);

    let session = FakeSession::with(vec![Reply::Text(
        "你好，今天的会议改到下午三点十五分。".into(),
    )]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt
        .translate(zh_en("你好，今天的会议改到下午三点十五分。"))
        .unwrap();
    assert!(matches!(stream.next(&cancel), Some(Ok(MtDelta::Text(_)))));
    assert_eq!(stream.flags(), vec![MtFlag::WrongLanguage]);
}

#[test]
fn model_specific_checks_do_not_apply() {
    // An English output that opens like a chat reply is a translation of "sure" here.
    let session = FakeSession::with(vec![Reply::Text("Sure, here is the report.".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("当然，这是报告。")).unwrap();
    let cancel = CancellationToken::new();
    while stream.next(&cancel).is_some() {}
    assert!(stream.flags().is_empty());
    assert!(stream.suspect().is_none());
}

#[test]
fn a_timeout_maps_to_the_ports_timeout_and_the_engine_stays_usable() {
    let session = FakeSession::with(vec![
        Reply::Fault(SystemFaultKind::Timeout),
        Reply::Text("Hello.".into()),
    ]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let cancel = CancellationToken::new();
    let mut first = mt.translate(zh_en("你好")).unwrap();
    let err = first.next(&cancel).unwrap().unwrap_err();
    assert!(err.is_timeout(), "{err:?}");
    assert_eq!(first.next(&cancel), None);
    let mut second = mt.translate(zh_en("你好")).unwrap();
    assert_eq!(
        second.next(&cancel),
        Some(Ok(MtDelta::Text("Hello.".into())))
    );
}

#[test]
fn another_framework_failure_is_unavailable() {
    let session = FakeSession::with(vec![Reply::Fault(SystemFaultKind::Framework)]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("你好")).unwrap();
    assert!(matches!(
        stream.next(&CancellationToken::new()),
        Some(Err(MtError::Unavailable { .. }))
    ));
}

#[test]
fn cancelling_interrupts_the_blocked_call_promptly() {
    let session = FakeSession::with(vec![Reply::UntilCancel]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("你好")).unwrap();
    session.started.wait();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    let t = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(30));
        trigger.cancel();
    });
    let began = Instant::now();
    assert_eq!(stream.next(&cancel), Some(Err(MtError::Cancelled)));
    assert!(began.elapsed() < Duration::from_millis(500));
    assert_eq!(stream.next(&cancel), None);
    assert_eq!(session.cancels.load(Ordering::SeqCst), 1);
    t.join().unwrap();
}

#[test]
fn dropping_a_waiting_stream_abandons_its_call() {
    let session = FakeSession::with(vec![Reply::UntilCancel]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let stream = mt.translate(zh_en("你好")).unwrap();
    session.started.wait();
    drop(stream);
    assert_eq!(session.cancels.load(Ordering::SeqCst), 1);
}

#[test]
fn cancelling_a_queued_request_leaves_the_running_call_alone_and_never_starts_it() {
    let session = FakeSession::with(vec![Reply::UntilGate("First.".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let cancel = CancellationToken::new();
    let mut running = mt.translate(zh_en("一")).unwrap();
    session.started.wait();
    // The second request waits behind the first one's call; its stream is dropped (cancelled).
    let queued = mt.translate(zh_en("二")).unwrap();
    drop(queued);
    assert_eq!(
        session.cancels.load(Ordering::SeqCst),
        0,
        "the running call belongs to another request"
    );
    session.gate.release();
    assert_eq!(
        running.next(&cancel),
        Some(Ok(MtDelta::Text("First.".into())))
    );
    // Give the queued thread the chance to run: it must not call the service.
    drop(running);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(*session.calls.lock().unwrap(), vec!["一".to_owned()]);
}

#[test]
fn a_cancelled_token_ends_a_stream_before_any_wait() {
    let session = FakeSession::with(vec![Reply::UntilGate("late".into())]);
    let mt = engine(&session, Lang::Zh, Lang::En);
    let mut stream = mt.translate(zh_en("你好")).unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(stream.next(&cancel), Some(Err(MtError::Cancelled)));
    session.gate.release();
}
