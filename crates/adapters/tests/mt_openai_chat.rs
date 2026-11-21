//! `OpenAiChatMt` against a scripted local HTTP server (CS-5.1, PD 9.1). No
//! network beyond 127.0.0.1. The timeout tests use timeouts of a few hundred
//! milliseconds because a timeout is elapsed time by definition.

mod support;

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use prxvc_adapters::mt::{
    MtFailure, MtObserver, MtReport, OpenAiChatConfig, OpenAiChatMt, PromptTemplate, Stripped,
    Suspect, Timeouts,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{
    CancellationToken, Egress, MtDelta, MtError, MtRequest, MtStream, StreamingKind, Translator,
};
use support::{
    FakeServer, Gate, Script, Step, normal, sse_done, sse_finish, sse_raw, sse_token, sse_usage,
};

const LONG: Duration = Duration::from_secs(5);

#[derive(Default)]
struct Collector(Mutex<Vec<MtReport>>);

impl MtObserver for Collector {
    fn on_report(&self, report: &MtReport) {
        self.0.lock().unwrap().push(report.clone());
    }
}

impl Collector {
    fn reports(&self) -> Vec<MtReport> {
        self.0.lock().unwrap().clone()
    }

    fn only(&self) -> MtReport {
        let reports = self.reports();
        assert_eq!(reports.len(), 1, "{reports:?}");
        reports.into_iter().next().unwrap()
    }
}

fn quick() -> Timeouts {
    Timeouts {
        connect: Duration::from_secs(2),
        first_token: Duration::from_millis(300),
        idle: Duration::from_millis(300),
    }
}

fn mt_with(
    server: &FakeServer,
    tweak: impl FnOnce(OpenAiChatConfig) -> OpenAiChatConfig,
) -> (OpenAiChatMt, Arc<Collector>) {
    let cfg = tweak(
        OpenAiChatConfig::new(server.base_url(), "test-model")
            .with_provider_id("fake-llama")
            .with_template(PromptTemplate::Gemma4),
    );
    let collector = Arc::new(Collector::default());
    let mt = OpenAiChatMt::with_observer(cfg, collector.clone()).unwrap();
    (mt, collector)
}

fn mt(server: &FakeServer) -> (OpenAiChatMt, Arc<Collector>) {
    mt_with(server, |c| c.with_timeouts(quick()))
}

fn clause(index: u16) -> ClauseId {
    ClauseId::new(SegmentId::first(Direction::Outgoing), index)
}

fn req(index: u16, text: &str) -> MtRequest {
    MtRequest {
        clause: clause(index),
        source: Lang::Zh,
        target: Lang::En,
        text: text.into(),
        context: Vec::new(),
        glossary: Vec::new(),
        strict_language: false,
    }
}

struct Outcome {
    text: String,
    done: Option<MtDelta>,
    error: Option<MtError>,
}

/// Read a stream to its end and check the protocol: nothing after the terminal item.
fn drain(stream: &mut dyn MtStream, cancel: &CancellationToken) -> Outcome {
    let mut out = Outcome {
        text: String::new(),
        done: None,
        error: None,
    };
    while let Some(item) = stream.next(cancel) {
        match item {
            Ok(MtDelta::Text(t)) => {
                assert!(out.done.is_none(), "text after Done");
                out.text.push_str(&t);
            }
            Ok(d @ MtDelta::Done { .. }) => out.done = Some(d),
            Err(e) => {
                out.error = Some(e);
                break;
            }
        }
    }
    assert!(stream.next(cancel).is_none(), "the stream must stay ended");
    out
}

fn run(mt: &OpenAiChatMt, request: MtRequest) -> Outcome {
    let mut stream = mt.translate(request).unwrap();
    drain(&mut *stream, &CancellationToken::new())
}

#[test]
fn a_normal_stream_yields_text_then_done_with_the_reported_usage() {
    let server = FakeServer::start(|_| normal(&["We meet", " at ", "three."]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "我们三点见。"));
    assert_eq!(out.text, "We meet at three.");
    assert_eq!(
        out.done,
        Some(MtDelta::Done {
            prompt_tokens: 30,
            completion_tokens: 3
        })
    );
    assert!(out.error.is_none());

    let report = collector.only();
    assert_eq!(report.clause, clause(0));
    assert!(report.failure.is_none() && report.suspects.is_empty());
    assert_eq!(report.text, "We meet at three.");
    assert!(report.first_token.is_some());
    assert_eq!(report.prompt_tokens, 30);
}

#[test]
fn the_request_has_the_documented_shape_and_no_key_header_without_a_key() {
    let server = FakeServer::start(|_| normal(&["Hi."]));
    let (mt, _) = mt(&server);
    run(&mt, req(0, "你好。"));
    let requests = server.requests();
    assert_eq!(requests.len(), 1);
    let r = &requests[0];
    assert_eq!(r.path, "/v1/chat/completions");
    assert_eq!(r.header("authorization"), None);
    assert_eq!(r.header("accept"), Some("text/event-stream"));
    assert_eq!(r.body["model"], "test-model");
    assert_eq!(r.body["stream"], true);
    assert_eq!(r.body["temperature"], 0);
    assert_eq!(r.body["messages"][1]["content"], "<source>你好。</source>");
    assert_eq!(r.body["messages"][0]["role"], "system");
}

#[test]
fn the_api_key_goes_in_a_bearer_header_and_nowhere_else() {
    let server = FakeServer::start(|_| normal(&["Hi."]));
    let (mt, _) = mt_with(&server, |c| {
        c.with_api_key("sk-test-key-777").with_timeouts(quick())
    });
    assert!(!format!("{mt:?}").contains("sk-test-key-777"));
    run(&mt, req(0, "你好。"));
    let r = &server.requests()[0];
    assert_eq!(r.header("authorization"), Some("Bearer sk-test-key-777"));
    assert!(!r.body.to_string().contains("sk-test-key-777"));
}

#[test]
fn the_first_token_arrives_before_the_body_ends() {
    let gate = Gate::new();
    let held = gate.clone();
    let server = FakeServer::start(move |_| {
        Script::ok(vec![
            sse_token("First"),
            Step::Wait(held.clone()),
            sse_token(" second."),
            sse_finish("stop"),
            sse_done(),
            Step::End,
        ])
    });
    let (mt, _) = mt_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..Timeouts::default()
        })
    });
    let mut stream = mt.translate(req(0, "x")).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(MtDelta::Text("First".into()))),
        "delivered while the server still holds the rest"
    );
    gate.open();
    let out = drain(&mut *stream, &cancel);
    assert_eq!(out.text, " second.");
    assert!(out.done.is_some());
}

