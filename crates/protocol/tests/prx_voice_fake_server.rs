//! Tests of `PrxVoiceTransport` against an in-process fake of the prx-voice
//! `/api/v1/stream` endpoint.
//!
//! The fake speaks the protocol as the real server does (see
//! `prx-voice-control/src/api.rs`, `handle_unified_stream`): it issues its own
//! `sess-{uuid}` ids in `session_created`, ignores messages for unknown
//! sids, omits `sid` on `token` / `tts_start` / `tts_end`, and prefixes binary
//! TTS frames with the 36-byte hyphenated session UUID. No network beyond
//! loopback and no audio hardware is involved.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::{SinkExt, StreamExt};
use parking_lot::Mutex;
use prxvc_protocol::prelude::*;
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use uuid::Uuid;

/// Payload of every fake TTS frame: a recognisable byte ramp.
fn tts_payload() -> Vec<u8> {
    (0..640u32).map(|i| (i % 251) as u8).collect()
}

#[derive(Debug, Clone)]
enum Received {
    Text(Value),
    Binary(Vec<u8>),
}

#[derive(Clone, Debug)]
enum Ctl {
    /// Close every open connection from the server side.
    CloseAll,
    /// Push a raw message to every open connection.
    Push(Message),
}

struct FakeServer {
    base_url: String,
    received: Arc<Mutex<Vec<Received>>>,
    issued: Arc<Mutex<Vec<String>>>,
    ctl: tokio::sync::broadcast::Sender<Ctl>,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FakeServer {
    fn start() -> Self {
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        std_listener.set_nonblocking(true).expect("nonblocking");
        let addr = std_listener.local_addr().expect("addr");
        let received = Arc::new(Mutex::new(Vec::new()));
        let issued = Arc::new(Mutex::new(Vec::new()));
        let (ctl, _) = tokio::sync::broadcast::channel::<Ctl>(16);
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

        let thread = {
            let received = Arc::clone(&received);
            let issued = Arc::clone(&issued);
            let ctl = ctl.clone();
            std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("fake server runtime");
                rt.block_on(async move {
                    let listener = tokio::net::TcpListener::from_std(std_listener).expect("listener");
                    let accept_loop = async {
                        loop {
                            let Ok((stream, _)) = listener.accept().await else { continue };
                            let received = Arc::clone(&received);
                            let issued = Arc::clone(&issued);
                            let ctl_rx = ctl.subscribe();
                            tokio::spawn(serve_connection(stream, received, issued, ctl_rx));
                        }
                    };
                    tokio::select! {
                        _ = accept_loop => {}
                        _ = shutdown_rx => {}
                    }
                });
            })
        };

