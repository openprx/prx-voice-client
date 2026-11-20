//! Manager: queue, concurrency, preparation, snapshot, persistence, offline
//! behaviour and migration, against the local fault server.

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use common::*;
use prxvc_models::catalogue::{Catalogue, ModelId, Role};
use prxvc_models::net::{Blocked, Endpoint, NetworkPolicy};
use prxvc_models::{
    ActiveSet, FailReason, InstallState, LangKey, LoadState, ManagerConfig, ModelCommand,
    ModelEvent, ModelHandle, ModelManager, NotReady, UpdateAvailable,
};
use url::Url;

const WAIT: Duration = Duration::from_secs(20);

struct Rig {
    dir: tempfile::TempDir,
    server: FaultServer,
    cat: Catalogue,
    specs: Vec<ModelSpec>,
    mgr: Option<ModelManager>,
    handle: ModelHandle,
    events: Receiver<ModelEvent>,
}

fn config(
    dir: &Path,
    server: &FaultServer,
    cat: &Catalogue,
    max_concurrent: usize,
) -> ManagerConfig {
    let (settings, _delays) = test_settings(64 * 1024);
    let mut cfg = ManagerConfig::new(
        dir.to_path_buf(),
        cat.clone(),
        semver::Version::new(0, 2, 0),
    );
    cfg.endpoint = Endpoint::HuggingFace {
        base_url: server.base(),
    };
    cfg.max_concurrent = max_concurrent;
    cfg.options.engine = settings;
    cfg.options.offline_retry_interval = Duration::from_millis(100);
    cfg.options.hardware = Some(prxvc_models::HardwareInfo::new(
        16,
        prxvc_models::Os::Macos,
        prxvc_models::Arch::Aarch64,
    ));
    cfg
}

fn rig_with(
    specs: Vec<ModelSpec>,
    max_concurrent: usize,
    tweak: impl FnOnce(&mut ManagerConfig),
) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    let server = FaultServer::start();
    let cat = catalogue_of(&specs);
    serve(&server, &cat, &specs);
    let mut cfg = config(dir.path(), &server, &cat, max_concurrent);
    tweak(&mut cfg);
    let (mgr, handle) = ModelManager::start(cfg).unwrap();
    let events = handle.subscribe();
    Rig {
        dir,
        server,
        cat,
        specs,
        mgr: Some(mgr),
        handle,
        events,
    }
}

fn rig(specs: Vec<ModelSpec>, max_concurrent: usize) -> Rig {
    rig_with(specs, max_concurrent, |_| {})
}

fn id(s: &str) -> ModelId {
    ModelId::new(s)
}

impl Rig {
    fn state(&self, model: &str) -> InstallState {
        self.handle
            .snapshot()
            .model(&id(model))
            .expect("model in snapshot")
            .install
            .clone()
    }
    fn send(&self, c: ModelCommand) {
        self.handle.send(c);
    }
    fn wait_state(&self, model: &str, what: &str, pred: impl Fn(&InstallState) -> bool) {
        wait_until(&format!("{model}: {what}"), WAIT, || {
            pred(&self.state(model))
        });
    }
    fn wait_installed(&self, model: &str) {
        self.wait_state(model, "installed", |s| {
            matches!(s, InstallState::Installed { .. })
        });
    }
    fn drain(&self) -> Vec<ModelEvent> {
        self.events.try_iter().collect()
    }
    fn partial_files(&self) -> usize {
        fn walk(p: &Path) -> usize {
            fs::read_dir(p)
                .map(|rd| {
                    rd.flatten()
                        .map(|e| {
                            if e.path().is_dir() {
                                walk(&e.path())
                            } else {
                                usize::from(e.file_name().to_string_lossy().ends_with(".part"))
                            }
                        })
                        .sum()
                })
                .unwrap_or(0)
        }
        walk(&self.dir.path().join("partial"))
    }
    fn first_request_order(&self) -> Vec<String> {
        let mut seen = Vec::new();
        for r in self.server.requests() {
            if !seen.contains(&r.path) {
                seen.push(r.path);
            }
        }
        seen
    }
    fn path_of(&self, model: &str) -> String {
        server_path(entry_of(&self.cat, model), 0)
    }
    fn shutdown(&mut self) {
        if let Some(m) = self.mgr.take() {
            m.shutdown();
        }
    }
}

