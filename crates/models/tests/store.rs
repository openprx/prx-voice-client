//! Model store: readiness, promotion, removal, garbage collection, legacy
//! migration and store move. All on temporary directories.

mod common;

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, SystemTime};

use common::*;
use prxvc_models::catalogue::{Catalogue, ModelEntry, ModelId, Role};
use prxvc_models::store::{GcKind, MoveProgress, PromoteStep};
use prxvc_models::{ActiveSet, LangKey, ModelStore, StoreError, StoreInstallState};

fn open(dir: &Path) -> ModelStore {
    ModelStore::open_with(dir.to_path_buf(), Arc::new(|_: &Path| Ok(u64::MAX / 2))).unwrap()
}

/// Write the entry's files to scratch paths and promote them.
fn install(store: &ModelStore, cat: &Catalogue, spec: &ModelSpec) -> ModelEntry {
    let entry = entry_of(cat, &spec.id).clone();
    let scratch = store.root().join("scratch").join(&spec.id);
    fs::create_dir_all(&scratch).unwrap();
    let parts: Vec<PathBuf> = spec
        .files
        .iter()
        .map(|(n, d)| {
            let p = scratch.join(n);
            fs::write(&p, d).unwrap();
            p
        })
        .collect();
    store
        .promote(&entry, cat.catalogue_version, &parts)
        .unwrap();
    entry
}

fn touch_old(p: &Path, age: Duration) {
    let f = OpenOptions::new().write(true).open(p).unwrap();
    f.set_modified(SystemTime::now() - age).unwrap();
}

#[test]
fn promoted_model_is_ready_and_listed() {
    let dir = tempfile::tempdir().unwrap();
    let specs = vec![
        ModelSpec::single("aa", 1000),
        ModelSpec::multi("bb", &[500, 700]),
    ];
    let cat = catalogue_of(&specs);
    let store = open(dir.path());
    let a = install(&store, &cat, &specs[0]);
    let b = install(&store, &cat, &specs[1]);
    let ra = store.ready(&a).unwrap();
    assert_eq!(
        fs::read(ra.file("model").unwrap()).unwrap(),
        specs[0].files[0].1
    );
    let rb = store.ready(&b).unwrap();
    assert_eq!(rb.files.len(), 2);
    assert_eq!(rb.role, Role::Tts);
    assert_eq!(rb.files[1].purpose, "extra1", "catalogue order");
    let listed = store.installed_models();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.iter().map(|m| m.bytes).sum::<u64>(), 1000 + 1200);
}

#[test]
fn ready_is_none_when_a_file_changes_or_the_receipt_does_not_match_the_pin() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 1000);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path());
    let entry = install(&store, &cat, &spec);
    let file = store.rev_dir(&entry).join(&entry.files[0].name);

    // mtime changed
    touch_old(&file, Duration::from_secs(3600));
    assert!(store.ready(&entry).is_none(), "mtime change");
    assert_eq!(store.install_state(&entry), StoreInstallState::Corrupt);

    // reinstall, then size changed
    let store2 = open(&tempfile::tempdir().unwrap().keep());
    let entry2 = install(&store2, &cat, &spec);
    let file2 = store2.rev_dir(&entry2).join(&entry2.files[0].name);
    fs::write(&file2, b"short").unwrap();
    assert!(store2.ready(&entry2).is_none(), "size change");

    // pin changed: same id and revision, different content hash
    let other = ModelSpec {
        files: vec![(spec.files[0].0.clone(), blob(1000, 99))],
        ..ModelSpec::single("aa", 1000)
    };
    let cat_other = catalogue_of(std::slice::from_ref(&other));
    let store3 = open(&tempfile::tempdir().unwrap().keep());
    install(&store3, &cat, &spec);
    assert!(
        store3.ready(entry_of(&cat_other, "aa")).is_none(),
        "receipt does not match the catalogue pin"
    );
}