        Self {
            base_url: format!("http://{addr}"),
            received,
            issued,
            ctl,
            shutdown: Some(shutdown_tx),
            thread: Some(thread),
        }
    }

    fn config(&self) -> PrxVoiceConfig {
        PrxVoiceConfig {
            base_url: self.base_url.clone(),
            ..PrxVoiceConfig::default()
        }
    }

    fn send_ctl(&self, ctl: Ctl) {
        let _ = self.ctl.send(ctl);
    }

    fn texts(&self) -> Vec<Value> {
        self.received
            .lock()
            .iter()
            .filter_map(|r| match r {
                Received::Text(v) => Some(v.clone()),
                Received::Binary(_) => None,
            })
            .collect()
    }

    fn texts_of(&self, msg_type: &str) -> Vec<Value> {
        self.texts()
            .into_iter()
            .filter(|v| v["type"] == msg_type)
            .collect()
    }

    fn binaries(&self) -> Vec<Vec<u8>> {
        self.received
            .lock()
            .iter()
            .filter_map(|r| match r {
                Received::Binary(b) => Some(b.clone()),
                Received::Text(_) => None,
            })
            .collect()
    }

    fn issued(&self) -> Vec<String> {
        self.issued.lock().clone()
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    received: Arc<Mutex<Vec<Received>>>,
    issued: Arc<Mutex<Vec<String>>>,
    mut ctl_rx: tokio::sync::broadcast::Receiver<Ctl>,
) {
    let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let hello = json!({"type": "connected", "conn_id": "c1", "heartbeat_interval_ms": 10000});
    let _ = ws.send(Message::Text(hello.to_string().into())).await;

    // Sessions of this connection, as the real server tracks them.
    let mut sessions: Vec<String> = Vec::new();

    loop {
        tokio::select! {
            ctl = ctl_rx.recv() => {
                match ctl {
                    Ok(Ctl::CloseAll) => {
                        let _ = ws.send(Message::Close(None)).await;
                        return;
                    }
                    Ok(Ctl::Push(msg)) => { let _ = ws.send(msg).await; }
                    Err(_) => return,
                }
            }
            msg = ws.next() => {
                let Some(Ok(msg)) = msg else { return };
                match msg {
                    Message::Text(text) => {
                        let v: Value = serde_json::from_str(&text).expect("client sent JSON");
                        received.lock().push(Received::Text(v.clone()));
                        let known = v["sid"].as_str().is_some_and(|s| sessions.iter().any(|k| k == s));
                        match v["type"].as_str().unwrap_or_default() {
                            "ping" => {
                                let pong = json!({"type": "pong"});
                                let _ = ws.send(Message::Text(pong.to_string().into())).await;
                            }
                            "create_session" => {
                                let sid = format!("sess-{}", Uuid::new_v4());
                                sessions.push(sid.clone());
                                issued.lock().push(sid.clone());
                                let reply = json!({
                                    "type": "session_created",
                                    "req_id": v["req_id"],
                                    "sid": sid,
                                    "state": "Listening",
                                });
                                let _ = ws.send(Message::Text(reply.to_string().into())).await;
                            }
                            // Like the real server: unknown sids are ignored.
                            "text" if known => {
                                let sid = v["sid"].as_str().unwrap();
                                let uuid = sid.strip_prefix("sess-").unwrap();
                                for m in [
                                    json!({"type": "status", "sid": sid, "status": "thinking"}),
                                    json!({"type": "tts_start", "sample_rate": 24000}),
                                    json!({"type": "token", "token": "hi", "cumulative": "hi"}),
                                ] {
                                    let _ = ws.send(Message::Text(m.to_string().into())).await;
                                }
                                let mut frame = uuid.as_bytes().to_vec();
                                frame.extend_from_slice(&tts_payload());
                                let _ = ws.send(Message::Binary(frame.into())).await;
                                for m in [
                                    json!({"type": "response", "user_text": v["text"], "agent_text": "hi"}),
                                    json!({"type": "tts_end"}),
                                ] {
                                    let _ = ws.send(Message::Text(m.to_string().into())).await;
                                }
                            }
                            "audio_end" if known => {
                                let m = json!({"type": "transcript", "sid": v["sid"], "text": "hello"});
                                let _ = ws.send(Message::Text(m.to_string().into())).await;
                            }
                            "interrupt" if known => {
                                let m = json!({"type": "status", "sid": v["sid"], "status": "interrupted"});
                                let _ = ws.send(Message::Text(m.to_string().into())).await;
                            }
                            "close_session" if known => {
                                let m = json!({"type": "session_closed", "sid": v["sid"]});
                                let _ = ws.send(Message::Text(m.to_string().into())).await;
                            }
                            _ => {}
                        }
                    }
                    Message::Binary(data) => received.lock().push(Received::Binary(data.to_vec())),
                    Message::Close(_) => return,
                    _ => {}
                }
            }
        }
    }
}