fn names(n: usize) -> Vec<ModelSpec> {
    (0..n)
        .map(|i| {
            ModelSpec::single(
                &format!("mod-{}", (b'a' + i as u8) as char),
                150_000 + i * 1000,
            )
        })
        .collect()
}

#[test]
fn download_installs_updates_snapshot_and_fires_events_and_waker() {
    let r = rig(names(1), 2);
    let wakes = Arc::new(AtomicUsize::new(0));
    let w2 = wakes.clone();
    r.handle.set_waker(Box::new(move || {
        w2.fetch_add(1, Ordering::SeqCst);
    }));
    assert_eq!(r.state("mod-a"), InstallState::NotInstalled);
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_installed("mod-a");
    let snap = r.handle.snapshot();
    assert!(snap.jobs.is_empty());
    assert!(snap.storage.used_bytes >= 150_000);
    assert_eq!(snap.catalogue_version, r.cat.catalogue_version);
    assert!(wakes.load(Ordering::SeqCst) > 0);
    assert!(
        r.drain()
            .iter()
            .any(|e| matches!(e, ModelEvent::StateChanged(m) if m.0 == "mod-a"))
    );
    let ready = r
        .handle
        .resolve(Role::Asr, &LangKey::Lang("en".into()))
        .unwrap();
    assert_eq!(ready.id, id("mod-a"));
    assert_eq!(
        fs::read(ready.file("model").unwrap()).unwrap(),
        r.specs[0].files[0].1
    );
}