#[test]
fn install_state_transitions() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 1000);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path());
    let entry = entry_of(&cat, "aa").clone();
    assert_eq!(store.install_state(&entry), StoreInstallState::NotInstalled);
    fs::create_dir_all(store.partial_dir(&entry)).unwrap();
    fs::write(store.part_path(&entry, &entry.files[0]), vec![0u8; 300]).unwrap();
    assert_eq!(
        store.install_state(&entry),
        StoreInstallState::Partial { done: 300 }
    );
    let installed = install(&store, &cat, &spec);
    assert!(matches!(
        store.install_state(&installed),
        StoreInstallState::Installed { .. }
    ));
    assert!(
        !store.partial_dir(&installed).exists(),
        "partial cleaned by promotion"
    );
    store.remove(&ModelId::new("aa")).unwrap();
    assert_eq!(
        store.install_state(&installed),
        StoreInstallState::NotInstalled
    );
}

#[test]
fn a_crash_during_promotion_never_leaves_a_visible_model() {
    for step in [
        PromoteStep::Synced,
        PromoteStep::TmpCreated,
        PromoteStep::Renamed,
        PromoteStep::ReceiptWritten,
        PromoteStep::BeforeFinalRename,
    ] {
        let dir = tempfile::tempdir().unwrap();
        let spec = ModelSpec::multi("mx", &[400, 600]);
        let cat = catalogue_of(std::slice::from_ref(&spec));
        let entry = entry_of(&cat, "mx").clone();
        {
            let store = open(dir.path());
            let scratch = dir.path().join("scratch");
            fs::create_dir_all(&scratch).unwrap();
            let parts: Vec<PathBuf> = spec
                .files
                .iter()
                .map(|(n, d)| {
                    let p = scratch.join(n);
                    fs::write(&p, d).unwrap();
                    p
                })
                .collect();
            let res = store.promote_with_hook(&entry, 1, &parts, &|s| s != step);
            assert!(res.is_err(), "crash at {step:?}");
            assert!(store.ready(&entry).is_none(), "{step:?}");
            assert!(
                !matches!(
                    store.install_state(&entry),
                    StoreInstallState::Installed { .. }
                ),
                "{step:?}"
            );
            assert!(store.installed_models().is_empty());
        }
        // Restart: .tmp directories are removed, still nothing visible.
        let store = open(dir.path());
        let model_dir = store.model_dir(&entry.id);
        let leftovers = fs::read_dir(&model_dir)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
                    .count()
            })
            .unwrap_or(0);
        assert_eq!(leftovers, 0, "{step:?}");
        assert!(store.ready(&entry).is_none());
    }
}

#[test]
fn a_second_instance_opens_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let first = open(dir.path());
    assert!(!first.is_read_only());
    let second = ModelStore::open(dir.path().to_path_buf()).unwrap();
    assert!(second.is_read_only());
    assert!(matches!(
        second.remove(&ModelId::new("x")),
        Err(StoreError::ReadOnly)
    ));
    first.close();
    let third = ModelStore::open(dir.path().to_path_buf()).unwrap();
    assert!(!third.is_read_only(), "lock released by close");
}

#[test]
fn remove_frees_bytes_and_refuses_while_a_job_owns_the_model() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 1500);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path());
    let entry = install(&store, &cat, &spec);
    let id = ModelId::new("aa");
    assert!(store.claim(&id));
    assert!(matches!(store.remove(&id), Err(StoreError::Busy(_))));
    store.release(&id);
    assert!(store.ready(&entry).is_some());
    let freed = store.remove(&id).unwrap();
    assert!(freed >= 1500);
    assert!(store.ready(&entry).is_none());
    assert!(!store.model_dir(&id).exists());
}

