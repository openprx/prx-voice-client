//! Small filesystem helpers shared by the store modules: hashing, atomic JSON
//! writes, directory sizes and moves. Single responsibility: file-level utilities.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::types::StoreError;

pub(super) const HASH_BUFFER: usize = 1024 * 1024;

pub(super) fn mtime_ns(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

pub(super) fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
    s
}

/// Stream a file through SHA-256 (1 MiB buffer). `progress` receives the
/// cumulative byte count; `cancel` is polled between reads.
pub(crate) fn hash_file(
    path: &Path,
    cancel: Option<&dyn Fn() -> bool>,
    progress: &mut dyn FnMut(u64),
) -> Result<String, StoreError> {
    let mut f = File::open(path).map_err(|e| StoreError::io(path, e))?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; HASH_BUFFER];
    let mut done = 0u64;
    loop {
        if cancel.is_some_and(|c| c()) {
            return Err(StoreError::Cancelled);
        }
        let n = f.read(&mut buf).map_err(|e| StoreError::io(path, e))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        done += n as u64;
        progress(done);
    }
    Ok(to_hex(&hasher.finalize()))
}

pub(super) fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(f) = File::open(dir) {
        let _ = f.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

pub(super) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    let tmp = path.with_extension("json.tmp");
    let data = serde_json::to_vec_pretty(value).map_err(|e| StoreError::Other(e.to_string()))?;
    {
        let mut f = File::create(&tmp).map_err(|e| StoreError::io(&tmp, e))?;
        f.write_all(&data).map_err(|e| StoreError::io(&tmp, e))?;
        f.sync_all().map_err(|e| StoreError::io(&tmp, e))?;
    }
    fs::rename(&tmp, path).map_err(|e| StoreError::io(path, e))
}

pub(super) fn dir_size(path: &Path) -> u64 {
    let mut total = 0;
    if let Ok(rd) = fs::read_dir(path) {
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => total += dir_size(&e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

pub(super) fn remove_path(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Rename (or copy and delete across volumes).
pub(super) fn move_file(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    fs::copy(src, dst)?;
    File::open(dst)?.sync_all().ok();
    fs::remove_file(src)
}

pub(super) fn sorted_children(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    v.sort();
    v
}

pub(super) fn file_name_str(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}
