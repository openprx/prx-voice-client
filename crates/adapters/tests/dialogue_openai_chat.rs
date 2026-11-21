//! `OpenAiChatDialogue` against a scripted local HTTP server (CS-5.1). No network beyond
//! 127.0.0.1. The timeout tests use timeouts of a few hundred milliseconds because a timeout is
//! elapsed time by definition.

mod support;

use std::thread;
use std::time::Duration;

use prxvc_adapters::dialogue::{DialogueConfig, HistoryLimits, OpenAiChatDialogue};
use prxvc_adapters::mt::Timeouts;
use prxvc_media::Lang;
use prxvc_ports::{
    CancellationToken, ConversationId, DialogueBackend, DialogueDelta, DialogueError,
    DialogueMessage, DialogueRequest, DialogueStream, Egress, StreamingKind,
};
use support::{FakeServer, Gate, Script, Step, normal, sse_done, sse_finish, sse_token};

const LONG: Duration = Duration::from_secs(5);

fn quick() -> Timeouts {
    Timeouts {
        connect: Duration::from_secs(2),
        first_token: Duration::from_millis(300),
        idle: Duration::from_millis(300),
    }
}

fn backend_with(
    server: &FakeServer,
    tweak: impl FnOnce(DialogueConfig) -> DialogueConfig,
) -> OpenAiChatDialogue {
    let cfg = tweak(
        DialogueConfig::new(server.base_url(), "test-model")
            .with_provider_id("fake-llama")
            .with_timeouts(quick()),
    );
    OpenAiChatDialogue::new(cfg).unwrap()
}

fn backend(server: &FakeServer) -> OpenAiChatDialogue {
    backend_with(server, |c| c)
}

fn conversation() -> ConversationId {
    ConversationId::new("c1")
}

fn req(text: &str) -> DialogueRequest {
    DialogueRequest {
        conversation: conversation(),
        messages: vec![DialogueMessage::user(text)],
        lang: Lang::En,
    }
}

struct Outcome {
    text: String,
    done: Option<DialogueDelta>,
    error: Option<DialogueError>,
}

fn drain(stream: &mut dyn DialogueStream, cancel: &CancellationToken) -> Outcome {
    let mut out = Outcome {
        text: String::new(),
        done: None,
        error: None,
    };
    while let Some(item) = stream.next(cancel) {
        match item {
            Ok(DialogueDelta::Text(t)) => {
                assert!(out.done.is_none(), "text after Done");
                out.text.push_str(&t);
            }
            Ok(d @ DialogueDelta::Done { .. }) => out.done = Some(d),
            Err(e) => {
                out.error = Some(e);
                break;
            }
        }
    }
    assert!(stream.next(cancel).is_none(), "the stream must stay ended");
    out
}

fn run(backend: &OpenAiChatDialogue, request: DialogueRequest) -> Outcome {
    let mut stream = backend.respond(request).unwrap();
    drain(&mut *stream, &CancellationToken::new())
}

fn roles_and_texts(body: &serde_json::Value) -> Vec<(String, String)> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["role"].as_str().unwrap().to_owned(),
                m["content"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_normal_stream_yields_text_then_done_with_the_reported_usage() {
    let server = FakeServer::start(|_| normal(&["Three ", "o'clock."]));
    let out = run(&backend(&server), req("what time is it"));
    assert_eq!(out.text, "Three o'clock.");
    assert_eq!(
        out.done,
        Some(DialogueDelta::Done {
            prompt_tokens: 30,
            completion_tokens: 2
        })
    );
    assert!(out.error.is_none());
}

#[test]
fn the_request_has_the_system_prompt_the_user_text_and_the_streaming_options() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let _ = run(&backend(&server), req("hello there"));
    let sent = &server.requests()[0];
    assert_eq!(sent.path, "/v1/chat/completions");
    assert_eq!(sent.body["model"], "test-model");
    assert_eq!(sent.body["stream"], true);
    let messages = roles_and_texts(&sent.body);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].0, "system");
    assert!(
        messages[0].1.contains("English"),
        "the user's language is named"
    );
    assert!(messages[0].1.contains("No markdown"));
    assert_eq!(messages[1], ("user".to_owned(), "hello there".to_owned()));
}

#[test]
fn extra_parameters_reach_the_body_so_that_thinking_can_be_switched_off() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let backend = backend_with(&server, |mut c| {
        c.params.insert(
            "chat_template_kwargs".into(),
            serde_json::json!({"enable_thinking": false}),
        );
        c
    });
    let _ = run(&backend, req("hi"));
    let body = &server.requests()[0].body;
    assert_eq!(body["chat_template_kwargs"]["enable_thinking"], false);
}

