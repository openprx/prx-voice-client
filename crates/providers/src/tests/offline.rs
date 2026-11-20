//! The offline policy against a recording listener: a blocked endpoint is never contacted.

use std::io::Read;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

use prxvc_media::Lang;

use super::fixtures::*;
use crate::problem::{BuildError, ProblemKind};
use crate::test_provider::{FailureClass, test_translator};

/// A loopback listener that counts the connections it accepts and answers each with `reply`.
struct Recorder {
    port: u16,
    accepted: Arc<AtomicUsize>,
    stop: Arc<std::sync::atomic::AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Recorder {
    fn start(reply: &'static str) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let thread = {
            let (accepted, stop) = (Arc::clone(&accepted), Arc::clone(&stop));
            thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            accepted.fetch_add(1, Ordering::SeqCst);
                            stream.set_nonblocking(false).unwrap();
                            stream
                                .set_read_timeout(Some(Duration::from_millis(300)))
                                .unwrap();
                            let mut buf = [0u8; 4096];
                            let _ = stream.read(&mut buf);
                            let _ = std::io::Write::write_all(&mut stream, reply.as_bytes());
                        }
                        Err(_) => thread::sleep(Duration::from_millis(5)),
                    }
                }
            })
        };
        Self {
            port,
            accepted,
            stop,
            thread: Some(thread),
        }
    }

    fn accepted(&self) -> usize {
        self.accepted.load(Ordering::SeqCst)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

const UNAUTHORIZED: &str =
    "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n";

/// `0.0.0.0` is not loopback to the policy, yet the operating system connects it to this machine,
/// so a leak would reach the recorder.
fn entry_to(rig: &mut Rig, port: u16) {
    rig.cfg.providers.entries.push(toml_entry(&format!(
        "id = \"leaky\"\nprotocol = \"openai-chat\"\nbase_url = \"http://0.0.0.0:{port}/v1\"\nmodel = \"m\"\n"
    )));
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "leaky".into());
}

#[test]
fn a_blocked_endpoint_is_never_contacted() {
    let recorder = Recorder::start(UNAUTHORIZED);
    let mut rig = Rig::new();
    entry_to(&mut rig, recorder.port);
    rig.cfg.network_policy.offline = true;
    let registry = rig.registry();

    // Resolution refuses it.
    let Err(problems) = registry.resolve_mt(Lang::Zh, Lang::En) else {
        panic!("must be blocked")
    };
    assert_eq!(problems.len(), 1);
    assert!(
        matches!(&problems[0].kind, ProblemKind::EndpointBlocked { host, .. } if host == "0.0.0.0")
    );

    // So does construction, even from a spec resolved while online.
    let mut online = rig.cfg.clone();
    online.network_policy.offline = false;
    registry.reload(&online);
    let spec = registry.resolve_mt(Lang::Zh, Lang::En).unwrap().primary;
    registry.reload(&rig.cfg);
    let Err(err) = registry.build_mt(&spec) else {
        panic!("must be blocked")
    };
    assert!(
        matches!(&err, BuildError::Blocked { host, .. } if host == "0.0.0.0"),
        "{err:?}"
    );
    assert_eq!(registry.live_engines().mt, 0, "nothing was constructed");

    thread::sleep(Duration::from_millis(300));
    assert_eq!(recorder.accepted(), 0, "no connection was attempted");
}

#[test]
fn the_same_endpoint_online_is_reached_and_a_rejected_key_is_classified() {
    let recorder = Recorder::start(UNAUTHORIZED);
    let mut rig = Rig::new();
    entry_to(&mut rig, recorder.port);
    let registry = rig.registry();
    let spec = registry.resolve_mt(Lang::Zh, Lang::En).unwrap().primary;
    let provider = registry.build_mt(&spec).unwrap();
    assert_eq!(
        recorder.accepted(),
        0,
        "building a provider does not connect"
    );
    let report = test_translator(&*provider, Lang::Zh, Lang::En, Duration::from_secs(10));
    assert!(!report.ok);
    assert_eq!(report.error.unwrap().class, FailureClass::Unauthorized);
    assert_eq!(
        recorder.accepted(),
        1,
        "the control: this request does reach the listener"
    );
}

#[test]
fn a_refused_connection_is_classified_unreachable() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let mut rig = Rig::new();
    rig.cfg.providers.entries.push(toml_entry(&format!(
        "id = \"down\"\nprotocol = \"openai-chat\"\nbase_url = \"http://127.0.0.1:{port}/v1\"\nmodel = \"m\"\n"
    )));
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "down".into());
    let registry = rig.registry();
    let spec = registry.resolve_mt(Lang::Zh, Lang::En).unwrap().primary;
    let report = test_translator(
        &*registry.build_mt(&spec).unwrap(),
        Lang::Zh,
        Lang::En,
        Duration::from_secs(10),
    );
    assert_eq!(report.error.unwrap().class, FailureClass::Unreachable);
}

#[test]
fn the_default_local_server_is_allowed_while_offline() {
    let mut rig = Rig::new();
    rig.cfg.network_policy.offline = true;
    let registry = rig.registry();
    let plan = registry
        .resolve_call()
        .expect("loopback is allowed offline");
    assert!(registry.build_mt(&plan.outgoing.mt.primary).is_ok());
    let _ = &mut rig;
}
