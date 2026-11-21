//! Download engine against the local fault server (no real network).

mod common;

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use common::*;
use prxvc_models::catalogue::{Catalogue, ModelEntry};
use prxvc_models::control::{JobControl, JobProgress, Stop};
use prxvc_models::download::{DownloadEngine, EngineSettings, JobOutcome, JobSpec};
use prxvc_models::net::{Blocked, Endpoint, NetworkPolicy};
use prxvc_models::store::ModelStore;
use prxvc_models::{FailReason, StoreInstallState};
use url::Url;

struct Fixture {
    _dir: tempfile::TempDir,
    store: Arc<ModelStore>,
    server: FaultServer,
    cat: Catalogue,
    specs: Vec<ModelSpec>,
    delays: Delays,
    engine: Arc<DownloadEngine>,
}

fn fixture_with(
    specs: Vec<ModelSpec>,
    piece: u64,
    tweak: impl FnOnce(&mut EngineSettings),
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let server = FaultServer::start();
    let cat = catalogue_of(&specs);
    serve(&server, &cat, &specs);
    let (mut settings, delays) = test_settings(piece);
    tweak(&mut settings);
    let store = Arc::new(ModelStore::open(dir.path().to_path_buf()).unwrap());
    let engine = Arc::new(DownloadEngine::new(settings).unwrap());
    Fixture {
        _dir: dir,
        store,
        server,
        cat,
        specs,
        delays,
        engine,
    }
}

fn fixture(specs: Vec<ModelSpec>, piece: u64) -> Fixture {
    fixture_with(specs, piece, |_| {})
}

impl Fixture {
    fn entry(&self, id: &str) -> ModelEntry {
        entry_of(&self.cat, id).clone()
    }
    fn endpoint(&self) -> Endpoint {
        Endpoint::HuggingFace {
            base_url: self.server.base(),
        }
    }
    fn run(&self, id: &str) -> JobOutcome {
        self.run_with(id, &JobControl::new(), &JobProgress::new())
    }
    fn run_with(&self, id: &str, ctl: &JobControl, progress: &JobProgress) -> JobOutcome {
        let entry = self.entry(id);
        let endpoint = self.endpoint();
        self.engine.run_job(
            &self.store,
            JobSpec {
                entry: &entry,
                catalogue_version: self.cat.catalogue_version,
                endpoint: &endpoint,
            },
            ctl,
            progress,
        )
    }
    fn data(&self, id: &str, idx: usize) -> &[u8] {
        &self.specs.iter().find(|s| s.id == id).unwrap().files[idx].1
    }
    fn part_files(&self) -> Vec<String> {
        let mut out = Vec::new();
        fn walk(p: &Path, out: &mut Vec<String>) {
            if let Ok(rd) = fs::read_dir(p) {
                for e in rd.flatten() {
                    if e.path().is_dir() {
                        walk(&e.path(), out);
                    } else if e.file_name().to_string_lossy().ends_with(".part") {
                        out.push(e.path().display().to_string());
                    }
                }
            }
        }
        walk(&self.store.partial_root(), &mut out);
        out
    }
}

fn secs(d: &[Duration]) -> Vec<u64> {
    d.iter().map(|d| d.as_secs()).collect()
}

#[test]
fn full_download_installs_and_verifies() {
    let f = fixture(vec![ModelSpec::single("m1", 200_000)], 32 * 1024);
    let entry = f.entry("m1");
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let ready = f.store.ready(&entry).expect("ready after install");
    assert_eq!(
        fs::read(ready.file("model").unwrap()).unwrap(),
        f.data("m1", 0)
    );
    let reqs = f.server.requests_for(&server_path(&entry, 0));
    assert!(
        reqs.len() >= 7,
        "multiple pieces expected, got {}",
        reqs.len()
    );
    assert!(
        reqs.iter().all(|r| r.range_start.is_some()),
        "every request is ranged"
    );
    assert!(f.part_files().is_empty(), "no partial left behind");
    assert!(matches!(
        f.store.install_state(&entry),
        StoreInstallState::Installed { .. }
    ));
}

