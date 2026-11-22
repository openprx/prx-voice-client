//! One-time, idempotent migration of the legacy config, database and model
//! files into the directories of [`crate::paths::APP_ID`] (design 4.3).
//!
//! The functions take their source and destination directories as parameters;
//! only [`LegacyDirs::resolve`] looks at the real user directories.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use crate::paths::{create_private_dir_all, project_dirs};
use crate::{ConfigError, atomic};

/// File name of the session database inside the data directory.
pub const DATABASE_FILE_NAME: &str = "sessions.db";

/// Name of the note left beside a copied legacy file.
pub const MOVED_NOTE_NAME: &str = "MOVED.txt";

// ---------------------------------------------------------------------------

/// The directories used before `APP_ID` existed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyDirs {
    /// Config directory of `com.prx.prx-voice-client`.
    pub config_dir: PathBuf,
    /// Data directory of `com.prx.prx-voice-client` (holds `sessions.db`).
    pub data_dir: PathBuf,
    /// Models directory of `com.prx.voice-client`.
    pub models_dir: PathBuf,
}

impl LegacyDirs {
    /// Resolve from the real platform directories.
    pub fn resolve() -> Result<Self, ConfigError> {
        let app = project_dirs(("com", "prx", "prx-voice-client"))?;
        let models = project_dirs(("com", "prx", "voice-client"))?;
        Ok(Self {
            config_dir: app.config_dir().to_path_buf(),
            data_dir: app.data_dir().to_path_buf(),
            models_dir: models.data_dir().join("models"),
        })
    }
}

/// What a migration did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Destinations of files copied; the originals were left in place.
    pub copied: Vec<PathBuf>,
    /// Destinations of files moved; the originals are gone.
    pub moved: Vec<PathBuf>,
    /// Sources not migrated because the destination already has the file.
    pub kept_existing: Vec<PathBuf>,
    /// Sources that could not be migrated, with the reason. They are intact.
    pub failures: Vec<(PathBuf, String)>,
    /// Empty legacy directories that were removed.
    pub removed_dirs: Vec<PathBuf>,
}

impl MigrationReport {
    /// Nothing was copied, moved, removed or failed.
    pub fn is_noop(&self) -> bool {
        self.copied.is_empty()
            && self.moved.is_empty()
            && self.failures.is_empty()
            && self.removed_dirs.is_empty()
    }
}

/// Copy the legacy `config.toml` and session database into the new
/// directories (design 4.3).
///
/// Each file is copied, never moved, and only when the destination does not
/// have it; the copy is verified byte for byte before it appears under its
/// final name. A [`MOVED_NOTE_NAME`] naming the new location is left beside the
/// originals. A legacy directory equal to the new one (Linux) is skipped.
/// Idempotent; failures are reported and leave every source untouched.
pub fn migrate_legacy_config_and_database(
    legacy_config_dir: &Path,
    legacy_data_dir: &Path,
    config_dir: &Path,
    data_dir: &Path,
) -> MigrationReport {
    let mut report = MigrationReport::default();
    if !same_dir(legacy_config_dir, config_dir) {
        copy_group(
            legacy_config_dir,
            config_dir,
            &[crate::CONFIG_FILE_NAME],
            &mut report,
        );
    }
    if !same_dir(legacy_data_dir, data_dir) {
        copy_group(
            legacy_data_dir,
            data_dir,
            &[DATABASE_FILE_NAME, "sessions.db-wal", "sessions.db-shm"],
            &mut report,
        );
    }
    report
}

/// Copy `names` (the first is the main file; the others only travel with it)
/// from `from` to `to`.
fn copy_group(from: &Path, to: &Path, names: &[&str], report: &mut MigrationReport) {
    let main = names[0];
    let src_main = from.join(main);
    if !src_main.is_file() {
        return;
    }
    if to.join(main).exists() {
        report.kept_existing.push(src_main);
        return;
    }
    let mut copied_here = Vec::new();
    for name in names {
        let (src, dst) = (from.join(name), to.join(name));
        if !src.is_file() || dst.exists() {
            continue;
        }
        match copy_verified(&src, &dst) {
            Ok(()) => {
                copied_here.push((*name, dst.clone()));
                report.copied.push(dst);
            }
            Err(e) => {
                report.failures.push((src, e.to_string()));
                // Do not leave a database without its main file or a partial
                // group: stop at the first failure.
                return;
            }
        }
    }
    if let Err(e) = append_moved_note(from, to, &copied_here) {
        tracing::warn!(
            "could not write {MOVED_NOTE_NAME} in {}: {e}",
            from.display()
        );
    }
}