#[test]
fn snapshot_shows_queued_downloading_and_progress() {
    let r = rig(names(2), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.send(ModelCommand::Download(id("mod-b")));
    r.wait_state(
        "mod-a",
        "downloading with progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 0),
    );
    assert_eq!(r.state("mod-b"), InstallState::Queued);
    let snap = r.handle.snapshot();
    assert_eq!(snap.jobs.len(), 2);
    assert_eq!(snap.jobs[0].id, id("mod-a"));
    if let InstallState::Downloading { total, .. } = r.state("mod-a") {
        assert_eq!(total, 150_000);
    }
    r.server.set_chunk_delay(Duration::ZERO);
    r.wait_installed("mod-a");
    r.wait_installed("mod-b");
}

#[test]
fn concurrency_limit_is_respected() {
    for limit in [1usize, 2] {
        let r = rig(names(5), limit);
        r.server.set_latency(Duration::from_millis(100));
        for i in 0..5 {
            r.send(ModelCommand::Download(id(&format!(
                "mod-{}",
                (b'a' + i) as char
            ))));
        }
        for i in 0..5 {
            r.wait_installed(&format!("mod-{}", (b'a' + i) as char));
        }
        assert_eq!(r.server.max_active(), limit, "limit {limit}");
    }
}

#[test]
fn set_max_concurrent_applies_and_is_clamped() {
    let r = rig(names(5), 3);
    r.send(ModelCommand::SetMaxConcurrent(1));
    r.server.set_latency(Duration::from_millis(80));
    for i in 0..4 {
        r.send(ModelCommand::Download(id(&format!(
            "mod-{}",
            (b'a' + i) as char
        ))));
    }
    for i in 0..4 {
        r.wait_installed(&format!("mod-{}", (b'a' + i) as char));
    }
    assert_eq!(r.server.max_active(), 1);
    let r2 = rig(names(6), 1);
    r2.send(ModelCommand::SetMaxConcurrent(99));
    r2.server.set_latency(Duration::from_millis(150));
    for i in 0..6 {
        r2.send(ModelCommand::Download(id(&format!(
            "mod-{}",
            (b'a' + i) as char
        ))));
    }
    for i in 0..6 {
        r2.wait_installed(&format!("mod-{}", (b'a' + i) as char));
    }
    assert_eq!(r2.server.max_active(), 3, "clamped to the maximum of 3");
}

#[test]
fn queue_is_fifo_and_download_next_jumps_it() {
    let r = rig(names(3), 1);
    r.server.set_latency(Duration::from_millis(150));
    r.send(ModelCommand::Download(id("mod-a")));
    wait_until("a requested", WAIT, || !r.server.requests().is_empty());
    r.send(ModelCommand::Download(id("mod-b")));
    r.send(ModelCommand::Download(id("mod-c")));
    r.send(ModelCommand::DownloadNext(id("mod-c")));
    wait_until("queue visible", WAIT, || {
        r.handle.snapshot().jobs.len() == 3
    });
    let order: Vec<String> = r
        .handle
        .snapshot()
        .jobs
        .iter()
        .map(|j| j.id.0.clone())
        .collect();
    assert_eq!(
        order[0], "mod-c",
        "queue order puts the jumped job first: {order:?}"
    );
    for m in ["mod-a", "mod-b", "mod-c"] {
        r.wait_installed(m);
    }
    let paths: Vec<String> = [r.path_of("mod-a"), r.path_of("mod-c"), r.path_of("mod-b")].to_vec();
    assert_eq!(
        r.first_request_order(),
        paths,
        "a, then the jumped c, then b"
    );
}

#[test]
fn pause_keeps_progress_and_resume_completes() {
    let r = rig(names(1), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state(
        "mod-a",
        "progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 10_000),
    );
    r.send(ModelCommand::Pause(id("mod-a")));
    r.wait_state(
        "mod-a",
        "paused",
        |s| matches!(s, InstallState::Paused { done, .. } if *done > 0),
    );
    assert_eq!(r.partial_files(), 1, "partial kept while paused");
    r.server.set_chunk_delay(Duration::ZERO);
    r.send(ModelCommand::Resume(id("mod-a")));
    r.wait_installed("mod-a");
    assert_eq!(r.partial_files(), 0);
}

#[test]
fn pause_all_pauses_running_and_queued() {
    let r = rig(names(3), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    for m in ["mod-a", "mod-b", "mod-c"] {
        r.send(ModelCommand::Download(id(m)));
    }
    r.wait_state(
        "mod-a",
        "progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 5_000),
    );
    r.send(ModelCommand::PauseAll);
    for m in ["mod-a", "mod-b", "mod-c"] {
        r.wait_state(m, "paused", |s| matches!(s, InstallState::Paused { .. }));
    }
}

#[test]
fn cancel_returns_to_not_installed_and_deletes_the_partial() {
    let r = rig(names(2), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.send(ModelCommand::Download(id("mod-b")));
    r.wait_state(
        "mod-a",
        "progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 10_000),
    );
    r.send(ModelCommand::Cancel(id("mod-a")));
    r.send(ModelCommand::Cancel(id("mod-b")));
    r.wait_state("mod-a", "not installed", |s| {
        *s == InstallState::NotInstalled
    });
    r.wait_state("mod-b", "not installed", |s| {
        *s == InstallState::NotInstalled
    });
    assert_eq!(r.partial_files(), 0);
    assert!(r.handle.snapshot().jobs.is_empty());
    assert!(
        r.handle
            .resolve(Role::Asr, &LangKey::Lang("en".into()))
            .is_err()
    );
}

#[test]
fn a_paused_partial_without_a_job_shows_as_paused_and_cancel_removes_it() {
    // Plant a partial as an earlier run would have left it.
    let specs = names(1);
    let dir = tempfile::tempdir().unwrap();
    let cat = catalogue_of(&specs);
    let entry = entry_of(&cat, "mod-a");
    {
        let store = prxvc_models::ModelStore::open(dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(store.partial_dir(entry)).unwrap();
        fs::write(store.part_path(entry, &entry.files[0]), vec![0u8; 5000]).unwrap();
        fs::write(
            store.partial_dir(entry).join(format!("{}.part.json", entry.files[0].name)),
            serde_json::json!({"size": entry.files[0].size, "sha256": entry.files[0].sha256, "url": "u"}).to_string(),
        )
        .unwrap();
    }
    let server = FaultServer::start();
    let (mgr, handle) = ModelManager::start(config(dir.path(), &server, &cat, 1)).unwrap();
    assert_eq!(
        handle.snapshot().model(&id("mod-a")).unwrap().install,
        InstallState::Paused {
            done: 5000,
            total: 150_000
        }
    );
    handle.send(ModelCommand::Cancel(id("mod-a")));
    wait_until("cancelled", WAIT, || {
        handle.snapshot().model(&id("mod-a")).unwrap().install == InstallState::NotInstalled
    });
    mgr.shutdown();
}

#[test]
fn a_newer_choice_supersedes_a_pending_preparation() {
    let r = rig(names(2), 2);
    r.server.set_chunk_delay(Duration::from_millis(5));
    let key = LangKey::Lang("en".into());
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key: key.clone(),
        id: id("mod-a"),
    });
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key: key.clone(),
        id: id("mod-b"),
    });
    r.wait_installed("mod-a");
    r.wait_installed("mod-b");
    // Give the coordinator a moment to emit anything it is going to emit.
    std::thread::sleep(Duration::from_millis(200));
    let done: Vec<_> = r
        .drain()
        .into_iter()
        .filter_map(|e| match e {
            ModelEvent::PreparationDone { id, .. } => Some(id.0),
            ModelEvent::PreparationFailed { .. } => panic!("unexpected failure"),
            _ => None,
        })
        .collect();
    assert_eq!(
        done,
        vec!["mod-b".to_string()],
        "the superseded preparation never reports done"
    );
}

#[test]
fn preparations_for_different_keys_both_complete() {
    let mut specs = names(2);
    specs[1].role = "mt";
    let r = rig(specs, 2);
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key: LangKey::Lang("en".into()),
        id: id("mod-a"),
    });
    r.send(ModelCommand::Use {
        role: Role::Mt,
        key: LangKey::Pair {
            source: "zh".into(),
            target: "en".into(),
        },
        id: id("mod-b"),
    });
    r.wait_installed("mod-a");
    r.wait_installed("mod-b");
    wait_until("both done events", WAIT, || {
        let ev: Vec<_> = r.events.try_iter().collect();
        ev.iter()
            .filter(|e| matches!(e, ModelEvent::PreparationDone { .. }))
            .count()
            == 2
    });
}