#[test]
fn a_slow_first_token_is_a_first_token_timeout() {
    let server = FakeServer::start(|_| Script::ok(vec![Step::HoldUntilClosed]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert_eq!(out.error, Some(MtError::Timeout));
    assert_eq!(collector.only().failure, Some(MtFailure::FirstTokenTimeout));
    assert!(server.closed(LONG).is_some(), "the connection was given up");
}

#[test]
fn a_silent_server_before_the_response_head_is_also_a_first_token_timeout() {
    let gate = Gate::new();
    let held = gate.clone();
    // The handler runs before the head is written: block it.
    let server = FakeServer::start(move |_| {
        held.wait_for_test();
        normal(&["late"])
    });
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert_eq!(out.error, Some(MtError::Timeout));
    assert_eq!(collector.only().failure, Some(MtFailure::FirstTokenTimeout));
    gate.open();
}

#[test]
fn a_stall_in_the_middle_is_an_idle_timeout_with_its_own_error() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_token("Half"), Step::HoldUntilClosed]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert_eq!(out.text, "Half", "what arrived stays delivered");
    match out.error {
        Some(MtError::Unavailable { reason }) => assert!(reason.contains("stalled"), "{reason}"),
        other => panic!("{other:?}"),
    }
    let report = collector.only();
    assert_eq!(report.failure, Some(MtFailure::IdleTimeout));
    assert_eq!(report.text, "Half");
    assert!(server.closed(LONG).is_some());
}