#[test]
fn resume_after_dropped_connection_continues_from_partial() {
    let f = fixture(vec![ModelSpec::single("m1", 100_000)], 1 << 20);
    let entry = f.entry("m1");
    let path = server_path(&entry, 0);
    f.server.first(&path, Behaviour::DropAfter(40_000), 1);
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let reqs = f.server.requests_for(&path);
    assert_eq!(reqs.len(), 2);
    assert_eq!(reqs[0].range_start, Some(0));
    assert_eq!(
        reqs[1].range_start,
        Some(40_000),
        "second request resumes where the first stopped"
    );
    assert_eq!(secs(&f.delays.lock().unwrap()), vec![1]);
    let ready = f.store.ready(&entry).unwrap();
    assert_eq!(
        fs::read(ready.file("model").unwrap()).unwrap(),
        f.data("m1", 0)
    );
}

/// Writes a partial and sidecar by hand, as an earlier run would have left them.
fn plant_partial(f: &Fixture, entry: &ModelEntry, bytes: &[u8], sha: &str) {
    let dir = f.store.partial_dir(entry);
    fs::create_dir_all(&dir).unwrap();
    let file = &entry.files[0];
    fs::write(f.store.part_path(entry, file), bytes).unwrap();
    let side = serde_json::json!({"size": file.size, "sha256": sha, "url": "x"});
    fs::write(
        dir.join(format!("{}.part.json", file.name)),
        side.to_string(),
    )
    .unwrap();
}

#[test]
fn existing_partial_is_resumed() {
    let f = fixture(vec![ModelSpec::single("m1", 100_000)], 1 << 20);
    let entry = f.entry("m1");
    plant_partial(
        &f,
        &entry,
        &f.data("m1", 0)[..60_000],
        &entry.files[0].sha256,
    );
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let reqs = f.server.requests_for(&server_path(&entry, 0));
    assert_eq!(reqs[0].range_start, Some(60_000));
    assert!(f.store.ready(&entry).is_some());
}

#[test]
fn sidecar_with_changed_pin_invalidates_the_partial() {
    let f = fixture(vec![ModelSpec::single("m1", 100_000)], 1 << 20);
    let entry = f.entry("m1");
    plant_partial(&f, &entry, &f.data("m1", 0)[..60_000], &"0".repeat(64));
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let reqs = f.server.requests_for(&server_path(&entry, 0));
    assert_eq!(reqs[0].range_start, Some(0), "stale partial discarded");
    assert!(f.store.ready(&entry).is_some());
}

#[test]
fn server_that_ignores_ranges_is_handled() {
    let f = fixture(vec![ModelSpec::single("m1", 120_000)], 32 * 1024);
    let entry = f.entry("m1");
    // A partial exists, but the server answers 200 with the whole body.
    plant_partial(
        &f,
        &entry,
        &f.data("m1", 0)[..50_000],
        &entry.files[0].sha256,
    );
    f.server
        .always(&server_path(&entry, 0), Behaviour::IgnoreRange);
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let ready = f.store.ready(&entry).unwrap();
    assert_eq!(
        fs::read(ready.file("model").unwrap()).unwrap(),
        f.data("m1", 0)
    );
    assert_eq!(f.server.requests_for(&server_path(&entry, 0)).len(), 1);
}

#[test]
fn wrong_content_range_start_fails_the_piece_and_is_retried() {
    let f = fixture(vec![ModelSpec::single("m1", 50_000)], 1 << 20);
    let entry = f.entry("m1");
    f.server
        .first(&server_path(&entry, 0), Behaviour::BadContentRange, 1);
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    assert_eq!(secs(&f.delays.lock().unwrap()), vec![1]);
    assert_eq!(f.server.requests_for(&server_path(&entry, 0)).len(), 2);
}

#[test]
fn server_sending_extra_bytes_fails_the_file() {
    let f = fixture(vec![ModelSpec::single("m1", 50_000)], 1 << 20);
    let entry = f.entry("m1");
    f.server
        .always(&server_path(&entry, 0), Behaviour::Extra(100));
    let out = f.run("m1");
    assert!(
        matches!(out, JobOutcome::Failed(FailReason::Network { .. })),
        "{out:?}"
    );
    assert!(f.store.ready(&entry).is_none());
    for p in f.part_files() {
        assert!(
            fs::metadata(&p).unwrap().len() <= 50_000,
            "never more than size bytes"
        );
    }
    assert!(f.delays.lock().unwrap().is_empty(), "not retried");
}