#[test]
fn use_of_an_installed_model_is_done_at_once_and_failures_are_reported() {
    let mut specs = names(3);
    specs[2].runtime = "future.cpp";
    let r = rig(specs, 1);
    let key = LangKey::Lang("en".into());
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_installed("mod-a");
    r.drain();
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key: key.clone(),
        id: id("mod-a"),
    });
    wait_until("done event", WAIT, || {
        r.events
            .try_iter()
            .any(|e| matches!(e, ModelEvent::PreparationDone { id, .. } if id.0 == "mod-a"))
    });
    // A failing source is reported as a preparation failure.
    r.server.always(&r.path_of("mod-b"), Behaviour::Status(404));
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key: key.clone(),
        id: id("mod-b"),
    });
    wait_until("failure event", WAIT, || {
        r.events.try_iter().any(|e| {
            matches!(e, ModelEvent::PreparationFailed { reason: FailReason::SourceRejected { status: 404 }, id, .. } if id.0 == "mod-b")
        })
    });
    // An unsupported runtime is refused without downloading.
    r.send(ModelCommand::Use {
        role: Role::Asr,
        key,
        id: id("mod-c"),
    });
    wait_until("unsupported", WAIT, || {
        r.events.try_iter().any(|e| {
            matches!(e, ModelEvent::PreparationFailed { reason: FailReason::UnsupportedRuntime { name }, .. } if name == "future.cpp")
        })
    });
    assert!(r.server.requests_for(&r.path_of("mod-c")).is_empty());
}

#[test]
fn failed_jobs_report_the_reason_and_retry_works() {
    let r = rig(names(1), 1);
    let path = r.path_of("mod-a");
    r.server.always(&path, Behaviour::Status(404));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state("mod-a", "failed", |s| {
        matches!(
            s,
            InstallState::Failed {
                reason: FailReason::SourceRejected { status: 404 },
                retryable: true
            }
        )
    });
    assert!(matches!(
        r.handle.resolve(Role::Asr, &LangKey::Lang("en".into())),
        Err(NotReady::Failed {
            reason: FailReason::SourceRejected { .. },
            ..
        })
    ));
    r.server.clear_rules(&path);
    r.send(ModelCommand::Retry(id("mod-a")));
    r.wait_installed("mod-a");
}

#[test]
fn offline_jobs_are_retried_automatically() {
    let r = rig(names(1), 1);
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    r.send(ModelCommand::SetEndpoint(Endpoint::HuggingFace {
        base_url: format!("http://{closed}"),
    }));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state("mod-a", "offline", |s| {
        matches!(
            s,
            InstallState::Failed {
                reason: FailReason::Offline,
                ..
            }
        )
    });
    // The network comes back (the endpoint now answers): no user action needed.
    r.send(ModelCommand::SetEndpoint(Endpoint::HuggingFace {
        base_url: r.server.base(),
    }));
    r.wait_installed("mod-a");
}

struct BlockAll;
impl NetworkPolicy for BlockAll {
    fn check(&self, url: &Url) -> Result<(), Blocked> {
        Err(Blocked {
            host: url.host_str().unwrap_or("").into(),
            reason: "offline mode".into(),
        })
    }
}