fn wait_for<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Poll events until `pick` matches one, ignoring the others.
fn wait_event<T>(
    t: &mut PrxVoiceTransport,
    what: &str,
    mut pick: impl FnMut(&Event) -> Option<T>,
) -> T {
    wait_for(what, || {
        while let Some(ev) = t.poll_event().expect("poll_event") {
            if let Some(v) = pick(&ev) {
                return Some(v);
            }
        }
        None
    })
}

fn connected(server: &FakeServer) -> PrxVoiceTransport {
    let mut t = PrxVoiceTransport::new(server.config());
    t.connect().expect("connect");
    assert_eq!(t.state(), TransportState::Connected);
    t
}

// ---------------------------------------------------------------------------
// Item 2: session handshake
// ---------------------------------------------------------------------------

#[test]
fn outgoing_messages_carry_the_sid_the_server_issued() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();

    t.create_session(engine_id).expect("create_session");
    let issued = server.issued();
    assert_eq!(issued.len(), 1);
    let sid = issued[0].clone();
    assert_eq!(t.server_sid(&engine_id).as_deref(), Some(sid.as_str()));
    assert_ne!(sid, format!("sess-{engine_id}"), "engine id must not leak");

    t.send_command(&Command::StartSession { session_id: engine_id }).unwrap();
    t.send_command(&Command::SendText { session_id: engine_id, text: "hello".into() })
        .unwrap();
    t.send_command(&Command::InterruptPlayback { session_id: engine_id }).unwrap();
    t.send_command(&Command::StopSession { session_id: engine_id }).unwrap();

    wait_for("close_session", || {
        (!server.texts_of("close_session").is_empty()).then_some(())
    });
    for msg_type in ["audio_start", "text", "interrupt", "close_session"] {
        let msgs = server.texts_of(msg_type);
        assert_eq!(msgs.len(), 1, "exactly one {msg_type}");
        assert_eq!(msgs[0]["sid"], sid.as_str(), "{msg_type} must carry the issued sid");
    }
    assert!(
        server
            .texts()
            .iter()
            .all(|m| !m.to_string().contains(&engine_id.to_string())),
        "the engine id must never be sent to the server"
    );
}

#[test]
fn inbound_events_carry_the_engine_session_id() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    t.send_command(&Command::SendText { session_id: engine_id, text: "hi".into() })
        .unwrap();

    // `token` and `tts_start` carry no sid on the wire (as on the real server).
    let (mut token_sid, mut started_sid) = (None, None);
    wait_event(&mut t, "token and tts_start", |e| {
        match e {
            Event::AssistantReplyTextDelta { session_id, delta } if delta == "hi" => {
                token_sid = Some(*session_id);
            }
            Event::AssistantReplyStarted { session_id } => started_sid = Some(*session_id),
            _ => {}
        }
        (token_sid.is_some() && started_sid.is_some()).then_some(())
    });
    assert_eq!(token_sid, Some(engine_id));
    assert_eq!(started_sid, Some(engine_id));

    // `transcript` carries the server sid and must be mapped back.
    t.send_audio_data(&engine_id.to_string(), &[0u8; 4]).unwrap();
    let sid = server.issued()[0].clone();
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "transcript", "sid": sid, "text": "hello"}).to_string().into(),
    )));
    let transcript_sid = wait_event(&mut t, "transcript", |e| match e {
        Event::InterimTranscriptProduced { session_id, text } if text == "hello" => {
            Some(*session_id)
        }
        _ => None,
    });
    assert_eq!(transcript_sid, engine_id);
}