#[test]
fn rejected_statuses_are_not_retried() {
    for code in [401u16, 403, 404] {
        let f = fixture(vec![ModelSpec::single("m1", 10_000)], 1 << 20);
        let entry = f.entry("m1");
        f.server
            .always(&server_path(&entry, 0), Behaviour::Status(code));
        assert_eq!(
            f.run("m1"),
            JobOutcome::Failed(FailReason::SourceRejected { status: code })
        );
        assert_eq!(
            f.server.requests_for(&server_path(&entry, 0)).len(),
            1,
            "status {code}"
        );
        assert!(f.delays.lock().unwrap().is_empty());
    }
}

#[test]
fn server_errors_and_rate_limits_are_retried_with_backoff() {
    for (code, times, expected) in [(500u16, 3usize, vec![1u64, 2, 4]), (429, 2, vec![1, 2])] {
        let f = fixture(vec![ModelSpec::single("m1", 10_000)], 1 << 20);
        let entry = f.entry("m1");
        f.server
            .first(&server_path(&entry, 0), Behaviour::Status(code), times);
        assert_eq!(f.run("m1"), JobOutcome::Installed, "status {code}");
        assert_eq!(secs(&f.delays.lock().unwrap()), expected);
    }
}

#[test]
fn retries_are_bounded_and_the_partial_survives() {
    let f = fixture(vec![ModelSpec::single("m1", 100_000)], 1 << 20);
    let entry = f.entry("m1");
    let path = server_path(&entry, 0);
    // Every attempt delivers some bytes and then drops, so progress is kept.
    f.server.always(&path, Behaviour::DropAfter(5_000));
    let out = f.run("m1");
    assert!(
        matches!(out, JobOutcome::Failed(FailReason::Network { .. })),
        "{out:?}"
    );
    assert_eq!(
        f.server.requests_for(&path).len(),
        6,
        "six consecutive failures"
    );
    assert_eq!(secs(&f.delays.lock().unwrap()), vec![1, 2, 4, 8, 16]);
    assert!(f.store.ready(&entry).is_none());
    let parts = f.part_files();
    assert_eq!(parts.len(), 1);
    assert_eq!(
        fs::metadata(&parts[0]).unwrap().len(),
        30_000,
        "partial is kept for Retry"
    );
    // Retry resumes from it.
    f.server.clear_rules(&path);
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    assert_eq!(
        f.server.requests_for(&path).last().unwrap().range_start,
        Some(30_000)
    );
}

#[test]
fn redirect_is_followed_and_the_original_url_is_used_after_a_failure() {
    let f = fixture(vec![ModelSpec::single("m1", 100_000)], 32 * 1024);
    let entry = f.entry("m1");
    let origin = server_path(&entry, 0);
    let cdn = "/cdn/m1.bin".to_string();
    f.server.add_file(&cdn, f.data("m1", 0).to_vec());
    f.server.always(
        &origin,
        Behaviour::Redirect(format!("{}{cdn}", f.server.base())),
    );
    // The 2nd request to the CDN host fails once.
    f.server.window(&cdn, Behaviour::Status(500), 1, Some(1));
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let seq: Vec<String> = f.server.requests().into_iter().map(|r| r.path).collect();
    // origin, cdn (ok), cdn (500), then back to the original URL.
    assert_eq!(
        &seq[..4],
        [origin.clone(), cdn.clone(), cdn.clone(), origin.clone()],
        "{seq:?}"
    );
    // Between failures later pieces go straight to the final URL.
    assert_eq!(seq[4], cdn);
    assert_eq!(seq[5], cdn);
    let ready = f.store.ready(&entry).unwrap();
    assert_eq!(
        fs::read(ready.file("model").unwrap()).unwrap(),
        f.data("m1", 0)
    );
}