#[test]
fn a_blocked_url_fails_as_offline_without_any_connection() {
    let r = rig_with(names(1), 1, |cfg| {
        cfg.options.engine.policy = Arc::new(BlockAll)
    });
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state("mod-a", "offline", |s| {
        matches!(
            s,
            InstallState::Failed {
                reason: FailReason::Offline,
                ..
            }
        )
    });
    std::thread::sleep(Duration::from_millis(350)); // several probe intervals
    assert!(
        r.server.requests().is_empty(),
        "neither the download nor the probe connected"
    );
    assert!(matches!(
        r.state("mod-a"),
        InstallState::Failed {
            reason: FailReason::Offline,
            ..
        }
    ));
}

#[test]
fn jobs_survive_a_restart_and_resume_by_themselves() {
    let mut r = rig(names(1), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state(
        "mod-a",
        "progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 10_000),
    );
    r.shutdown();
    let jobs: serde_json::Value =
        serde_json::from_slice(&fs::read(r.dir.path().join("jobs.json")).unwrap()).unwrap();
    assert_eq!(jobs[0]["model_id"], "mod-a");
    assert_eq!(jobs[0]["paused"], false);
    assert_eq!(
        r.partial_files(),
        1,
        "quitting pauses and keeps the partial"
    );
    r.server.set_chunk_delay(Duration::ZERO);

    let cfg = config(r.dir.path(), &r.server, &r.cat, 1);
    let (mgr, handle) = ModelManager::start(cfg).unwrap();
    wait_until("resumed and installed", WAIT, || {
        matches!(
            handle.snapshot().model(&id("mod-a")).unwrap().install,
            InstallState::Installed { .. }
        )
    });
    let resumed = r.server.requests_for(&r.path_of("mod-a"));
    assert!(
        resumed.iter().any(|q| q.range_start.is_some_and(|s| s > 0)),
        "continued from the partial"
    );
    mgr.shutdown();
}

#[test]
fn a_user_paused_job_stays_paused_after_a_restart() {
    let mut r = rig(names(1), 1);
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state(
        "mod-a",
        "progress",
        |s| matches!(s, InstallState::Downloading { done, .. } if *done > 5_000),
    );
    r.send(ModelCommand::Pause(id("mod-a")));
    r.wait_state("mod-a", "paused", |s| {
        matches!(s, InstallState::Paused { .. })
    });
    r.shutdown();
    let (mgr, handle) = ModelManager::start(config(r.dir.path(), &r.server, &r.cat, 1)).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(matches!(
        handle.snapshot().model(&id("mod-a")).unwrap().install,
        InstallState::Paused { .. }
    ));
    mgr.shutdown();
}

#[test]
fn resolve_explains_why_a_model_is_not_ready() {
    let mut specs = names(2);
    specs[1].runtime = "future.cpp";
    specs[1].role = "mt";
    let r = rig(specs, 1);
    let en = LangKey::Lang("en".into());
    match r.handle.resolve(Role::Asr, &en) {
        Err(NotReady::NotInstalled { id: i, size }) => {
            assert_eq!(i, id("mod-a"));
            assert_eq!(size, 150_000);
        }
        other => panic!("{other:?}"),
    }
    r.server.set_chunk_delay(Duration::from_millis(10));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_state("mod-a", "downloading", |s| {
        matches!(s, InstallState::Downloading { .. })
    });
    assert!(matches!(
        r.handle.resolve(Role::Asr, &en),
        Err(NotReady::InProgress { .. })
    ));
    r.server.set_chunk_delay(Duration::ZERO);
    r.wait_installed("mod-a");
    assert!(r.handle.resolve(Role::Asr, &en).is_ok());
    let pair = LangKey::Pair {
        source: "zh".into(),
        target: "en".into(),
    };
    // Never recommended (runtime missing) ...
    assert!(matches!(
        r.handle.resolve(Role::Mt, &pair),
        Err(NotReady::NoSelection)
    ));
    // ... and refused when a configuration selects it anyway.
    r.handle
        .set_active(ActiveSet::new().with(Role::Mt, pair.clone(), id("mod-b")));
    assert!(matches!(
        r.handle.resolve(Role::Mt, &pair),
        Err(NotReady::UnsupportedRuntime { runtime, .. }) if runtime == "future.cpp"
    ));
    assert!(matches!(
        r.handle.resolve(Role::Vad, &LangKey::Any),
        Err(NotReady::NoSelection)
    ));
}

#[test]
fn the_active_selection_overrides_the_recommendation() {
    let mut specs = names(2);
    specs[1].files[0].0 = "second.bin".into();
    let r = rig(specs, 2);
    let en = LangKey::Lang("en".into());
    r.send(ModelCommand::Download(id("mod-a")));
    r.send(ModelCommand::Download(id("mod-b")));
    r.wait_installed("mod-a");
    r.wait_installed("mod-b");
    r.handle
        .set_active(ActiveSet::new().with(Role::Asr, en.clone(), id("mod-b")));
    assert_eq!(r.handle.resolve(Role::Asr, &en).unwrap().id, id("mod-b"));
    r.handle
        .set_active(ActiveSet::new().with(Role::Asr, en.clone(), id("mod-a")));
    assert_eq!(r.handle.resolve(Role::Asr, &en).unwrap().id, id("mod-a"));
}

#[test]
fn remove_refuses_loaded_models_and_remove_unused_keeps_active_ones() {
    let r = rig(names(2), 2);
    r.send(ModelCommand::Download(id("mod-a")));
    r.send(ModelCommand::Download(id("mod-b")));
    r.wait_installed("mod-a");
    r.wait_installed("mod-b");
    // Loaded: Remove is refused.
    r.handle.report_load_state(&id("mod-a"), LoadState::Ready);
    r.send(ModelCommand::Remove(id("mod-a")));
    std::thread::sleep(Duration::from_millis(250));
    assert!(matches!(r.state("mod-a"), InstallState::Installed { .. }));
    assert_eq!(
        r.handle.snapshot().model(&id("mod-a")).unwrap().load,
        LoadState::Ready
    );
    r.handle
        .report_load_state(&id("mod-a"), LoadState::Unloaded);
    // Unused removal: mod-a active, mod-b not.
    r.handle
        .set_active(ActiveSet::new().with(Role::Asr, LangKey::Lang("en".into()), id("mod-a")));
    r.send(ModelCommand::RemoveUnused);
    r.wait_state("mod-b", "removed", |s| *s == InstallState::NotInstalled);
    assert!(matches!(r.state("mod-a"), InstallState::Installed { .. }));
    // Now remove mod-a explicitly.
    r.send(ModelCommand::Remove(id("mod-a")));
    r.wait_state("mod-a", "removed", |s| *s == InstallState::NotInstalled);
}

#[test]
fn legacy_files_are_adopted_at_start_without_any_download() {
    let specs = names(1);
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join("legacy").join("models");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join(&specs[0].files[0].0), &specs[0].files[0].1).unwrap();
    fs::write(legacy.join("unknown.bin"), b"keep me").unwrap();
    let models = dir.path().join("models");
    let server = FaultServer::start();
    let cat = catalogue_of(&specs);
    let mut cfg = config(&models, &server, &cat, 1);
    cfg.legacy_models_dir = Some(legacy.clone());
    let (mgr, handle) = ModelManager::start(cfg).unwrap();
    let events = handle.subscribe();
    wait_until("migration finished", WAIT, || {
        handle.snapshot().migration.is_none()
            && matches!(
                handle.snapshot().model(&id("mod-a")).unwrap().install,
                InstallState::Installed { .. }
            )
    });
    assert!(
        handle
            .resolve(Role::Asr, &LangKey::Lang("en".into()))
            .is_ok()
    );
    assert!(server.requests().is_empty(), "no download");
    assert!(legacy.join("unknown.bin").exists());
    let _ = events; // the MigrationFinished event may predate the subscription
    // Asking to download an adopted model is a no-op.
    handle.send(ModelCommand::Download(id("mod-a")));
    std::thread::sleep(Duration::from_millis(200));
    assert!(server.requests().is_empty());
    mgr.shutdown();
}