#[test]
fn unknown_session_has_no_fallback() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let known = Uuid::new_v4();
    let stranger = Uuid::new_v4();
    t.create_session(known).unwrap();

    // Commands for a session the transport never created fail instead of
    // silently going to the most recent one.
    assert!(t.send_command(&Command::SendText { session_id: stranger, text: "x".into() }).is_err());
    assert!(t.send_command(&Command::InterruptPlayback { session_id: stranger }).is_err());
    assert!(t.send_command(&Command::StartSession { session_id: stranger }).is_err());
    assert!(t.send_audio_data(&stranger.to_string(), &[0u8; 2]).is_err());
    assert!(t.send_audio_data("not-a-uuid", &[0u8; 2]).is_err());

    // An event naming an unknown sid is dropped, not attributed to `known`.
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "token", "sid": format!("sess-{}", Uuid::new_v4()), "token": "ghost"})
            .to_string()
            .into(),
    )));
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "token", "token": "marker"}).to_string().into(),
    )));
    let first = wait_event(&mut t, "marker token", |e| match e {
        Event::AssistantReplyTextDelta { delta, .. } => Some(delta.clone()),
        _ => None,
    });
    assert_eq!(first, "marker", "the ghost token must have been dropped");
    assert!(server.texts_of("text").is_empty());
}

#[test]
fn sid_less_messages_are_dropped_when_ambiguous() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    t.create_session(Uuid::new_v4()).unwrap();
    t.create_session(Uuid::new_v4()).unwrap();

    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "token", "token": "which one?"}).to_string().into(),
    )));
    // A message that is not session scoped still passes and tells us the
    // earlier one has been processed.
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "error", "message": "marker"}).to_string().into(),
    )));
    let mut saw_token = false;
    wait_event(&mut t, "error marker", |e| match e {
        Event::AssistantReplyTextDelta { .. } => {
            saw_token = true;
            None
        }
        Event::ErrorOccurred { message, .. } if message == "marker" => Some(()),
        _ => None,
    });
    assert!(!saw_token, "ambiguous sid-less token must be dropped");
}

#[test]
fn create_session_after_server_close_fails() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    server.send_ctl(Ctl::CloseAll);
    wait_for("disconnect", || (t.state() == TransportState::Disconnected).then_some(()));
    assert!(t.create_session(Uuid::new_v4()).is_err());
}

// ---------------------------------------------------------------------------
// Item 5: audio delivery
// ---------------------------------------------------------------------------

#[test]
fn binary_frames_reach_the_receive_path_with_prefix_stripped() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    t.send_command(&Command::SendText { session_id: engine_id, text: "hi".into() })
        .unwrap();

    let chunk = wait_for("tts audio", || t.poll_audio());
    assert_eq!(chunk.session_id, engine_id);
    assert_eq!(chunk.sample_rate, 24000, "rate comes from tts_start");
    assert_eq!(chunk.pcm16, tts_payload(), "36-byte prefix must be stripped");
    assert!(t.poll_audio().is_none());
}

#[test]
fn audio_of_an_interrupted_reply_is_dropped_until_the_next_tts_start() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    let sid = server.issued()[0].clone();
    let uuid = sid.strip_prefix("sess-").unwrap().to_owned();
    let frame = || {
        let mut f = uuid.as_bytes().to_vec();
        f.extend_from_slice(&tts_payload());
        Message::Binary(f.into())
    };

    t.send_command(&Command::InterruptPlayback { session_id: engine_id }).unwrap();
    // Late frame of the interrupted reply, then a marker proving it was seen.
    server.send_ctl(Ctl::Push(frame()));
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "error", "message": "marker"}).to_string().into(),
    )));
    wait_event(&mut t, "marker", |e| match e {
        Event::ErrorOccurred { message, .. } if message == "marker" => Some(()),
        _ => None,
    });
    assert!(t.poll_audio().is_none(), "stale audio must be dropped");

    // A new reply re-enables audio.
    server.send_ctl(Ctl::Push(Message::Text(
        json!({"type": "tts_start", "sample_rate": 16000}).to_string().into(),
    )));
    server.send_ctl(Ctl::Push(frame()));
    let chunk = wait_for("audio of the new reply", || t.poll_audio());
    assert_eq!(chunk.sample_rate, 16000);
}