#[test]
fn verify_full_detects_corruption_and_honours_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 5000);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path());
    let entry = install(&store, &cat, &spec);
    let seen = std::sync::atomic::AtomicU64::new(0);
    store
        .verify_full(&entry, &AtomicBool::new(false), &|n| {
            seen.store(n, std::sync::atomic::Ordering::SeqCst)
        })
        .unwrap();
    assert_eq!(seen.load(std::sync::atomic::Ordering::SeqCst), 5000);
    assert!(matches!(
        store.verify_full(&entry, &AtomicBool::new(true), &|_| {}),
        Err(StoreError::Cancelled)
    ));
    // Flip a byte but keep size and mtime.
    let p = store.rev_dir(&entry).join(&entry.files[0].name);
    let meta = fs::metadata(&p).unwrap();
    let mut data = fs::read(&p).unwrap();
    data[0] ^= 1;
    fs::write(&p, &data).unwrap();
    OpenOptions::new()
        .write(true)
        .open(&p)
        .unwrap()
        .set_modified(meta.modified().unwrap())
        .unwrap();
    assert!(matches!(
        store.verify_full(&entry, &AtomicBool::new(false), &|_| {}),
        Err(StoreError::ChecksumMismatch { .. })
    ));
}

// ---- garbage collection --------------------------------------------------

#[test]
fn gc_plan_never_contains_an_active_or_loaded_model() {
    let dir = tempfile::tempdir().unwrap();
    let specs: Vec<_> = ["act", "load", "idle"]
        .iter()
        .map(|id| ModelSpec::single(id, 800))
        .collect();
    let cat = catalogue_of(&specs);
    let store = open(dir.path());
    for s in &specs {
        install(&store, &cat, s);
    }
    let active = ActiveSet::new().with(Role::Asr, LangKey::Lang("en".into()), ModelId::new("act"));
    let loaded = [ModelId::new("load")];
    let plan = store.gc_plan(&cat, &active, &loaded);
    let ids: Vec<_> = plan
        .items
        .iter()
        .filter_map(|i| i.model_id.as_ref().map(|m| m.0.as_str()))
        .collect();
    assert_eq!(ids, vec!["idle"], "only the unused model is proposed");
    assert!(plan.items.iter().all(|i| i.kind == GcKind::UnusedModel));
    assert!(
        plan.automatic().items.is_empty(),
        "unused models are manual only"
    );
    let freed = store.gc_apply(&plan.unused_models()).unwrap();
    assert!(freed >= 800);
    assert!(store.ready(entry_of(&cat, "idle")).is_none());
    assert!(store.ready(entry_of(&cat, "act")).is_some());
    assert!(store.ready(entry_of(&cat, "load")).is_some());
}

