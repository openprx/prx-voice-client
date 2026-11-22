//! Helpers for tests: temporary directories and a fake credential store.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::credentials::{CredentialError, CredentialStore};

static COUNTER: AtomicU32 = AtomicU32::new(0);

/// A unique directory under the system temporary directory, removed on drop.
/// Tests never touch the real user directories.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new(label: &str) -> Self {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("prxvc_config_{label}_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("create temp dir");
        Self(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Records every secret it is given.
#[derive(Default)]
pub(crate) struct FakeStore {
    pub saved: Mutex<Vec<(String, String)>>,
}

impl CredentialStore for FakeStore {
    fn store(&self, id: &str, secret: &str) -> Result<(), CredentialError> {
        self.saved
            .lock()
            .unwrap()
            .push((id.to_owned(), secret.to_owned()));
        Ok(())
    }
}
