//! Moving the whole store to another directory with verification (design
//! 4.3 "Moving the store"). Single responsibility: copy, verify, commit, clean up.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::types::StoreError;

use super::core::*;
use super::fsutil::*;
use super::types::*;

/// Room required on a destination volume in addition to the data.
pub const MOVE_MARGIN_BYTES: u64 = 512 * 1024 * 1024;

impl ModelStore {
    /// Move the store to another models directory (design 4.3 "Moving the
    /// store"). Copies every installed revision, re-hashes the copies against
    /// their receipts, calls `commit` (the app writes `models.dir` there), and
    /// only then deletes the old `store/`. On any failure, including a failing
    /// `commit` or a cancel, the old location stays in use and the new copy is
    /// removed. Returns the store opened at the new location.
    pub fn move_store(
        &self,
        new_models_dir: &Path,
        cancel: &AtomicBool,
        progress: &dyn Fn(MoveProgress),
        commit: &dyn Fn(&Path) -> Result<(), String>,
    ) -> Result<ModelStore, StoreError> {
        self.require_writable()?;
        if let Some(id) = self.claims.lock().iter().next() {
            return Err(StoreError::Busy(id.clone()));
        }
        if new_models_dir == self.root {
            return Err(StoreError::Other(
                "destination is the current location".into(),
            ));
        }
        let new_store = new_models_dir.join("store");
        if fs::read_dir(&new_store)
            .map(|mut r| r.next().is_some())
            .unwrap_or(false)
        {
            return Err(StoreError::Other(
                "the destination already contains models".into(),
            ));
        }
        fs::create_dir_all(&new_store).map_err(|e| StoreError::io(&new_store, e))?;
        let probe = new_models_dir.join(".write-test");
        File::create(&probe)
            .and_then(|mut f| f.write_all(b"x"))
            .map_err(|e| StoreError::io(new_models_dir, e))?;
        let _ = fs::remove_file(&probe);

        let result = self.copy_and_verify(new_models_dir, cancel, progress);
        let result = result.and_then(|()| {
            commit(new_models_dir)
                .map_err(|m| StoreError::Other(format!("could not save the new location: {m}")))
        });
        if let Err(e) = result {
            let _ = remove_path(&new_store);
            return Err(e);
        }
        let opened =
            match ModelStore::open_with(new_models_dir.to_path_buf(), self.free_space.clone()) {
                Ok(s) => s,
                Err(e) => {
                    let _ = remove_path(&new_store);
                    return Err(e);
                }
            };
        // The new location is committed: remove the old copy.
        let _ = remove_path(&self.store_dir());
        let _ = fs::create_dir_all(self.store_dir());
        Ok(opened)
    }

    fn copy_and_verify(
        &self,
        new_models_dir: &Path,
        cancel: &AtomicBool,
        progress: &dyn Fn(MoveProgress),
    ) -> Result<(), StoreError> {
        let installed = self.installed_models();
        let total: u64 = installed.iter().map(|m| m.bytes).sum();
        let needed = total + MOVE_MARGIN_BYTES;
        let available =
            (self.free_space)(new_models_dir).map_err(|e| StoreError::io(new_models_dir, e))?;
        if available < needed {
            return Err(StoreError::DiskFull { needed, available });
        }
        let new_store = new_models_dir.join("store");
        let mut done = 0u64;
        let mut copied: Vec<(PathBuf, Receipt)> = Vec::new();
        for m in &installed {
            let src = self.model_dir(&m.id).join(&m.revision_key);
            let dst = new_store.join(&m.id.0).join(&m.revision_key);
            let receipt = Self::read_receipt(&src)
                .ok_or_else(|| StoreError::Other(format!("receipt of {} vanished", m.id)))?;
            fs::create_dir_all(&dst).map_err(|e| StoreError::io(&dst, e))?;
            for f in &receipt.files {
                copy_cancellable(&src.join(&f.name), &dst.join(&f.name), cancel, &mut |n| {
                    done += n;
                    progress(MoveProgress {
                        phase: MovePhase::Copying,
                        done,
                        total,
                    });
                })?;
            }
            copied.push((dst, receipt));
        }
        let mut vdone = 0u64;
        for (dst, receipt) in &mut copied {
            for f in &mut receipt.files {
                let base = vdone;
                let stopped = || cancel.load(Ordering::Relaxed);
                let sum = hash_file(&dst.join(&f.name), Some(&stopped), &mut |d| {
                    progress(MoveProgress {
                        phase: MovePhase::Verifying,
                        done: base + d,
                        total,
                    })
                })?;
                if sum != f.sha256 {
                    return Err(StoreError::ChecksumMismatch {
                        file: f.name.clone(),
                    });
                }
                vdone += f.size;
                let meta = fs::metadata(dst.join(&f.name))
                    .map_err(|e| StoreError::io(dst.join(&f.name), e))?;
                f.mtime_ns = mtime_ns(&meta);
            }
            write_json_atomic(&dst.join(RECEIPT_NAME), receipt)?;
        }
        Ok(())
    }
}
fn copy_cancellable(
    src: &Path,
    dst: &Path,
    cancel: &AtomicBool,
    on_bytes: &mut dyn FnMut(u64),
) -> Result<(), StoreError> {
    let mut r = File::open(src).map_err(|e| StoreError::io(src, e))?;
    let mut w = File::create(dst).map_err(|e| StoreError::io(dst, e))?;
    let mut buf = vec![0u8; HASH_BUFFER];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(StoreError::Cancelled);
        }
        let n = r.read(&mut buf).map_err(|e| StoreError::io(src, e))?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).map_err(|e| StoreError::io(dst, e))?;
        on_bytes(n as u64);
    }
    w.sync_all().map_err(|e| StoreError::io(dst, e))
}