#[test]
fn a_refused_connection_is_unavailable_with_a_connect_failure() {
    // Bind and drop to get a port nothing listens on.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let collector = Arc::new(Collector::default());
    let cfg = OpenAiChatConfig::new(format!("http://127.0.0.1:{port}"), "m").with_timeouts(quick());
    let mt = OpenAiChatMt::with_observer(cfg, collector.clone()).unwrap();
    let out = run(&mt, req(0, "x"));
    assert!(
        matches!(out.error, Some(MtError::Unavailable { .. })),
        "{:?}",
        out.error
    );
    assert!(matches!(
        collector.only().failure,
        Some(MtFailure::ConnectFailed(_))
    ));
}

#[test]
fn http_statuses_map_to_rejected_for_4xx_and_unavailable_for_5xx() {
    for (status, rejected) in [
        (400u16, true),
        (401, true),
        (404, true),
        (429, true),
        (500, false),
        (503, false),
    ] {
        let server = FakeServer::start(move |_| Script::status(status));
        let (mt, collector) = mt(&server);
        let out = run(&mt, req(0, "x"));
        if rejected {
            assert_eq!(out.error, Some(MtError::Rejected { status }), "{status}");
        } else {
            assert!(
                matches!(out.error, Some(MtError::Unavailable { .. })),
                "{status}"
            );
        }
        assert_eq!(
            collector.only().failure,
            Some(MtFailure::HttpStatus(status))
        );
        assert!(out.text.is_empty());
    }
}

#[test]
fn a_redirect_is_not_followed() {
    let server = FakeServer::start(|_| Script::status(302));
    let (mt, collector) = mt(&server);
    run(&mt, req(0, "x"));
    assert_eq!(collector.only().failure, Some(MtFailure::HttpStatus(302)));
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_connection_dropped_mid_stream_is_unavailable_and_keeps_the_text_so_far() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_token("Cut o"), Step::Abort]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert_eq!(out.text, "Cut o");
    assert!(
        matches!(out.error, Some(MtError::Unavailable { .. })),
        "{:?}",
        out.error
    );
    assert!(matches!(
        collector.only().failure,
        Some(MtFailure::StreamBroken(_))
    ));
}

#[test]
fn a_clean_end_without_done_is_complete_only_if_the_model_said_stop() {
    let server = FakeServer::start(|r| {
        if r.index == 0 {
            Script::ok(vec![sse_token("No marker"), Step::End])
        } else {
            Script::ok(vec![sse_token("Stopped."), sse_finish("stop"), Step::End])
        }
    });
    let (mt, collector) = mt(&server);
    let first = run(&mt, req(0, "x"));
    assert!(matches!(first.error, Some(MtError::Unavailable { .. })));
    let second = run(&mt, req(1, "x"));
    assert_eq!(second.text, "Stopped.");
    assert!(second.done.is_some() && second.error.is_none());
    assert_eq!(collector.reports().len(), 2);
}

#[test]
fn a_malformed_data_line_is_skipped_and_counted() {
    let server = FakeServer::start(|_| {
        Script::ok(vec![
            sse_token("Good "),
            sse_raw("data: {not json at all"),
            sse_token("text."),
            sse_finish("stop"),
            sse_done(),
            Step::End,
        ])
    });
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert_eq!(out.text, "Good text.");
    assert!(out.error.is_none());
    assert_eq!(collector.only().malformed_lines, 1);
}