#[test]
fn the_api_key_is_sent_as_a_bearer_token_and_never_shown() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let backend = backend_with(&server, |c| c.with_api_key("sekret"));
    let _ = run(&backend, req("hi"));
    assert_eq!(
        server.requests()[0].header("authorization"),
        Some("Bearer sekret")
    );
    assert!(!format!("{backend:?}").contains("sekret"));
}

#[test]
fn the_history_grows_with_each_completed_turn_and_is_sent_with_the_next_request() {
    let server = FakeServer::start(|r| {
        if r.index == 0 {
            normal(&["Paris."])
        } else {
            normal(&["About two million."])
        }
    });
    let backend = backend(&server);
    let _ = run(&backend, req("capital of France?"));
    assert_eq!(backend.history_turns(&conversation()), 1);
    let _ = run(&backend, req("and how many people live there?"));
    let second = roles_and_texts(&server.requests()[1].body);
    let tail: Vec<_> = second[1..]
        .iter()
        .map(|(r, t)| (r.as_str(), t.as_str()))
        .collect();
    assert_eq!(
        tail,
        [
            ("user", "capital of France?"),
            ("assistant", "Paris."),
            ("user", "and how many people live there?"),
        ]
    );
    assert_eq!(backend.history_turns(&conversation()), 2);
}

#[test]
fn history_is_bounded_by_turns() {
    let server = FakeServer::start(|_| normal(&["fine."]));
    let backend = backend_with(&server, |c| {
        c.with_history(HistoryLimits {
            max_turns: 2,
            max_tokens: 100_000,
        })
    });
    for i in 0..4 {
        let _ = run(&backend, req(&format!("question {i}")));
    }
    let last = roles_and_texts(&server.requests()[3].body);
    let users: Vec<_> = last
        .iter()
        .filter(|(r, _)| r == "user")
        .map(|(_, t)| t.as_str())
        .collect();
    assert_eq!(
        users,
        ["question 2", "question 3"],
        "the current turn and one before it"
    );
    assert_eq!(backend.history_turns(&conversation()), 2);
}

#[test]
fn history_is_bounded_by_tokens_but_the_current_turn_is_always_sent() {
    let server = FakeServer::start(|_| normal(&["a rather long and wordy answer indeed."]));
    let backend = backend_with(&server, |c| {
        c.with_history(HistoryLimits {
            max_turns: 100,
            max_tokens: 12,
        })
    });
    let _ = run(&backend, req("first question here"));
    let _ = run(
        &backend,
        req("second question that is considerably longer than the budget allows for"),
    );
    let second = roles_and_texts(&server.requests()[1].body);
    assert_eq!(
        second.len(),
        2,
        "history dropped, system prompt and the current turn remain"
    );
    assert_eq!(second[1].0, "user");
}

#[test]
fn conversations_are_independent_and_can_be_ended() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let backend = backend(&server);
    let other = ConversationId::new("c2");
    let _ = run(&backend, req("one"));
    let mut r = req("two");
    r.conversation = other.clone();
    let _ = run(&backend, r);
    assert_eq!(
        roles_and_texts(&server.requests()[1].body).len(),
        2,
        "no history of c1"
    );
    backend.end_conversation(&conversation());
    assert_eq!(backend.history_turns(&conversation()), 0);
    assert_eq!(backend.history_turns(&other), 1);
}

#[test]
fn the_first_delta_arrives_before_the_body_ends() {
    let gate = Gate::new();
    let g = gate.clone();
    let server = FakeServer::start(move |_| {
        Script::ok(vec![
            sse_token("First"),
            Step::Wait(g.clone()),
            sse_token(" second."),
            sse_finish("stop"),
            sse_done(),
            Step::End,
        ])
    });
    let backend = backend_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..quick()
        })
    });
    let mut stream = backend.respond(req("go")).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(DialogueDelta::Text("First".into())))
    );
    gate.open();
    let rest = drain(&mut *stream, &cancel);
    assert_eq!(rest.text, " second.");
    assert!(rest.done.is_some());
}

#[test]
fn cancelling_ends_the_stream_promptly_closes_the_connection_and_keeps_the_delivered_part() {
    let server =
        FakeServer::start(|_| Script::ok(vec![sse_token("Half an"), Step::HoldUntilClosed]));
    let backend = backend_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..quick()
        })
    });
    let mut stream = backend.respond(req("tell me a story")).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(DialogueDelta::Text("Half an".into())))
    );
    let waiter = {
        let cancel = cancel.clone();
        thread::spawn(move || {
            let item = stream.next(&cancel);
            (item, stream)
        })
    };
    thread::sleep(Duration::from_millis(50));
    cancel.cancel();
    let (item, mut stream) = waiter.join().unwrap();
    assert_eq!(item, Some(Err(DialogueError::Cancelled)));
    assert!(stream.next(&CancellationToken::new()).is_none());
    assert!(
        server.closed(LONG).is_some(),
        "the server saw the client go away"
    );
    assert_eq!(
        backend.history_turns(&conversation()),
        1,
        "the interrupted turn is kept with the part that was delivered"
    );
}

