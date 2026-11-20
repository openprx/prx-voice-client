use super::*;
use crate::test_util::TempDir;

struct Fixture {
    root: TempDir,
}

impl Fixture {
    fn new(label: &str) -> Self {
        Self { root: TempDir::new(label) }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.path().join(rel)
    }
    fn write(&self, rel: &str, content: &[u8]) -> PathBuf {
        let p = self.p(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, content).unwrap();
        p
    }
    /// Old layout: config + database in `old_app`, models in `old_models`.
    fn legacy(&self) -> (PathBuf, PathBuf) {
        self.write("old_app/config.toml", b"[ui]\nlanguage = \"zh-CN\"\n");
        self.write("old_app/sessions.db", b"SQLITE-main");
        self.write("old_app/sessions.db-wal", b"wal-bytes");
        self.write("old_models/models/ggml-small.bin", &[1u8; 5000]);
        self.write("old_models/models/ggml-silero-vad.bin", &[2u8; 300]);
        (self.p("old_app"), self.p("old_models/models"))
    }
    fn run_config_db(&self) -> MigrationReport {
        migrate_legacy_config_and_database(
            &self.p("old_app"),
            &self.p("old_app"),
            &self.p("new/config"),
            &self.p("new/data"),
        )
    }
}

#[test]
fn config_and_database_are_copied_with_a_note() {
    let f = Fixture::new("cfgdb");
    f.legacy();
    let report = f.run_config_db();
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(report.copied.len(), 3);
    assert_eq!(fs::read(f.p("new/config/config.toml")).unwrap(), b"[ui]\nlanguage = \"zh-CN\"\n");
    assert_eq!(fs::read(f.p("new/data/sessions.db")).unwrap(), b"SQLITE-main");
    assert_eq!(fs::read(f.p("new/data/sessions.db-wal")).unwrap(), b"wal-bytes");
    // Originals stay, with a note naming the new location.
    assert!(f.p("old_app/config.toml").is_file());
    assert!(f.p("old_app/sessions.db").is_file());
    let note = fs::read_to_string(f.p("old_app/MOVED.txt")).unwrap();
    assert!(note.contains(&f.p("new/config").join("config.toml").display().to_string()), "{note}");
    assert!(note.contains(&f.p("new/data").join("sessions.db").display().to_string()), "{note}");
    assert!(!f.p("new/config/config.toml.migrating").exists());
}

#[test]
fn config_and_database_rerun_is_a_noop_and_never_overwrites() {
    let f = Fixture::new("cfgdb_rerun");
    f.legacy();
    f.run_config_db();
    let note_before = fs::read(f.p("old_app/MOVED.txt")).unwrap();
    let again = f.run_config_db();
    assert!(again.is_noop(), "{again:?}");
    assert_eq!(fs::read(f.p("old_app/MOVED.txt")).unwrap(), note_before);

    // The user changes the new config; a rerun must not bring the old one back.
    fs::write(f.p("new/config/config.toml"), b"changed").unwrap();
    f.run_config_db();
    assert_eq!(fs::read(f.p("new/config/config.toml")).unwrap(), b"changed");
}

#[test]
fn existing_destination_config_is_kept() {
    let f = Fixture::new("cfgdb_exists");
    f.legacy();
    f.write("new/config/config.toml", b"already here");
    f.write("new/data/sessions.db", b"new db");
    let report = f.run_config_db();
    assert!(report.copied.is_empty());
    assert_eq!(report.kept_existing.len(), 2);
    assert_eq!(fs::read(f.p("new/config/config.toml")).unwrap(), b"already here");
    assert_eq!(fs::read(f.p("new/data/sessions.db")).unwrap(), b"new db");
    assert!(!f.p("new/data/sessions.db-wal").exists(), "a wal must not travel to someone else's database");
    assert!(!f.p("old_app/MOVED.txt").exists());
}

#[test]
fn same_legacy_and_new_directory_is_skipped() {
    let f = Fixture::new("cfgdb_same");
    f.write("app/config.toml", b"x");
    f.write("app/sessions.db", b"y");
    let r = migrate_legacy_config_and_database(&f.p("app"), &f.p("app"), &f.p("app"), &f.p("app"));
    assert!(r.is_noop());
    assert!(!f.p("app/MOVED.txt").exists());
}

#[test]
fn failed_copy_leaves_sources_and_no_partial_destination() {
    let f = Fixture::new("cfgdb_fail");
    f.legacy();
    // A directory where the temporary file must go makes the copy fail.
    fs::create_dir_all(f.p("new/config/config.toml.migrating")).unwrap();
    let report = f.run_config_db();
    assert_eq!(report.failures.len(), 1, "{report:?}");
    assert!(!f.p("new/config/config.toml").exists());
    assert_eq!(fs::read(f.p("old_app/config.toml")).unwrap(), b"[ui]\nlanguage = \"zh-CN\"\n");
    // The database group is independent and still copied.
    assert!(f.p("new/data/sessions.db").is_file());
    // After the obstacle is gone, a rerun completes the job.
    fs::remove_dir_all(f.p("new/config/config.toml.migrating")).unwrap();
    let again = f.run_config_db();
    assert!(again.failures.is_empty());
    assert!(f.p("new/config/config.toml").is_file());
}