#[test]
fn an_error_event_inside_the_stream_fails_the_request() {
    let server = FakeServer::start(|_| {
        Script::ok(vec![
            sse_token("Start"),
            sse_raw(r#"data: {"error":{"code":503,"message":"slot lost"}}"#),
            Step::End,
        ])
    });
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "x"));
    assert!(matches!(out.error, Some(MtError::Unavailable { .. })));
    assert_eq!(
        collector.only().failure,
        Some(MtFailure::ServerEvent(Some(503)))
    );
}

#[test]
fn cancelling_mid_stream_closes_the_connection_while_another_request_completes() {
    let gate = Gate::new();
    let held = gate.clone();
    let server = FakeServer::start(move |r| {
        if r.body["messages"][1]["content"] == "<source>slow</source>" {
            Script::ok(vec![sse_token("Partial"), Step::HoldUntilClosed])
        } else {
            Script::ok(vec![
                sse_token("Other "),
                Step::Wait(held.clone()),
                sse_token("done."),
                sse_finish("stop"),
                sse_usage(10, 2),
                sse_done(),
                Step::End,
            ])
        }
    });
    let (mt, collector) = mt_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..Timeouts::default()
        })
    });
    // Direction A: reads its first token, then blocks in `next`.
    let cancel_a = CancellationToken::new();
    let mut slow = mt.translate(req(0, "slow")).unwrap();
    assert_eq!(
        slow.next(&cancel_a),
        Some(Ok(MtDelta::Text("Partial".into())))
    );
    let a_token = cancel_a.clone();
    let a = thread::spawn(move || {
        let second = slow.next(&a_token);
        let after = slow.next(&a_token);
        (second, after)
    });

    // Direction B: independent, started while A is blocked.
    let mut other = mt
        .translate(MtRequest {
            clause: ClauseId::new(SegmentId::first(Direction::Incoming), 0),
            source: Lang::En,
            target: Lang::Zh,
            text: "other".into(),
            context: Vec::new(),
            glossary: Vec::new(),
            strict_language: false,
        })
        .unwrap();
    let cancel_b = CancellationToken::new();
    // Trailing whitespace is held until more text follows.
    assert_eq!(
        other.next(&cancel_b),
        Some(Ok(MtDelta::Text("Other".into())))
    );

    cancel_a.cancel();
    let (second, after) = a.join().unwrap();
    assert_eq!(second, Some(Err(MtError::Cancelled)));
    assert_eq!(after, None);
    assert_eq!(
        server.closed(LONG),
        Some(0),
        "the cancelled request's connection was closed"
    );

    gate.open();
    let out = drain(&mut *other, &cancel_b);
    assert_eq!(out.text, " done.");
    assert_eq!(
        out.done,
        Some(MtDelta::Done {
            prompt_tokens: 10,
            completion_tokens: 2
        })
    );
    assert!(out.error.is_none());

    let reports = collector.reports();
    assert_eq!(reports.len(), 2);
    assert_eq!(reports[0].failure, Some(MtFailure::Cancelled));
    assert_eq!(reports[0].text, "Partial");
    assert!(reports[1].failure.is_none());
}

#[test]
fn cancelling_before_the_first_token_returns_promptly_and_closes_the_connection() {
    let server = FakeServer::start(|_| Script::ok(vec![Step::HoldUntilClosed]));
    let (mt, _) = mt_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..Timeouts::default()
        })
    });
    let cancel = CancellationToken::new();
    let mut stream = mt.translate(req(0, "x")).unwrap();
    let trigger = cancel.clone();
    let canceller = thread::spawn(move || {
        // Cancel once the request has surely reached the server.
        thread::sleep(Duration::from_millis(100));
        trigger.cancel();
    });
    assert_eq!(stream.next(&cancel), Some(Err(MtError::Cancelled)));
    assert_eq!(stream.next(&cancel), None);
    canceller.join().unwrap();
    assert_eq!(server.closed(LONG), Some(0));
}