#[test]
fn gc_collects_tmp_orphans_stale_partials_and_old_revisions() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 800);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path());
    let entry = install(&store, &cat, &spec);
    let active = ActiveSet::new().with(Role::Asr, LangKey::Lang("en".into()), ModelId::new("aa"));

    // A leftover .tmp directory (created after open, so open did not clean it).
    let tmp = store.model_dir(&entry.id).join("abcdef.tmp");
    fs::create_dir_all(&tmp).unwrap();
    fs::write(tmp.join("x"), b"junk").unwrap();
    // An old revision with an intact receipt: copy the current one under another key.
    let cur = store.rev_dir(&entry);
    let old = store.model_dir(&entry.id).join("000000000000");
    fs::create_dir_all(&old).unwrap();
    for e in fs::read_dir(&cur).unwrap().flatten() {
        fs::copy(e.path(), old.join(e.file_name())).unwrap();
    }
    // Copy changed mtimes: rewrite the old receipt so it still validates.
    let mut receipt: serde_json::Value =
        serde_json::from_slice(&fs::read(old.join("receipt.json")).unwrap()).unwrap();
    for f in receipt["files"].as_array_mut().unwrap() {
        let m = fs::metadata(old.join(f["name"].as_str().unwrap())).unwrap();
        f["mtime_ns"] = m
            .modified()
            .unwrap()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
            .to_string()
            .parse::<u64>()
            .unwrap()
            .into();
    }
    receipt["revision_key"] = "000000000000".into();
    fs::write(old.join("receipt.json"), receipt.to_string()).unwrap();
    // Orphan partial (sidecar matches nothing) and a stale matching partial.
    let other = ModelSpec::single("bb", 500);
    let cat2 = catalogue_of(&[ModelSpec::single("aa", 800), other]);
    let b = entry_of(&cat2, "bb").clone();
    fs::create_dir_all(store.partial_dir(&b)).unwrap();
    fs::write(store.part_path(&b, &b.files[0]), b"zzz").unwrap();
    fs::write(
        store
            .partial_dir(&b)
            .join(format!("{}.part.json", b.files[0].name)),
        serde_json::json!({"size": 1, "sha256": "x", "url": "u"}).to_string(),
    )
    .unwrap();

    let plan = store.gc_plan(&cat, &active, &[]);
    let kinds: Vec<GcKind> = plan.items.iter().map(|i| i.kind).collect();
    assert!(kinds.contains(&GcKind::TmpDir), "{kinds:?}");
    assert!(kinds.contains(&GcKind::OldRevision), "{kinds:?}");
    assert!(kinds.contains(&GcKind::OrphanPartial), "{kinds:?}");
    assert!(
        !kinds.contains(&GcKind::UnusedModel),
        "active model is kept"
    );
    let loaded_plan = store.gc_plan(&cat, &active, &[ModelId::new("aa")]);
    assert!(
        loaded_plan
            .items
            .iter()
            .all(|i| i.kind == GcKind::TmpDir || i.model_id.as_ref().is_none_or(|m| m.0 != "aa")),
        "no installed file of a loaded model is proposed (only its junk .tmp directory)"
    );

    store.gc_apply(&plan.automatic()).unwrap();
    assert!(!tmp.exists());
    assert!(!old.exists());
    assert!(!store.part_path(&b, &b.files[0]).exists());
    assert!(
        store.ready(&entry).is_some(),
        "the current revision survives"
    );

    // Stale partial: matches the catalogue but is older than 30 days.
    let a2 = entry.clone();
    fs::create_dir_all(store.partial_dir(&a2)).unwrap();
    let part = store.part_path(&a2, &a2.files[0]);
    fs::write(&part, b"abc").unwrap();
    fs::write(
        store
            .partial_dir(&a2)
            .join(format!("{}.part.json", a2.files[0].name)),
        serde_json::json!({"size": a2.files[0].size, "sha256": a2.files[0].sha256, "url": "u"})
            .to_string(),
    )
    .unwrap();
    assert!(
        store
            .gc_plan(&cat, &active, &[])
            .items
            .iter()
            .all(|i| i.kind != GcKind::StalePartial)
    );
    touch_old(&part, Duration::from_secs(31 * 24 * 3600));
    let plan = store.gc_plan(&cat, &active, &[]);
    assert!(plan.items.iter().any(|i| i.kind == GcKind::StalePartial));
    // A partial that belongs to a running job is never collected.
    store.claim(&a2.id);
    assert!(
        store
            .gc_plan(&cat, &active, &[])
            .items
            .iter()
            .all(|i| i.kind != GcKind::StalePartial)
    );
}

// ---- legacy migration -------------------------------------------------------

fn legacy_setup(specs: &[ModelSpec]) -> (tempfile::TempDir, PathBuf, PathBuf, Catalogue) {
    let root = tempfile::tempdir().unwrap();
    let legacy = root.path().join("old").join("models");
    fs::create_dir_all(&legacy).unwrap();
    let models = root.path().join("new");
    (root, legacy, models, catalogue_of(specs))
}