#[test]
fn checksum_mismatch_is_reported_and_nothing_is_installed() {
    let f = fixture(vec![ModelSpec::single("m1", 40_000)], 1 << 20);
    let entry = f.entry("m1");
    let path = server_path(&entry, 0);
    f.server.always(&path, Behaviour::WrongBytes);
    let out = f.run("m1");
    assert_eq!(
        out,
        JobOutcome::Failed(FailReason::ChecksumMismatch {
            file: entry.files[0].name.clone()
        })
    );
    assert_eq!(
        f.server.requests_for(&path).len(),
        2,
        "one automatic re-download, then reported"
    );
    assert!(f.part_files().is_empty(), "corrupt partial deleted");
    assert!(f.store.ready(&entry).is_none());
    assert!(!f.store.rev_dir(&entry).exists());
}

#[test]
fn one_corrupt_download_is_repaired_by_the_automatic_retry() {
    let f = fixture(vec![ModelSpec::single("m1", 40_000)], 1 << 20);
    let entry = f.entry("m1");
    f.server
        .first(&server_path(&entry, 0), Behaviour::WrongBytes, 1);
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    assert!(f.store.ready(&entry).is_some());
}

#[test]
fn pause_keeps_a_resumable_partial_and_resume_completes() {
    let f = fixture(vec![ModelSpec::single("m1", 400_000)], 64 * 1024);
    let entry = f.entry("m1");
    f.server.set_chunk_delay(Duration::from_millis(15));
    let ctl = JobControl::new();
    let progress = JobProgress::new();
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_with("m1", &ctl, &progress));
        wait_until("some bytes downloaded", Duration::from_secs(10), || {
            progress.done() > 20_000
        });
        ctl.request(Stop::Pause);
        h.join().unwrap()
    });
    assert_eq!(out, JobOutcome::Paused);
    assert!(f.store.ready(&entry).is_none());
    let parts = f.part_files();
    assert_eq!(parts.len(), 1);
    let kept = fs::metadata(&parts[0]).unwrap().len();
    assert!(kept > 0 && kept < 400_000, "kept {kept}");
    f.server.set_chunk_delay(Duration::ZERO);
    let before = f.server.requests().len();
    assert_eq!(f.run("m1"), JobOutcome::Installed);
    let first_after = &f.server.requests()[before];
    assert_eq!(
        first_after.range_start,
        Some(kept),
        "resume starts at the kept partial"
    );
    assert!(f.store.ready(&entry).is_some());
}

#[test]
fn cancel_deletes_the_partial_and_installs_nothing() {
    let f = fixture(vec![ModelSpec::single("m1", 400_000)], 64 * 1024);
    let entry = f.entry("m1");
    f.server.set_chunk_delay(Duration::from_millis(15));
    let ctl = JobControl::new();
    let progress = JobProgress::new();
    let out = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_with("m1", &ctl, &progress));
        wait_until("some bytes downloaded", Duration::from_secs(10), || {
            progress.done() > 20_000
        });
        ctl.request(Stop::Cancel);
        h.join().unwrap()
    });
    assert_eq!(out, JobOutcome::Cancelled);
    assert!(f.part_files().is_empty());
    assert!(!f.store.partial_dir(&entry).exists());
    assert!(f.store.ready(&entry).is_none());
    assert_eq!(
        f.store.install_state(&entry),
        StoreInstallState::NotInstalled
    );
}

#[test]
fn pause_takes_effect_quickly() {
    let f = fixture(vec![ModelSpec::single("m1", 400_000)], 64 * 1024);
    f.server.set_chunk_delay(Duration::from_millis(10));
    let ctl = JobControl::new();
    let progress = JobProgress::new();
    let elapsed = std::thread::scope(|s| {
        let h = s.spawn(|| f.run_with("m1", &ctl, &progress));
        wait_until("started", Duration::from_secs(10), || {
            progress.done() > 10_000
        });
        let t = std::time::Instant::now();
        ctl.request(Stop::Pause);
        let out = h.join().unwrap();
        assert_eq!(out, JobOutcome::Paused);
        t.elapsed()
    });
    assert!(
        elapsed < Duration::from_millis(250),
        "stopped after {elapsed:?}"
    );
}

