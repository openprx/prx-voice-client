//! Live checks of the manager against the real `llama-server` and the installed model. Ignored by
//! default; run one at a time:
//!
//! ```text
//! cargo test -p prxvc-desktop --bin prxvc-desktop llm_server::live_tests -- --ignored --test-threads=1 --nocapture
//! ```
//!
//! The owner's own server on port 8080 is never stopped: when it answers there, the adoption test
//! runs and the start-and-stop test uses a spare port instead. Neither leaves a process behind.

use std::time::Duration;

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;

use super::boot::{build, settings};
use super::core::ServerState;
use super::ports::{PortState, Probe};
use super::real::HttpProbe;

fn paths(dir: &tempfile::TempDir) -> AppPaths {
    // The model store of this machine, a private data directory for the record and the log.
    let real = AppPaths::resolve(&AppConfig::default()).expect("application directories");
    AppPaths::from_roots(
        dir.path().join("config"),
        dir.path().join("data"),
        real.models_dir,
        &AppConfig::default(),
    )
}

#[test]
#[ignore = "needs the installed model; adopts the server on port 8080 when it runs"]
fn adopts_the_server_that_is_already_running_and_leaves_it_running() {
    if HttpProbe.probe(8080, super::boot::MODEL_ID) != PortState::Compatible {
        println!("skipped: no compatible server answers on port 8080");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = paths(&dir);
    let server = build(
        settings(&AppConfig::default(), &p),
        String::new(),
        p.models_dir.clone(),
    );
    let lease = server.acquire();
    lease.wait_ready(Duration::from_secs(10)).expect("adopted");
    assert_eq!(
        server.snapshot().state,
        ServerState::Ready { adopted: true }
    );
    assert!(
        !dir.path().join("data/llm-server/server.json").exists(),
        "nothing of ours to record"
    );
    drop(lease);
    server.set_pinned(false);
    server.shutdown();
    assert_eq!(
        HttpProbe.probe(8080, super::boot::MODEL_ID),
        PortState::Compatible,
        "the owner's server was not stopped"
    );
}

#[test]
#[ignore = "starts the real llama-server with the real model on a spare port, once"]
fn starts_and_stops_the_real_server_on_a_spare_port() {
    const PORT: u16 = 18_471;
    if HttpProbe.probe(PORT, super::boot::MODEL_ID) != PortState::Closed {
        println!("skipped: the spare port {PORT} is in use");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let p = paths(&dir);
    let mut s = settings(&AppConfig::default(), &p);
    s.port = PORT;
    let record = s.record.clone();
    let server = build(s, String::new(), p.models_dir.clone());
    let attempt = server.set_pinned(true);
    match server.wait_ready(attempt, Duration::from_secs(150)) {
        Ok(()) => {}
        Err(problem) => {
            server.shutdown();
            panic!("the real server did not become ready: {problem}");
        }
    }
    assert_eq!(
        server.snapshot().state,
        ServerState::Ready { adopted: false }
    );
    assert!(record.exists(), "the record of our process");
    let pid = super::record::ServerRecord::read(&record)
        .expect("record")
        .pid;
    println!("real server ready, pid {pid}");
    assert_eq!(
        HttpProbe.probe(PORT, super::boot::MODEL_ID),
        PortState::Compatible
    );

    server.set_pinned(false);
    let until = std::time::Instant::now() + Duration::from_secs(30);
    while server.snapshot().state != ServerState::Stopped && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(server.snapshot().state, ServerState::Stopped);
    assert_eq!(
        HttpProbe.probe(PORT, super::boot::MODEL_ID),
        PortState::Closed,
        "the port is free again"
    );
    assert!(!record.exists());
    server.shutdown();
}
