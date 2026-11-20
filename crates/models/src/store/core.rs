//! `ModelStore` core: layout, locking, receipts, readiness, removal, partial
//! files and atomic promotion. Single responsibility: the state of installed
//! and partially downloaded models on disk.

use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use fs4::FileExt;
use parking_lot::Mutex;

use crate::catalogue::{FileSpec, ModelEntry, ModelId};
use crate::types::StoreError;

use super::fsutil::*;
use super::types::*;
use super::{FreeSpaceFn, default_free_space};

pub(super) const RECEIPT_NAME: &str = "receipt.json";
pub(super) const RECEIPT_SCHEMA: u32 = 1;
pub(super) const TMP_SUFFIX: &str = ".tmp";
pub(super) const SET_ASIDE_SUFFIX: &str = ".set-aside";

// ---------------------------------------------------------------------------
// ModelStore
// ---------------------------------------------------------------------------

pub struct ModelStore {
    pub(super) root: PathBuf,
    pub(super) read_only: bool,
    lock: Mutex<Option<File>>,
    pub(super) claims: Mutex<HashSet<ModelId>>,
    pub(super) free_space: FreeSpaceFn,
}

impl std::fmt::Debug for ModelStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelStore")
            .field("root", &self.root)
            .field("read_only", &self.read_only)
            .finish()
    }
}

impl ModelStore {
    /// Creates the directories, takes `state.lock` and cleans `.tmp`
    /// directories. When another instance holds the lock the store opens
    /// read-only: it can be queried but not changed.
    pub fn open(models_dir: PathBuf) -> Result<ModelStore, StoreError> {
        Self::open_with(models_dir, default_free_space())
    }