#[test]
fn an_already_cancelled_token_cancels_at_once() {
    let server = FakeServer::start(|_| normal(&["never"]));
    let (mt, _) = mt(&server);
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut stream = mt.translate(req(0, "x")).unwrap();
    assert_eq!(stream.next(&cancel), Some(Err(MtError::Cancelled)));
}

#[test]
fn dropping_a_stream_closes_its_connection() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_token("a"), Step::HoldUntilClosed]));
    let (mt, _) = mt_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..Timeouts::default()
        })
    });
    let cancel = CancellationToken::new();
    let mut stream = mt.translate(req(0, "x")).unwrap();
    assert!(stream.next(&cancel).is_some());
    drop(stream);
    assert_eq!(server.closed(LONG), Some(0));
}

#[test]
fn a_wrapper_around_the_translation_is_removed_and_reported() {
    let server = FakeServer::start(|_| normal(&["Translation: ", "\"", "See you", " at 3.", "\""]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "三点见。"));
    assert_eq!(out.text, "See you at 3.");
    let report = collector.only();
    assert_eq!(
        report.stripped,
        [Stripped::Preamble, Stripped::QuoteWrapper]
    );
    assert!(report.suspects.is_empty());
}

#[test]
fn an_answer_is_delivered_but_flagged_not_corrected() {
    let server = FakeServer::start(|_| normal(&["Sure! My name is Gemma."]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "你叫什么名字？"));
    assert_eq!(out.text, "Sure! My name is Gemma.");
    assert!(out.error.is_none());
    let report = collector.only();
    assert!(report.is_suspect());
    assert!(
        report.suspects.contains(&Suspect::AnswerLike),
        "{:?}",
        report.suspects
    );
}

#[test]
fn a_length_stop_is_flagged_as_truncated() {
    let server = FakeServer::start(|_| {
        Script::ok(vec![
            sse_token("Cut short"),
            sse_finish("length"),
            sse_done(),
            Step::End,
        ])
    });
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "你好，请问你今天过得怎么样"));
    assert!(out.error.is_none());
    assert!(collector.only().suspects.contains(&Suspect::Truncated));
}

#[test]
fn an_empty_translation_is_an_error_not_a_silent_success() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_finish("stop"), sse_done(), Step::End]));
    let (mt, collector) = mt(&server);
    let out = run(&mt, req(0, "你好"));
    assert!(
        matches!(out.error, Some(MtError::Internal(_))),
        "{:?}",
        out.error
    );
    assert!(out.done.is_none());
    let report = collector.only();
    assert_eq!(report.failure, Some(MtFailure::EmptyOutput));
    assert_eq!(report.suspects, [Suspect::Empty]);
}

#[test]
fn the_descriptor_says_local_streaming_and_which_pairs() {
    let server = FakeServer::start(|_| normal(&["x"]));
    let (mt, _) = mt(&server);
    let d = mt.descriptor();
    assert_eq!(d.id.as_str(), "fake-llama");
    assert_eq!(d.model, "test-model");
    assert_eq!(d.streaming, StreamingKind::Native);
    assert_eq!(d.egress, Egress::Local);
    assert!(d.require_native().is_ok());
    assert!(d.require_pair(Lang::Zh, Lang::En).is_ok());

    let remote =
        OpenAiChatMt::new(OpenAiChatConfig::new("https://api.example.test/v1", "m")).unwrap();
    assert_eq!(
        remote.descriptor().egress,
        Egress::Text {
            host: "api.example.test".into()
        },
        "text leaves the machine"
    );
}

#[test]
fn requests_that_cannot_work_are_refused_before_any_request_is_sent() {
    let server = FakeServer::start(|_| normal(&["x"]));
    let (mt, _) = mt(&server);
    let mut same = req(0, "hello");
    same.source = Lang::En;
    same.target = Lang::En;
    assert!(matches!(mt.translate(same), Err(MtError::Internal(_))));
    assert!(matches!(
        mt.translate(req(0, "   ")),
        Err(MtError::Internal(_))
    ));
    assert!(server.requests().is_empty());
}