#[test]
fn disk_space_precheck_fails_before_any_request() {
    let f = fixture_with(vec![ModelSpec::single("m1", 100_000)], 1 << 20, |s| {
        s.free_space = Some(Arc::new(|_: &Path| Ok(1_000)));
    });
    let entry = f.entry("m1");
    let needed = DownloadEngine::space_needed(100_000);
    assert_eq!(
        f.run("m1"),
        JobOutcome::Failed(FailReason::DiskFull {
            needed,
            available: 1_000
        })
    );
    assert!(
        f.server.requests().is_empty(),
        "no request, so no byte written"
    );
    assert!(f.part_files().is_empty());
    assert!(f.store.ready(&entry).is_none());
}

#[test]
fn disk_space_boundary_is_exact() {
    let needed = DownloadEngine::space_needed(100_000);
    let below = fixture_with(vec![ModelSpec::single("m1", 100_000)], 1 << 20, move |s| {
        s.free_space = Some(Arc::new(move |_: &Path| Ok(needed - 1)));
    });
    assert!(matches!(
        below.run("m1"),
        JobOutcome::Failed(FailReason::DiskFull { .. })
    ));
    let at = fixture_with(vec![ModelSpec::single("m1", 100_000)], 1 << 20, move |s| {
        s.free_space = Some(Arc::new(move |_: &Path| Ok(needed)));
    });
    assert_eq!(at.run("m1"), JobOutcome::Installed);
}

#[test]
fn multi_file_model_is_installed_only_when_all_files_verify() {
    let f = fixture(vec![ModelSpec::multi("mm", &[30_000, 20_000])], 1 << 20);
    let entry = f.entry("mm");
    let second = server_path(&entry, 1);
    f.server.always(&second, Behaviour::WrongBytes);
    let out = f.run("mm");
    assert_eq!(
        out,
        JobOutcome::Failed(FailReason::ChecksumMismatch {
            file: entry.files[1].name.clone()
        })
    );
    assert!(f.store.ready(&entry).is_none());
    assert!(
        !f.store.rev_dir(&entry).exists(),
        "no final directory for an incomplete model"
    );
    // Fix the server: the verified first file is not downloaded again.
    f.server.clear_rules(&second);
    let first_requests = f.server.requests_for(&server_path(&entry, 0)).len();
    assert_eq!(f.run("mm"), JobOutcome::Installed);
    assert_eq!(
        f.server.requests_for(&server_path(&entry, 0)).len(),
        first_requests
    );
    let ready = f.store.ready(&entry).unwrap();
    assert_eq!(
        fs::read(ready.file("extra1").unwrap()).unwrap(),
        f.data("mm", 1)
    );
}