    pub fn open_with(
        models_dir: PathBuf,
        free_space: FreeSpaceFn,
    ) -> Result<ModelStore, StoreError> {
        let (store_dir, partial_dir) = (models_dir.join("store"), models_dir.join("partial"));
        fs::create_dir_all(&store_dir).map_err(|e| StoreError::io(&store_dir, e))?;
        fs::create_dir_all(&partial_dir).map_err(|e| StoreError::io(&partial_dir, e))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&models_dir, fs::Permissions::from_mode(0o700));
        }
        let lock_path = models_dir.join("state.lock");
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .read(true)
            .open(&lock_path)
            .map_err(|e| StoreError::io(&lock_path, e))?;
        let read_only = FileExt::try_lock(&file).is_err();
        let store = ModelStore {
            root: models_dir,
            read_only,
            lock: Mutex::new(Some(file)),
            claims: Mutex::new(HashSet::new()),
            free_space,
        };
        if !read_only {
            store.clean_leftovers();
        }
        Ok(store)
    }

    /// Replace the free-space probe (tests).
    pub fn with_free_space(mut self, f: FreeSpaceFn) -> Self {
        self.free_space = f;
        self
    }

    /// Release `state.lock` (shutdown). The store stays readable.
    pub fn close(&self) {
        if let Some(f) = self.lock.lock().take() {
            let _ = FileExt::unlock(&f);
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn is_read_only(&self) -> bool {
        self.read_only
    }

    pub fn free_space_fn(&self) -> FreeSpaceFn {
        self.free_space.clone()
    }

    pub(super) fn require_writable(&self) -> Result<(), StoreError> {
        if self.read_only {
            Err(StoreError::ReadOnly)
        } else {
            Ok(())
        }
    }

    // ---- paths ----------------------------------------------------------

    pub fn store_dir(&self) -> PathBuf {
        self.root.join("store")
    }
    pub fn partial_root(&self) -> PathBuf {
        self.root.join("partial")
    }
    pub fn model_dir(&self, id: &ModelId) -> PathBuf {
        self.store_dir().join(&id.0)
    }
    /// Final directory of an entry's current pin.
    pub fn rev_dir(&self, entry: &ModelEntry) -> PathBuf {
        self.model_dir(&entry.id).join(entry.revision_key())
    }
    pub fn partial_dir(&self, entry: &ModelEntry) -> PathBuf {
        self.partial_root()
            .join(&entry.id.0)
            .join(entry.revision_key())
    }
    pub fn part_path(&self, entry: &ModelEntry, file: &FileSpec) -> PathBuf {
        self.partial_dir(entry).join(format!("{}.part", file.name))
    }
    pub(super) fn sidecar_path(&self, entry: &ModelEntry, file: &FileSpec) -> PathBuf {
        self.partial_dir(entry)
            .join(format!("{}.part.json", file.name))
    }

    // ---- claims (a job owns a model) --------------------------------------

    /// Mark a model as owned by a job. Returns false if already claimed.
    pub fn claim(&self, id: &ModelId) -> bool {
        self.claims.lock().insert(id.clone())
    }
    pub fn release(&self, id: &ModelId) {
        self.claims.lock().remove(id);
    }
    pub fn is_claimed(&self, id: &ModelId) -> bool {
        self.claims.lock().contains(id)
    }

    // ---- receipts and readiness ---------------------------------------------

    pub(super) fn read_receipt(dir: &Path) -> Option<Receipt> {
        let data = fs::read(dir.join(RECEIPT_NAME)).ok()?;
        let r: Receipt = serde_json::from_slice(&data).ok()?;
        (r.schema == RECEIPT_SCHEMA).then_some(r)
    }

    /// Receipt of a directory, if every file still has the recorded size and
    /// mtime. Does not compare with any catalogue entry.
    pub(super) fn intact_receipt(dir: &Path) -> Option<Receipt> {
        let r = Self::read_receipt(dir)?;
        if r.files.is_empty() {
            return None;
        }
        for f in &r.files {
            let meta = fs::metadata(dir.join(&f.name)).ok()?;
            if !meta.is_file() || meta.len() != f.size || mtime_ns(&meta) != f.mtime_ns {
                return None;
            }
        }
        Some(r)
    }

    pub(super) fn receipt_matches_entry(r: &Receipt, entry: &ModelEntry) -> bool {
        r.model_id == entry.id
            && r.revision_key == entry.revision_key()
            && r.files.len() == entry.files.len()
            && entry.files.iter().zip(&r.files).all(|(spec, rf)| {
                spec.name == rf.name && spec.size == rf.size && spec.sha256 == rf.sha256
            })
    }

    /// Receipt, size and mtime check; no hashing.
    pub fn ready(&self, entry: &ModelEntry) -> Option<ReadyModel> {
        let dir = self.rev_dir(entry);
        let receipt = Self::intact_receipt(&dir)?;
        if !Self::receipt_matches_entry(&receipt, entry) {
            return None;
        }
        Some(ReadyModel {
            id: entry.id.clone(),
            revision_key: entry.revision_key(),
            role: entry.role,
            runtime: entry.runtime.name.clone(),
            runtime_params: entry
                .runtime
                .params
                .clone()
                .unwrap_or_else(|| serde_json::Value::Object(Default::default())),
            files: entry
                .files
                .iter()
                .map(|f| ReadyFile {
                    purpose: f.purpose.clone(),
                    path: dir.join(&f.name),
                })
                .collect(),
        })
    }

    pub fn install_state(&self, entry: &ModelEntry) -> StoreInstallState {
        if self.ready(entry).is_some() {
            return StoreInstallState::Installed {
                revision_key: entry.revision_key(),
            };
        }
        let dir = self.rev_dir(entry);
        if dir.join(RECEIPT_NAME).exists() {
            return StoreInstallState::Corrupt;
        }
        let mut done = 0;
        if dir.is_dir() {
            done += entry
                .files
                .iter()
                .filter_map(|f| fs::metadata(dir.join(&f.name)).ok())
                .map(|m| m.len())
                .sum::<u64>();
        }
        done += entry
            .files
            .iter()
            .filter_map(|f| fs::metadata(self.part_path(entry, f)).ok())
            .map(|m| m.len())
            .sum::<u64>();
        if done > 0 {
            StoreInstallState::Partial { done }
        } else {
            StoreInstallState::NotInstalled
        }
    }

    /// Re-hash every file of an installed entry against the catalogue.
    /// `progress` receives the cumulative bytes hashed across all files.
    pub fn verify_full(
        &self,
        entry: &ModelEntry,
        cancel: &AtomicBool,
        progress: &dyn Fn(u64),
    ) -> Result<(), StoreError> {
        let dir = self.rev_dir(entry);
        let mut base = 0u64;
        for f in &entry.files {
            let stopped = || cancel.load(Ordering::Relaxed);
            let sum = hash_file(&dir.join(&f.name), Some(&stopped), &mut |d| {
                progress(base + d)
            })?;
            if sum != f.sha256 {
                return Err(StoreError::ChecksumMismatch {
                    file: f.name.clone(),
                });
            }
            base += f.size;
        }
        Ok(())
    }

    /// Every revision directory that holds an intact receipt, any model.
    pub fn installed_models(&self) -> Vec<InstalledModel> {
        let mut out = Vec::new();
        for model_dir in sorted_children(&self.store_dir()) {
            let id = ModelId(file_name_str(&model_dir));
            for rev in sorted_children(&model_dir) {
                let name = file_name_str(&rev);
                if name.ends_with(TMP_SUFFIX) || name.ends_with(SET_ASIDE_SUFFIX) {
                    continue;
                }
                if let Some(r) = Self::intact_receipt(&rev) {
                    out.push(InstalledModel {
                        id: id.clone(),
                        revision_key: name,
                        bytes: r.files.iter().map(|f| f.size).sum(),
                    });
                }
            }
        }
        out
    }

    // ---- removal ----------------------------------------------------------

    /// Delete every revision of a model. Returns bytes freed. Refuses while a
    /// job owns the model.
    pub fn remove(&self, id: &ModelId) -> Result<u64, StoreError> {
        self.require_writable()?;
        if self.is_claimed(id) {
            return Err(StoreError::Busy(id.clone()));
        }
        let dir = self.model_dir(id);
        let bytes = dir_size(&dir);
        self.remove_model_dir(&dir)?;
        Ok(bytes)
    }

    /// Receipts go first so the model stops counting as installed at once;
    /// files that cannot be deleted (still mapped on Windows) are renamed to
    /// `<name>.set-aside` and deleted at the next start.
    pub(super) fn remove_model_dir(&self, dir: &Path) -> Result<(), StoreError> {
        for rev in sorted_children(dir) {
            let _ = fs::remove_file(rev.join(RECEIPT_NAME));
        }
        match remove_path(dir) {
            Ok(()) => Ok(()),
            Err(first) => {
                let mut stuck = false;
                for rev in sorted_children(dir) {
                    for f in sorted_children(&rev) {
                        if remove_path(&f).is_err() {
                            let aside = PathBuf::from(format!("{}{SET_ASIDE_SUFFIX}", f.display()));
                            if fs::rename(&f, &aside).is_err() {
                                stuck = true;
                            }
                        }
                    }
                }
                if stuck {
                    Err(StoreError::io(dir, first))
                } else {
                    Ok(())
                }
            }
        }
    }

    pub(super) fn clean_leftovers(&self) {
        for model_dir in sorted_children(&self.store_dir()) {
            for rev in sorted_children(&model_dir) {
                let name = file_name_str(&rev);
                if name.ends_with(TMP_SUFFIX) {
                    let _ = remove_path(&rev);
                    continue;
                }
                for f in sorted_children(&rev) {
                    if file_name_str(&f).ends_with(SET_ASIDE_SUFFIX) {
                        let _ = remove_path(&f);
                    }
                }
                let _ = fs::remove_dir(&rev); // only succeeds when empty
            }
            let _ = fs::remove_dir(&model_dir);
        }
    }

    // ---- partial files (used by the download engine) ------------------------

    /// Validate the partial of one file against the catalogue and return how
    /// many bytes are already on disk. A sidecar whose size or hash differs
    /// from the pin invalidates the partial (it is deleted); so does a partial
    /// longer than the file. Writes the sidecar when missing.
    pub(crate) fn prepare_partial(
        &self,
        entry: &ModelEntry,
        file: &FileSpec,
        url: &str,
    ) -> Result<u64, StoreError> {
        self.require_writable()?;
        let dir = self.partial_dir(entry);
        fs::create_dir_all(&dir).map_err(|e| StoreError::io(&dir, e))?;
        let part = self.part_path(entry, file);
        let side = self.sidecar_path(entry, file);
        let current: Option<Sidecar> = fs::read(&side)
            .ok()
            .and_then(|d| serde_json::from_slice(&d).ok());
        let valid = current
            .as_ref()
            .is_some_and(|s| s.size == file.size && s.sha256 == file.sha256);
        if !valid {
            let _ = fs::remove_file(&part);
        }
        let mut have = fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
        if have > file.size {
            let _ = fs::remove_file(&part);
            have = 0;
        }
        let want = Sidecar {
            size: file.size,
            sha256: file.sha256.clone(),
            url: url.to_string(),
        };
        if current.as_ref() != Some(&want) {
            write_json_atomic(&side, &want)?;
        }
        Ok(have)
    }

    pub(crate) fn delete_partial_file(&self, entry: &ModelEntry, file: &FileSpec) {
        let _ = fs::remove_file(self.part_path(entry, file));
        let _ = fs::remove_file(self.sidecar_path(entry, file));
    }

    /// Delete every partial of an entry's current pin (cancel).
    pub fn delete_partials(&self, entry: &ModelEntry) {
        let dir = self.partial_dir(entry);
        let _ = remove_path(&dir);
        let _ = fs::remove_dir(self.partial_root().join(&entry.id.0));
    }

    /// Bytes of an entry's partial files (valid or not), for progress.
    pub fn partial_bytes(&self, entry: &ModelEntry) -> u64 {
        entry
            .files
            .iter()
            .filter_map(|f| fs::metadata(self.part_path(entry, f)).ok())
            .map(|m| m.len())
            .sum()
    }

    // ---- promotion ----------------------------------------------------------

    /// Atomically make a verified model visible. `files` are the verified
    /// source files (partials, or adopted files) in catalogue order.
    pub fn promote(
        &self,
        entry: &ModelEntry,
        catalogue_version: u64,
        files: &[PathBuf],
    ) -> Result<(), StoreError> {
        self.promote_with_hook(entry, catalogue_version, files, &|_| true)
    }

    /// As [`ModelStore::promote`]; `hook` is called after each step and may
    /// return false to simulate a crash at that point.
    #[doc(hidden)]
    pub fn promote_with_hook(
        &self,
        entry: &ModelEntry,
        catalogue_version: u64,
        files: &[PathBuf],
        hook: &dyn Fn(PromoteStep) -> bool,
    ) -> Result<(), StoreError> {
        self.require_writable()?;
        let crash = |step| {
            if hook(step) {
                Ok(())
            } else {
                Err(StoreError::Other(format!("simulated crash after {step:?}")))
            }
        };
        if files.len() != entry.files.len() {
            return Err(StoreError::Other(
                "promote needs one source per catalogue file".into(),
            ));
        }
        for p in files {
            let f = OpenOptions::new()
                .write(true)
                .open(p)
                .map_err(|e| StoreError::io(p, e))?;
            f.sync_all().map_err(|e| StoreError::io(p, e))?;
        }
        crash(PromoteStep::Synced)?;

        let model_dir = self.model_dir(&entry.id);
        let key = entry.revision_key();
        let tmp = model_dir.join(format!("{key}{TMP_SUFFIX}"));
        let fin = model_dir.join(&key);
        let _ = remove_path(&tmp);
        fs::create_dir_all(&tmp).map_err(|e| StoreError::io(&tmp, e))?;
        crash(PromoteStep::TmpCreated)?;

        let mut receipt_files = Vec::new();
        for (spec, src) in entry.files.iter().zip(files) {
            let dst = tmp.join(&spec.name);
            fs::rename(src, &dst).map_err(|e| StoreError::io(&dst, e))?;
            let meta = fs::metadata(&dst).map_err(|e| StoreError::io(&dst, e))?;
            receipt_files.push(ReceiptFile {
                name: spec.name.clone(),
                size: meta.len(),
                sha256: spec.sha256.clone(),
                mtime_ns: mtime_ns(&meta),
            });
        }
        crash(PromoteStep::Renamed)?;

        let receipt = Receipt {
            schema: RECEIPT_SCHEMA,
            model_id: entry.id.clone(),
            revision_key: key.clone(),
            catalogue_version,
            files: receipt_files,
            installed_at: chrono::Utc::now().to_rfc3339(),
        };
        write_json_atomic(&tmp.join(RECEIPT_NAME), &receipt)?;
        sync_dir(&tmp);
        crash(PromoteStep::ReceiptWritten)?;

        // A receipt-less directory here holds only adopted files that were
        // just moved into `tmp`; anything else is stale.
        let _ = remove_path(&fin);
        crash(PromoteStep::BeforeFinalRename)?;
        fs::rename(&tmp, &fin).map_err(|e| StoreError::io(&fin, e))?;
        sync_dir(&model_dir);

        let pdir = self.partial_dir(entry);
        let _ = remove_path(&pdir);
        let _ = fs::remove_dir(self.partial_root().join(&entry.id.0));
        Ok(())
    }
}

impl ModelStore {
    pub fn usage(&self) -> StorageUsage {
        StorageUsage {
            used_bytes: dir_size(&self.store_dir()) + dir_size(&self.partial_root()),
            free_bytes: (self.free_space)(&self.root).unwrap_or(0),
        }
    }
}