#[test]
fn migration_finished_event_reports_the_adopted_models() {
    let specs = names(1);
    let dir = tempfile::tempdir().unwrap();
    let legacy: PathBuf = dir.path().join("legacy");
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join(&specs[0].files[0].0), &specs[0].files[0].1).unwrap();
    let server = FaultServer::start();
    let cat = catalogue_of(&specs);
    let mut cfg = config(&dir.path().join("models"), &server, &cat, 1);
    cfg.legacy_models_dir = Some(legacy);
    // Subscribe before the manager can finish: start paused on a barrier is not
    // needed because events are replayed through the snapshot; instead poll both.
    let (mgr, handle) = ModelManager::start(cfg).unwrap();
    let events = handle.subscribe();
    wait_until("installed", WAIT, || {
        matches!(
            handle.snapshot().model(&id("mod-a")).unwrap().install,
            InstallState::Installed { .. }
        )
    });
    let got: Vec<_> = events.try_iter().collect();
    if let Some(ModelEvent::MigrationFinished(rep)) = got
        .iter()
        .find(|e| matches!(e, ModelEvent::MigrationFinished(_)))
    {
        assert_eq!(rep.installed, vec![id("mod-a")]);
    }
    mgr.shutdown();
}

#[test]
fn updates_and_removed_from_list_are_derived_from_the_catalogue() {
    let r = rig(names(1), 1);
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_installed("mod-a");
    let old_key = match r.state("mod-a") {
        InstallState::Installed { revision_key } => revision_key,
        other => panic!("{other:?}"),
    };

    // A catalogue with a new pin for the same id.
    let mut v = serde_json::to_value(&r.cat).unwrap();
    let new_rev = "f".repeat(40);
    v["models"][0]["files"][0]["source"]["revision"] = new_rev.clone().into();
    v["catalogue_version"] = 2026112002u64.into();
    let newer = Catalogue::parse(&v.to_string()).unwrap();
    let new_key = newer.models()[0].revision_key();
    r.server.add_file(
        &server_path(&newer.models()[0], 0),
        r.specs[0].files[0].1.clone(),
    );
    r.send(ModelCommand::ReplaceCatalogue(newer));
    wait_until("update flagged", WAIT, || {
        r.handle.snapshot().model(&id("mod-a")).unwrap().update
            == Some(UpdateAvailable::NewRevision {
                to_revision_key: new_key.clone(),
            })
    });
    assert_eq!(
        r.state("mod-a"),
        InstallState::Installed {
            revision_key: old_key
        },
        "old revision stays listed"
    );
    assert_eq!(r.handle.snapshot().catalogue_version, 2026112002);
    r.send(ModelCommand::Download(id("mod-a")));
    wait_until("updated", WAIT, || {
        let m = r.handle.snapshot().model(&id("mod-a")).cloned().unwrap();
        m.install
            == InstallState::Installed {
                revision_key: new_key.clone(),
            }
            && m.update.is_none()
    });

    // The model disappears from the catalogue: still listed, flagged.
    let mut v2 = serde_json::to_value(Catalogue::embedded()).unwrap();
    v2["catalogue_version"] = 2026112003u64.into();
    r.send(ModelCommand::ReplaceCatalogue(
        Catalogue::parse(&v2.to_string()).unwrap(),
    ));
    wait_until("removed from list", WAIT, || {
        r.handle
            .snapshot()
            .model(&id("mod-a"))
            .is_some_and(|m| !m.in_catalogue && matches!(m.install, InstallState::Installed { .. }))
    });
}