fn append_moved_note(
    legacy_dir: &Path,
    new_dir: &Path,
    copied: &[(&str, PathBuf)],
) -> io::Result<()> {
    if copied.is_empty() {
        return Ok(());
    }
    let mut note = OpenOptions::new()
        .create(true)
        .append(true)
        .open(legacy_dir.join(MOVED_NOTE_NAME))?;
    for (name, _) in copied {
        writeln!(
            note,
            "{name} was copied to {}. This copy is no longer used by the application.",
            new_dir.join(name).display()
        )?;
    }
    Ok(())
}

/// Move the legacy models folder content into `models_dir`.
///
/// Every regular file is moved (renamed; copied, verified and then removed when
/// the rename fails, e.g. across volumes) unless the destination already has a
/// file of that name, in which case both stay. A failure leaves that file's
/// source intact and the others are still tried. When the legacy folder ends up
/// empty it is removed, and its parent too if that is empty. Idempotent.
///
/// The files keep their names and are placed directly in `models_dir`; matching
/// them to catalogue entries is the model manager's job.
pub fn migrate_legacy_models(legacy_models_dir: &Path, models_dir: &Path) -> MigrationReport {
    migrate_models_impl(legacy_models_dir, models_dir, true)
}

fn migrate_models_impl(legacy: &Path, dest: &Path, allow_rename: bool) -> MigrationReport {
    let mut report = MigrationReport::default();
    if same_dir(legacy, dest) {
        return report;
    }
    let Ok(read) = fs::read_dir(legacy) else {
        return report;
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .collect();
    files.sort();
    for src in files {
        let Some(name) = src.file_name() else {
            continue;
        };
        let dst = dest.join(name);
        if dst.exists() {
            report.kept_existing.push(src);
            continue;
        }
        match transfer(&src, &dst, allow_rename) {
            Ok(()) => report.moved.push(dst),
            Err(e) => report.failures.push((src, e.to_string())),
        }
    }
    // Remove the legacy folder and its parent when empty (`remove_dir` fails
    // on a non-empty directory, which is what we want).
    if fs::remove_dir(legacy).is_ok() {
        report.removed_dirs.push(legacy.to_path_buf());
        if let Some(parent) = legacy.parent()
            && fs::remove_dir(parent).is_ok()
        {
            report.removed_dirs.push(parent.to_path_buf());
        }
    }
    report
}

/// Move `src` to the absent `dst`: rename, or copy-verify-remove.
fn transfer(src: &Path, dst: &Path, allow_rename: bool) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        create_private_dir_all(parent)?;
    }
    if allow_rename && fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    copy_verified(src, dst)?;
    // The copy is complete and verified: only now is the source removed. If
    // removing fails the file exists twice, which is harmless.
    fs::remove_file(src)
}

/// Copy `src` to `dst` through `<dst>.migrating`, verify the copy byte for
/// byte, and rename it into place. `src` is only read. If anything fails the
/// temporary file is removed and `dst` does not exist.
fn copy_verified(src: &Path, dst: &Path) -> io::Result<()> {
    if let Some(parent) = dst.parent() {
        create_private_dir_all(parent)?;
    }
    let mut tmp_name = dst.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".migrating");
    let tmp = dst.with_file_name(tmp_name);
    let result = (|| {
        let mut input = File::open(src)?;
        let mut output = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        io::copy(&mut input, &mut output)?;
        output.sync_all()?;
        drop(output);
        if !same_content(src, &tmp)? {
            return Err(io::Error::other(format!(
                "copy of {} does not match the original",
                src.display()
            )));
        }
        if dst.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination appeared",
            ));
        }
        fs::rename(&tmp, dst)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    } else if let Some(parent) = dst.parent() {
        atomic::sync_dir(parent);
    }
    result
}

fn same_content(a: &Path, b: &Path) -> io::Result<bool> {
    let (mut fa, mut fb) = (File::open(a)?, File::open(b)?);
    if fa.metadata()?.len() != fb.metadata()?.len() {
        return Ok(false);
    }
    let (mut ba, mut bb) = (vec![0u8; 64 * 1024], vec![0u8; 64 * 1024]);
    loop {
        let n = fa.read(&mut ba)?;
        if n == 0 {
            return Ok(true);
        }
        fb.read_exact(&mut bb[..n])?;
        if ba[..n] != bb[..n] {
            return Ok(false);
        }
    }
}

fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || match (a.canonicalize(), b.canonicalize()) {
            (Ok(x), Ok(y)) => x == y,
            _ => false,
        }
}

#[cfg(test)]
mod tests;