#[test]
fn migration_moves_a_matching_file_leaves_an_unknown_one_and_is_idempotent() {
    let specs = vec![ModelSpec::single("aa", 3000)];
    let (_root, legacy, models, cat) = legacy_setup(&specs);
    fs::write(legacy.join("aa.bin"), &specs[0].files[0].1).unwrap();
    fs::write(legacy.join("mystery.bin"), b"who knows").unwrap();
    // Right name and size, wrong content: must not be adopted.
    let store = open(&models);
    let progress = std::sync::Mutex::new(Vec::new());
    let report = store.migrate_legacy(&legacy, &cat, &|p| {
        progress.lock().unwrap().push(p.bytes_done)
    });
    assert_eq!(report.installed, vec![ModelId::new("aa")]);
    assert_eq!(report.moved.len(), 1);
    assert_eq!(report.unrecognised, vec![legacy.join("mystery.bin")]);
    assert!(!legacy.join("aa.bin").exists(), "moved, not copied");
    assert!(legacy.join("mystery.bin").exists(), "unknown file stays");
    assert!(!report.legacy_dir_removed);
    assert!(store.ready(entry_of(&cat, "aa")).is_some());
    assert!(!progress.lock().unwrap().is_empty());
    // Second run changes nothing.
    let again = store.migrate_legacy(&legacy, &cat, &|_| {});
    assert!(again.moved.is_empty() && again.installed.is_empty());
    assert!(store.ready(entry_of(&cat, "aa")).is_some());
}

#[test]
fn migration_does_not_adopt_a_file_with_the_right_size_but_the_wrong_hash() {
    let specs = vec![ModelSpec::single("aa", 3000)];
    let (_root, legacy, models, cat) = legacy_setup(&specs);
    fs::write(legacy.join("aa.bin"), vec![1u8; 3000]).unwrap();
    let store = open(&models);
    let report = store.migrate_legacy(&legacy, &cat, &|_| {});
    assert!(report.moved.is_empty());
    assert_eq!(report.unrecognised.len(), 1);
    assert!(legacy.join("aa.bin").exists());
    assert!(store.ready(entry_of(&cat, "aa")).is_none());
}

#[test]
fn migration_removes_an_emptied_legacy_folder_and_its_empty_parent() {
    let specs = vec![ModelSpec::single("aa", 3000)];
    let (root, legacy, models, cat) = legacy_setup(&specs);
    fs::write(legacy.join("aa.bin"), &specs[0].files[0].1).unwrap();
    let store = open(&models);
    let report = store.migrate_legacy(&legacy, &cat, &|_| {});
    assert!(report.legacy_dir_removed);
    assert!(!legacy.exists());
    assert!(
        !root.path().join("old").exists(),
        "empty parent removed too"
    );
}

#[test]
fn migration_honours_legacy_file_names_and_multi_file_models_complete_later() {
    // Entry with a legacy name for its first file.
    let spec = ModelSpec::multi("mm", &[1000, 2000]);
    let mut json: serde_json::Value =
        serde_json::to_value(catalogue_of(std::slice::from_ref(&spec))).unwrap();
    json["models"][0]["legacy_file_names"] = serde_json::json!(["old-name.bin"]);
    let cat = Catalogue::parse(&json.to_string()).unwrap();
    let (_root, legacy, models, _) = legacy_setup(std::slice::from_ref(&spec));
    let store = open(&models);
    fs::write(legacy.join("old-name.bin"), &spec.files[0].1).unwrap();
    let r1 = store.migrate_legacy(&legacy, &cat, &|_| {});
    assert_eq!(r1.moved.len(), 1);
    assert!(r1.installed.is_empty(), "second file still missing");
    assert!(store.ready(entry_of(&cat, "mm")).is_none());
    // The emptied legacy folder was removed after the first run.
    fs::create_dir_all(&legacy).unwrap();
    fs::write(legacy.join(&spec.files[1].0), &spec.files[1].1).unwrap();
    let r2 = store.migrate_legacy(&legacy, &cat, &|_| {});
    assert_eq!(r2.installed, vec![ModelId::new("mm")]);
    assert!(store.ready(entry_of(&cat, "mm")).is_some());
}

// ---- moving the store -----------------------------------------------------------

fn two_models() -> (Vec<ModelSpec>, Catalogue) {
    let specs = vec![
        ModelSpec::single("aa", 5000),
        ModelSpec::multi("bb", &[3000, 4000]),
    ];
    let cat = catalogue_of(&specs);
    (specs, cat)
}