#[test]
fn a_dropped_stream_abandons_the_request() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_token("x"), Step::HoldUntilClosed]));
    let backend = backend_with(&server, |c| {
        c.with_timeouts(Timeouts {
            first_token: LONG,
            idle: LONG,
            ..quick()
        })
    });
    let mut stream = backend.respond(req("hi")).unwrap();
    assert!(matches!(
        stream.next(&CancellationToken::new()),
        Some(Ok(DialogueDelta::Text(_)))
    ));
    drop(stream);
    assert!(server.closed(LONG).is_some());
}

#[test]
fn an_http_error_status_is_typed() {
    for (status, rejected) in [(401, true), (404, true), (503, false), (500, false)] {
        let server = FakeServer::start(move |_| Script::status(status));
        let out = run(&backend(&server), req("hi"));
        let error = out.error.expect("an error");
        if rejected {
            assert_eq!(error, DialogueError::Rejected { status }, "{status}");
        } else {
            assert!(
                matches!(error, DialogueError::Unavailable { .. }),
                "{status}: {error:?}"
            );
        }
    }
}

#[test]
fn a_server_that_does_not_answer_is_unavailable_not_a_mock() {
    // A port nothing listens on: bind and drop.
    let port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let cfg =
        DialogueConfig::new(format!("http://127.0.0.1:{port}/v1"), "m").with_timeouts(quick());
    let backend = OpenAiChatDialogue::new(cfg).unwrap();
    let out = run(&backend, req("hi"));
    assert!(
        matches!(out.error, Some(DialogueError::Unavailable { ref reason }) if reason.contains("connect")),
        "{:?}",
        out.error
    );
    assert!(out.text.is_empty());
    assert_eq!(
        backend.history_turns(&conversation()),
        0,
        "a failed turn leaves no history"
    );
}

#[test]
fn a_silent_server_times_out_at_the_first_token() {
    let server = FakeServer::start(|_| Script::ok(vec![Step::HoldUntilClosed]));
    let out = run(&backend(&server), req("hi"));
    assert_eq!(out.error, Some(DialogueError::Timeout));
}

#[test]
fn a_stream_that_stalls_after_the_first_token_is_unavailable() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_token("a"), Step::HoldUntilClosed]));
    let out = run(&backend(&server), req("hi"));
    assert_eq!(out.text, "a");
    assert!(
        matches!(out.error, Some(DialogueError::Unavailable { .. })),
        "{:?}",
        out.error
    );
}

#[test]
fn a_reply_without_text_is_an_error() {
    let server = FakeServer::start(|_| Script::ok(vec![sse_finish("stop"), sse_done(), Step::End]));
    let backend = backend(&server);
    let out = run(&backend, req("hi"));
    assert_eq!(out.error, Some(DialogueError::EmptyReply));
    assert_eq!(backend.history_turns(&conversation()), 0);
}

#[test]
fn a_leading_thinking_block_is_removed_and_a_label_in_the_reply_is_kept() {
    let server = FakeServer::start(|_| {
        normal(&[
            "<|channel>thought",
            "<channel|>",
            "Translation: ",
            "is a school subject.",
        ])
    });
    let out = run(&backend(&server), req("what is translation"));
    assert_eq!(out.text, "Translation: is a school subject.");
}

#[test]
fn a_language_the_backend_is_not_configured_for_is_refused_before_any_request() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let backend = backend_with(&server, |mut c| {
        c.languages = vec![Lang::En];
        c
    });
    let mut r = req("你好");
    r.lang = Lang::Zh;
    assert!(matches!(
        backend.respond(r),
        Err(DialogueError::Unavailable { .. })
    ));
    assert!(server.requests().is_empty());
}

#[test]
fn an_empty_user_message_is_refused() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    assert!(matches!(
        backend(&server).respond(req("   ")),
        Err(DialogueError::Internal(_))
    ));
}

#[test]
fn the_descriptor_says_local_native_streaming() {
    let server = FakeServer::start(|_| normal(&["ok."]));
    let d = backend(&server).descriptor();
    assert_eq!(d.id.as_str(), "fake-llama");
    assert_eq!(d.model, "test-model");
    assert_eq!(d.streaming, StreamingKind::Native);
    assert_eq!(d.egress, Egress::Local);
    assert!(d.languages.contains(&Lang::Zh));
}