#[test]
fn outgoing_audio_is_prefixed_with_the_server_session_uuid() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    let uuid = server.issued()[0].strip_prefix("sess-").unwrap().to_owned();

    t.send_audio_data(&engine_id.to_string(), &[1, 2, 3, 4]).unwrap();
    let bins = wait_for("binary frame", || {
        let b = server.binaries();
        (!b.is_empty()).then_some(b)
    });
    assert_eq!(&bins[0][..36], uuid.as_bytes());
    assert_eq!(&bins[0][36..], &[1, 2, 3, 4]);
}

// ---------------------------------------------------------------------------
// Item 3: runtime safety and reconnect
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn connect_disconnect_connect_inside_a_multi_thread_runtime() {
    let server = FakeServer::start();
    let mut t = PrxVoiceTransport::new(server.config());
    t.connect().expect("first connect");
    t.disconnect().expect("disconnect");
    assert_eq!(t.state(), TransportState::Disconnected);
    t.connect().expect("second connect");
    assert_eq!(t.state(), TransportState::Connected);
    t.disconnect().expect("final disconnect");
    // Dropping a connected transport inside the runtime must not panic either.
    t.connect().expect("third connect");
    drop(t);
}

#[tokio::test(flavor = "current_thread")]
async fn connect_disconnect_connect_inside_a_current_thread_runtime() {
    let server = FakeServer::start();
    let mut t = PrxVoiceTransport::new(server.config());
    t.connect().expect("first connect");
    t.disconnect().expect("disconnect");
    t.connect().expect("second connect");
    t.disconnect().expect("final disconnect");
}

#[test]
fn server_close_is_detected_and_reconnect_succeeds() {
    let server = FakeServer::start();
    let mut t = connected(&server);

    server.send_ctl(Ctl::CloseAll);
    wait_for("Disconnected", || {
        let _ = t.poll_event();
        (t.state() == TransportState::Disconnected).then_some(())
    });
    assert!(t.send_command(&Command::RefreshDeviceList).is_err());

    t.disconnect().expect("disconnect after loss");
    t.connect().expect("reconnect");
    assert_eq!(t.state(), TransportState::Connected);
    t.create_session(Uuid::new_v4()).expect("session on the new connection");
}

#[test]
fn sessions_survive_a_reconnect_under_the_same_engine_id() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    t.send_command(&Command::StartSession { session_id: engine_id }).unwrap();
    let old_sid = server.issued()[0].clone();

    server.send_ctl(Ctl::CloseAll);
    wait_for("Disconnected", || (t.state() == TransportState::Disconnected).then_some(()));
    t.disconnect().unwrap();
    t.connect().unwrap();

    let issued = server.issued();
    assert_eq!(issued.len(), 2, "the session is re-created on the new connection");
    let new_sid = issued[1].clone();
    assert_ne!(old_sid, new_sid);
    assert_eq!(t.server_sid(&engine_id).as_deref(), Some(new_sid.as_str()));

    t.send_command(&Command::SendText { session_id: engine_id, text: "again".into() })
        .unwrap();
    wait_for("text", || (!server.texts_of("text").is_empty()).then_some(()));
    assert_eq!(server.texts_of("text")[0]["sid"], new_sid.as_str());
    // The engine must not see a second SessionCreated for a session it owns.
    let mut events = Vec::new();
    while let Ok(Some(e)) = t.poll_event() {
        events.push(e);
    }
    assert!(
        !events.iter().any(|e| matches!(e, Event::SessionCreated { .. })),
        "unexpected {events:?}"
    );
}

#[test]
fn stopped_sessions_are_not_resumed() {
    let server = FakeServer::start();
    let mut t = connected(&server);
    let engine_id = Uuid::new_v4();
    t.create_session(engine_id).unwrap();
    t.send_command(&Command::StopSession { session_id: engine_id }).unwrap();
    t.disconnect().unwrap();
    t.connect().unwrap();
    assert_eq!(server.issued().len(), 1);
    assert!(t.server_sid(&engine_id).is_none());
}