#[test]
fn moving_the_store_keeps_every_model_ready() {
    let (specs, cat) = two_models();
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let store = open(src.path());
    let entries: Vec<_> = specs.iter().map(|s| install(&store, &cat, s)).collect();
    let phases = std::sync::Mutex::new(Vec::<MoveProgress>::new());
    let committed = std::sync::Mutex::new(None::<PathBuf>);
    let new_dir = dst.path().join("models");
    let moved = store
        .move_store(
            &new_dir,
            &AtomicBool::new(false),
            &|p| phases.lock().unwrap().push(p),
            &|d| {
                *committed.lock().unwrap() = Some(d.to_path_buf());
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        committed.lock().unwrap().as_deref(),
        Some(new_dir.as_path())
    );
    for e in &entries {
        let ready = moved.ready(e).expect("ready at the new location");
        assert!(ready.files.iter().all(|f| f.path.starts_with(&new_dir)));
    }
    assert!(
        entries.iter().all(|e| store.ready(e).is_none()),
        "old store deleted after commit"
    );
    let phases = phases.lock().unwrap();
    assert!(
        phases
            .iter()
            .any(|p| p.phase == prxvc_models::store::MovePhase::Copying)
    );
    assert!(
        phases
            .iter()
            .any(|p| p.phase == prxvc_models::store::MovePhase::Verifying)
    );
}

#[test]
fn a_failed_move_leaves_the_old_location_in_use_and_no_copy() {
    let (specs, cat) = two_models();
    let src = tempfile::tempdir().unwrap();
    let dst = tempfile::tempdir().unwrap();
    let store = open(src.path());
    let entries: Vec<_> = specs.iter().map(|s| install(&store, &cat, s)).collect();
    let new_dir = dst.path().join("models");

    // Failure of the commit step (writing models.dir).
    let err = store
        .move_store(&new_dir, &AtomicBool::new(false), &|_| {}, &|_| {
            Err("config is read-only".into())
        })
        .unwrap_err();
    assert!(matches!(err, StoreError::Other(_)));
    assert!(!new_dir.join("store").exists(), "copy removed");
    assert!(
        entries.iter().all(|e| store.ready(e).is_some()),
        "old location still ready"
    );

    // Cancel while copying.
    let err = store
        .move_store(&new_dir, &AtomicBool::new(true), &|_| {}, &|_| Ok(()))
        .unwrap_err();
    assert!(matches!(err, StoreError::Cancelled));
    assert!(!new_dir.join("store").exists());
    assert!(entries.iter().all(|e| store.ready(e).is_some()));

    // Not enough room on the destination.
    let store_small_disk = {
        let s = open(&tempfile::tempdir().unwrap().keep())
            .with_free_space(Arc::new(|_: &Path| Ok(10_000)));
        for sp in &specs {
            install(&s, &cat, sp);
        }
        s
    };
    let err = store_small_disk
        .move_store(&new_dir, &AtomicBool::new(false), &|_| {}, &|_| Ok(()))
        .unwrap_err();
    assert!(matches!(err, StoreError::DiskFull { .. }), "{err:?}");
    assert!(!new_dir.join("store").exists());

    // Destination already holds models.
    fs::create_dir_all(new_dir.join("store").join("x")).unwrap();
    let err = store
        .move_store(&new_dir, &AtomicBool::new(false), &|_| {}, &|_| Ok(()))
        .unwrap_err();
    assert!(matches!(err, StoreError::Other(_)));
    assert!(
        new_dir.join("store").join("x").exists(),
        "someone else's data is never deleted"
    );
}

#[test]
fn usage_reports_used_and_free_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let spec = ModelSpec::single("aa", 4000);
    let cat = catalogue_of(std::slice::from_ref(&spec));
    let store = open(dir.path()).with_free_space(Arc::new(|_: &Path| Ok(123_456)));
    install(&store, &cat, &spec);
    let u = store.usage();
    assert!(u.used_bytes >= 4000);
    assert_eq!(u.free_bytes, 123_456);
}
