//! Adoption of model files from the legacy models folder (design 4.3).
//! Single responsibility: move recognised, hash-verified files into the store.

use std::fs;
use std::path::{Path, PathBuf};

use crate::catalogue::{Catalogue, FileSpec, ModelEntry};
use crate::types::StoreError;

use super::core::*;
use super::fsutil::*;
use super::types::*;

impl ModelStore {
    /// Adopt model files from the legacy folder (design 4.3). Idempotent. A
    /// file is adopted only when its name (or a legacy name) and size match a
    /// catalogue file and its SHA-256 equals that file's pin. Everything else
    /// is left where it is.
    pub fn migrate_legacy(
        &self,
        legacy_dir: &Path,
        catalogue: &Catalogue,
        progress: &dyn Fn(MigrationProgress),
    ) -> MigrationReport {
        let mut report = MigrationReport::default();
        if self.require_writable().is_err() {
            report.errors.push("the model store is read-only".into());
            return report;
        }
        if !legacy_dir.is_dir() {
            return report;
        }
        let files: Vec<PathBuf> = sorted_children(legacy_dir)
            .into_iter()
            .filter(|p| {
                fs::symlink_metadata(p)
                    .map(|m| m.is_file())
                    .unwrap_or(false)
            })
            .collect();

        struct Cand<'a> {
            entry: &'a ModelEntry,
            spec: &'a FileSpec,
        }
        let candidates_for = |name: &str, size: u64| -> Vec<Cand<'_>> {
            let mut v = Vec::new();
            for entry in catalogue.models() {
                for spec in &entry.files {
                    let by_name = spec.name == name
                        || entry.legacy_file_names.iter().flatten().any(|n| n == name);
                    if by_name && spec.size == size {
                        v.push(Cand { entry, spec });
                    }
                }
            }
            v
        };
        let sizes: Vec<u64> = files
            .iter()
            .map(|p| fs::metadata(p).map(|m| m.len()).unwrap_or(0))
            .collect();
        let bytes_total: u64 = files
            .iter()
            .zip(&sizes)
            .filter(|(p, s)| !candidates_for(&file_name_str(p), **s).is_empty())
            .map(|(_, s)| *s)
            .sum();
        let mut bytes_done = 0u64;
        let mut touched: Vec<&ModelEntry> = Vec::new();

        for (i, (path, size)) in files.iter().zip(&sizes).enumerate() {
            let name = file_name_str(path);
            let cands = candidates_for(&name, *size);
            if cands.is_empty() {
                report.unrecognised.push(path.clone());
                continue;
            }
            let base = bytes_done;
            let hash = hash_file(path, None, &mut |d| {
                progress(MigrationProgress {
                    file: name.clone(),
                    files_done: i,
                    files_total: files.len(),
                    bytes_done: base + d,
                    bytes_total,
                })
            });
            bytes_done += size;
            let hash = match hash {
                Ok(h) => h,
                Err(e) => {
                    report.errors.push(format!("{name}: {e}"));
                    continue;
                }
            };
            let Some(c) = cands.iter().find(|c| c.spec.sha256 == hash) else {
                report.unrecognised.push(path.clone());
                continue;
            };
            if self.ready(c.entry).is_some() {
                report.duplicates.push(path.clone());
                continue;
            }
            let dir = self.rev_dir(c.entry);
            let target = dir.join(&c.spec.name);
            if target.exists() {
                report.duplicates.push(path.clone());
                continue;
            }
            if let Err(e) = fs::create_dir_all(&dir).and_then(|_| move_file(path, &target)) {
                report.errors.push(format!("{name}: {e}"));
                continue;
            }
            report.moved.push((name, c.entry.id.clone()));
            if !touched.iter().any(|t| t.id == c.entry.id) {
                touched.push(c.entry);
            }
        }

        for entry in touched {
            match self.finish_adopted(entry, catalogue.catalogue_version) {
                Ok(true) => report.installed.push(entry.id.clone()),
                Ok(false) => {}
                Err(e) => report.errors.push(format!("{}: {e}", entry.id)),
            }
        }

        if fs::read_dir(legacy_dir)
            .map(|mut r| r.next().is_none())
            .unwrap_or(false)
            && fs::remove_dir(legacy_dir).is_ok()
        {
            report.legacy_dir_removed = true;
            if let Some(parent) = legacy_dir.parent() {
                let _ = fs::remove_dir(parent); // only if empty
            }
        }
        report
    }

    /// If every file of the entry sits in its final directory with the right
    /// size, write the receipt. Files placed there were hash-verified by the
    /// caller.
    fn finish_adopted(
        &self,
        entry: &ModelEntry,
        catalogue_version: u64,
    ) -> Result<bool, StoreError> {
        let dir = self.rev_dir(entry);
        if dir.join(RECEIPT_NAME).exists() {
            return Ok(false);
        }
        let mut receipt_files = Vec::new();
        for spec in &entry.files {
            let p = dir.join(&spec.name);
            match fs::metadata(&p) {
                Ok(m) if m.is_file() && m.len() == spec.size => receipt_files.push(ReceiptFile {
                    name: spec.name.clone(),
                    size: spec.size,
                    sha256: spec.sha256.clone(),
                    mtime_ns: mtime_ns(&m),
                }),
                _ => return Ok(false),
            }
        }
        let receipt = Receipt {
            schema: RECEIPT_SCHEMA,
            model_id: entry.id.clone(),
            revision_key: entry.revision_key(),
            catalogue_version,
            files: receipt_files,
            installed_at: chrono::Utc::now().to_rfc3339(),
        };
        write_json_atomic(&dir.join(RECEIPT_NAME), &receipt)?;
        sync_dir(&dir);
        Ok(true)
    }
}