#[test]
fn configuration_errors_are_typed() {
    assert!(OpenAiChatMt::new(OpenAiChatConfig::new("ftp://x", "m")).is_err());
}

/// Tokens of an endless loop on one fragment, then the connection is held open: a client that
/// does not abort would wait for the token cap.
fn looping_server(prefix: &'static [&'static str], unit: &'static str) -> FakeServer {
    FakeServer::start(move |_| {
        let mut steps: Vec<Step> = prefix.iter().map(|t| sse_token(t)).collect();
        steps.extend((0..400).map(|_| sse_token(unit)));
        steps.push(Step::HoldUntilClosed);
        Script::ok(steps)
    })
}

#[test]
fn a_model_that_loops_is_aborted_early_and_flagged_with_a_specific_reason() {
    let server = looping_server(&["Good", ", let me ", "see this point"], "点");
    let (mt, collector) = mt(&server);
    let mut stream = mt.translate(req(0, "好的，我们看一下这一点")).unwrap();
    let cancel = CancellationToken::new();
    let mut text = String::new();
    while let Some(item) = stream.next(&cancel) {
        match item.expect("the stream ends normally") {
            MtDelta::Text(t) => text.push_str(&t),
            MtDelta::Done { .. } => {}
        }
        if stream.suspect().is_some() {
            break;
        }
    }
    let reason = stream.suspect().expect("the pipeline can read why");
    assert!(
        reason.starts_with("repetition: a unit of 1 character(s)"),
        "{reason}"
    );
    assert!(!reason.contains('点'), "no call content in the reason");
    // Early: far fewer than the 400 looped tokens arrived, and the request was abandoned.
    assert!(text.matches('点').count() < 40, "{}", text.len());
    assert!(
        server.closed(LONG).is_some(),
        "the connection was given up as soon as the loop was seen"
    );
    // The flag stays, and the stream ends in a protocol-conforming way.
    assert!(stream.suspect().is_some());
    while stream.next(&cancel).is_some() {}
    assert!(stream.next(&cancel).is_none());
    let report = collector.only();
    assert_eq!(report.suspects, [Suspect::Repetition]);
    assert!(report.failure.is_none(), "{:?}", report.failure);
    assert!(report.is_suspect());
}

#[test]
fn a_cycling_tail_is_flagged_with_a_period() {
    let server = FakeServer::start(|_| {
        let mut steps = vec![sse_token("We meet at three. ")];
        steps.extend((0..40).map(|_| sse_token("Please confirm that you can come. ")));
        steps.push(Step::HoldUntilClosed);
        Script::ok(steps)
    });
    let (mt, collector) = mt(&server);
    let mut stream = mt.translate(req(0, "我们三点见，请确认你能来")).unwrap();
    let cancel = CancellationToken::new();
    while stream.suspect().is_none() {
        assert!(stream.next(&cancel).is_some(), "flagged before the end");
    }
    let reason = stream.suspect().unwrap();
    assert!(reason.contains("cycles with a period of 34"), "{reason}");
    assert!(collector.only().suspects.contains(&Suspect::Repetition));
}

#[test]
fn legitimate_repetition_in_a_translation_passes_untouched() {
    let server = FakeServer::start(|_| {
        normal(&[
            "Ha",
            "ha",
            "ha. No, ",
            "no, no. ",
            "It is 000000",
            ", really",
            "......",
        ])
    });
    let (mt, collector) = mt(&server);
    let mut stream = mt
        .translate(req(0, "哈哈哈。不，不，不。真的是000000。"))
        .unwrap();
    let out = drain(&mut *stream, &CancellationToken::new());
    assert_eq!(out.text, "Hahaha. No, no, no. It is 000000, really......");
    assert!(out.error.is_none() && out.done.is_some());
    assert!(stream.suspect().is_none());
    assert!(!collector.only().suspects.contains(&Suspect::Repetition));
}