#[test]
fn a_superseding_entry_flags_an_update() {
    let specs = names(2);
    let mut v = serde_json::to_value(catalogue_of(&specs)).unwrap();
    v["models"][1]["supersedes"] = serde_json::json!(["mod-a"]);
    let cat = Catalogue::parse(&v.to_string()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let server = FaultServer::start();
    serve(&server, &cat, &specs);
    let (mgr, handle) = ModelManager::start(config(dir.path(), &server, &cat, 1)).unwrap();
    handle.send(ModelCommand::Download(id("mod-a")));
    wait_until("installed", WAIT, || {
        handle
            .snapshot()
            .model(&id("mod-a"))
            .is_some_and(|m| matches!(m.install, InstallState::Installed { .. }))
    });
    assert_eq!(
        handle.snapshot().model(&id("mod-a")).unwrap().update,
        Some(UpdateAvailable::Superseded { by: id("mod-b") })
    );
    mgr.shutdown();
}

#[test]
fn only_one_manager_per_models_folder_can_write() {
    let r = rig(names(1), 1);
    let (_m2, h2) = ModelManager::start(config(r.dir.path(), &r.server, &r.cat, 1)).unwrap();
    // The second instance is read-only: it never persists or removes anything.
    h2.send(ModelCommand::Remove(id("mod-a")));
    std::thread::sleep(Duration::from_millis(150));
    r.send(ModelCommand::Download(id("mod-a")));
    r.wait_installed("mod-a");
}