#[test]
fn models_move_to_the_new_root_and_the_old_folders_go() {
    let f = Fixture::new("models");
    let (_, old_models) = f.legacy();
    let dest = f.p("new/data/models");
    let report = migrate_legacy_models(&old_models, &dest);
    assert!(report.failures.is_empty(), "{report:?}");
    assert_eq!(report.moved.len(), 2);
    assert_eq!(fs::read(dest.join("ggml-small.bin")).unwrap(), vec![1u8; 5000]);
    assert_eq!(fs::read(dest.join("ggml-silero-vad.bin")).unwrap(), vec![2u8; 300]);
    assert!(!old_models.exists());
    assert!(!f.p("old_models").exists(), "empty parent is removed too");
    assert_eq!(report.removed_dirs.len(), 2);
}

#[test]
fn models_rerun_is_a_noop() {
    let f = Fixture::new("models_rerun");
    let (_, old_models) = f.legacy();
    let dest = f.p("new/data/models");
    migrate_legacy_models(&old_models, &dest);
    let again = migrate_legacy_models(&old_models, &dest);
    assert!(again.is_noop(), "{again:?}");
    assert_eq!(fs::read(dest.join("ggml-small.bin")).unwrap().len(), 5000);
}

#[test]
fn models_existing_destination_file_is_kept_and_source_stays() {
    let f = Fixture::new("models_exists");
    let (_, old_models) = f.legacy();
    let dest = f.p("new/data/models");
    fs::create_dir_all(&dest).unwrap();
    fs::write(dest.join("ggml-small.bin"), b"newer").unwrap();
    let report = migrate_legacy_models(&old_models, &dest);
    assert_eq!(fs::read(dest.join("ggml-small.bin")).unwrap(), b"newer");
    assert_eq!(fs::read(old_models.join("ggml-small.bin")).unwrap(), vec![1u8; 5000]);
    assert_eq!(report.kept_existing, vec![old_models.join("ggml-small.bin")]);
    assert_eq!(report.moved.len(), 1);
    assert!(old_models.is_dir(), "a folder that still holds a file is not removed");
}

#[test]
fn models_unmatched_content_and_subfolders_are_left_alone() {
    let f = Fixture::new("models_other");
    let (_, old_models) = f.legacy();
    f.write("old_models/models/sub/inner.bin", b"inner");
    let report = migrate_legacy_models(&old_models, &f.p("new/models"));
    assert_eq!(report.moved.len(), 2);
    assert_eq!(fs::read(old_models.join("sub/inner.bin")).unwrap(), b"inner");
    assert!(report.removed_dirs.is_empty());
}

#[test]
fn models_copy_path_verifies_then_removes_and_failure_midway_keeps_sources() {
    let f = Fixture::new("models_copy");
    let (_, old_models) = f.legacy();
    let dest = f.p("new/models");
    fs::create_dir_all(&dest).unwrap();
    // The second file (sorted: small.bin after silero) cannot be copied.
    fs::create_dir_all(dest.join("ggml-small.bin.migrating")).unwrap();
    // `false`: the cross-volume path (copy, verify, remove) instead of rename.
    let report = migrate_models_impl(&old_models, &dest, false);
    assert_eq!(report.moved, vec![dest.join("ggml-silero-vad.bin")]);
    assert_eq!(report.failures.len(), 1, "{report:?}");
    assert!(!old_models.join("ggml-silero-vad.bin").exists(), "moved file's source is removed after the verified copy");
    assert_eq!(fs::read(old_models.join("ggml-small.bin")).unwrap(), vec![1u8; 5000]);
    assert!(!dest.join("ggml-small.bin").exists());
    assert!(old_models.is_dir());
    // Retry once the obstacle is gone.
    fs::remove_dir_all(dest.join("ggml-small.bin.migrating")).unwrap();
    let again = migrate_models_impl(&old_models, &dest, false);
    assert!(again.failures.is_empty());
    assert_eq!(fs::read(dest.join("ggml-small.bin")).unwrap(), vec![1u8; 5000]);
    assert!(!old_models.exists());
}

#[test]
fn missing_legacy_folders_are_a_noop() {
    let f = Fixture::new("missing");
    assert!(migrate_legacy_models(&f.p("nope"), &f.p("dest")).is_noop());
    assert!(migrate_legacy_config_and_database(&f.p("a"), &f.p("a"), &f.p("c"), &f.p("d")).is_noop());
    assert!(!f.p("dest").exists());
}

#[test]
fn legacy_dirs_use_the_old_names() {
    let l = LegacyDirs::resolve().unwrap();
    assert!(l.config_dir.to_string_lossy().contains("prx-voice-client"));
    assert!(l.models_dir.ends_with("models"));
    assert!(l.models_dir.to_string_lossy().contains("voice-client"));
    #[cfg(target_os = "macos")]
    {
        assert!(l.config_dir.ends_with("com.prx.prx-voice-client"));
        assert!(l.models_dir.ends_with("com.prx.voice-client/models"));
    }
}