#[test]
fn nothing_appears_at_the_final_path_before_verification() {
    let f = fixture(vec![ModelSpec::multi("mm", &[200_000, 200_000])], 64 * 1024);
    let entry = f.entry("mm");
    f.server.set_chunk_delay(Duration::from_millis(3));
    let done = AtomicBool::new(false);
    let violations = std::thread::scope(|s| {
        let watcher = s.spawn(|| {
            let mut bad = 0;
            while !done.load(Ordering::SeqCst) {
                let rev = f.store.rev_dir(&entry);
                if rev.exists() && !rev.join("receipt.json").exists() {
                    bad += 1;
                }
                if entry.files.iter().any(|fl| rev.join(&fl.name).exists())
                    && !rev.join("receipt.json").exists()
                {
                    bad += 1;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
            bad
        });
        assert_eq!(f.run("mm"), JobOutcome::Installed);
        done.store(true, Ordering::SeqCst);
        watcher.join().unwrap()
    });
    assert_eq!(violations, 0);
    assert!(f.store.ready(&entry).is_some());
}

#[test]
fn a_correct_file_already_in_place_is_adopted_and_a_wrong_one_is_not() {
    let f = fixture(vec![ModelSpec::multi("mm", &[30_000, 20_000])], 1 << 20);
    let entry = f.entry("mm");
    let rev = f.store.rev_dir(&entry);
    fs::create_dir_all(&rev).unwrap();
    // File 0: correct. File 1: right size, wrong content.
    fs::write(rev.join(&entry.files[0].name), f.data("mm", 0)).unwrap();
    fs::write(rev.join(&entry.files[1].name), vec![7u8; 20_000]).unwrap();
    assert_eq!(f.run("mm"), JobOutcome::Installed);
    assert!(
        f.server.requests_for(&server_path(&entry, 0)).is_empty(),
        "adopted file is not downloaded"
    );
    assert!(
        !f.server.requests_for(&server_path(&entry, 1)).is_empty(),
        "wrong file is downloaded"
    );
    let ready = f.store.ready(&entry).unwrap();
    assert_eq!(
        fs::read(ready.file("extra1").unwrap()).unwrap(),
        f.data("mm", 1)
    );
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

struct BlockPrefix(&'static str);
impl NetworkPolicy for BlockPrefix {
    fn check(&self, url: &Url) -> Result<(), Blocked> {
        if url.path().starts_with(self.0) {
            Err(Blocked {
                host: "x".into(),
                reason: "blocked".into(),
            })
        } else {
            Ok(())
        }
    }
}

#[test]
fn blocked_url_fails_as_offline_without_a_connection() {
    let f = fixture_with(vec![ModelSpec::single("m1", 10_000)], 1 << 20, |s| {
        s.policy = Arc::new(BlockAll)
    });
    assert_eq!(f.run("m1"), JobOutcome::Failed(FailReason::Offline));
    assert!(f.server.requests().is_empty(), "no connection attempt");
    assert!(f.part_files().is_empty());
}

#[test]
fn blocked_redirect_target_fails_as_offline() {
    let f = fixture_with(vec![ModelSpec::single("m1", 10_000)], 1 << 20, |s| {
        s.policy = Arc::new(BlockPrefix("/cdn"))
    });
    let entry = f.entry("m1");
    f.server.add_file("/cdn/x.bin", f.data("m1", 0).to_vec());
    f.server.always(
        &server_path(&entry, 0),
        Behaviour::Redirect(format!("{}/cdn/x.bin", f.server.base())),
    );
    assert_eq!(f.run("m1"), JobOutcome::Failed(FailReason::Offline));
    assert!(f.server.requests_for("/cdn/x.bin").is_empty());
}

#[test]
fn connection_refused_on_the_first_request_is_offline() {
    let f = fixture(vec![ModelSpec::single("m1", 10_000)], 1 << 20);
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    let entry = f.entry("m1");
    let endpoint = Endpoint::HuggingFace {
        base_url: format!("http://{closed}"),
    };
    let out = f.engine.run_job(
        &f.store,
        JobSpec {
            entry: &entry,
            catalogue_version: 1,
            endpoint: &endpoint,
        },
        &JobControl::new(),
        &JobProgress::new(),
    );
    assert_eq!(out, JobOutcome::Failed(FailReason::Offline));
    assert!(
        f.delays.lock().unwrap().is_empty(),
        "offline is not retried with backoff"
    );
}

#[test]
fn probe_reports_reachability() {
    let f = fixture(vec![ModelSpec::single("m1", 10_000)], 1 << 20);
    assert!(f.engine.probe(&format!("{}/", f.server.base())));
    let closed = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap()
    };
    assert!(!f.engine.probe(&format!("http://{closed}/")));
}

/// One optional test with the real network; run it by hand with
/// `cargo test -p prxvc-models -- --ignored live`.
#[test]
#[ignore = "downloads 1,241,056 bytes from huggingface.co"]
fn live_download_of_the_silero_vad_file() {
    let cat = Catalogue::embedded();
    let entry = cat
        .get(&prxvc_models::ModelId::new("silero-vad-6.2"))
        .unwrap()
        .clone();
    let dir = tempfile::tempdir().unwrap();
    let store = ModelStore::open(dir.path().to_path_buf()).unwrap();
    let engine = DownloadEngine::new(EngineSettings::default()).unwrap();
    let endpoint = Endpoint::default();
    let out = engine.run_job(
        &store,
        JobSpec {
            entry: &entry,
            catalogue_version: cat.catalogue_version,
            endpoint: &endpoint,
        },
        &JobControl::new(),
        &JobProgress::new(),
    );
    assert_eq!(out, JobOutcome::Installed);
    let ready = store.ready(&entry).unwrap();
    assert_eq!(
        fs::metadata(ready.file("vad").unwrap()).unwrap().len(),
        1_241_056
    );
}
