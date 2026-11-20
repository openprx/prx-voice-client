//! The routing journal: a small file recording that call mode switched the system default input, written atomically before the switch and removed after the restore.
//!
//! Location `<data_dir>/routing-journal.json` (pipeline-design PD-4.2.1); the
//! directory is a parameter because `prxvc-audio` does not depend on
//! `prxvc-config`. A journal that exists when the application starts means the
//! last run did not restore the default input.
//!
//! ```json
//! {"schema_version":1,"saved_default_input_uid":"BuiltInMicrophoneDevice",
//!  "saved_default_input_name":"MacBook Pro Microphone",
//!  "our_mic_uid":"org.openprx.prx-voice-client.microphone",
//!  "pid":4242,"started_at":1790000000}
//! ```
//!
//! `saved_default_input_uid` and `saved_default_input_name` are `null` when
//! our microphone was already the default before call mode (nothing to
//! remember; the restore then follows the input policy).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// File name inside the data directory.
pub const JOURNAL_FILE: &str = "routing-journal.json";

/// The journal format version this build writes and reads.
pub const SCHEMA_VERSION: u32 = 1;

/// What the journal records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Journal {
    /// Format version.
    pub schema_version: u32,
    /// UID of the default input before the switch; `None` when it was already ours.
    pub saved_default_input_uid: Option<String>,
    /// Name of that device, for messages.
    pub saved_default_input_name: Option<String>,
    /// UID of the virtual microphone that was made the default.
    pub our_mic_uid: String,
    /// Process that switched the default input.
    pub pid: u32,
    /// When it did, in seconds since the Unix epoch.
    pub started_at: u64,
}

/// What reading the journal found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JournalRead {
    /// No journal file.
    Absent,
    /// A well-formed journal.
    Valid(Journal),
    /// A file that cannot be used (unreadable, not JSON, other schema version).
    Corrupt {
        /// Why.
        reason: String,
    },
}

/// A journal operation failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("routing journal: {0}")]
pub struct JournalError(pub String);

/// Where the journal lives (a file in production, memory in tests).
pub trait JournalStore: Send {
    /// Read the journal.
    fn read(&self) -> JournalRead;

    /// Write the journal atomically: a reader sees the old file or the new one, never a part.
    ///
    /// # Errors
    /// [`JournalError`] when it could not be written.
    fn write(&self, journal: &Journal) -> Result<(), JournalError>;

    /// Delete the journal; deleting an absent one succeeds.
    ///
    /// # Errors
    /// [`JournalError`] when it could not be deleted.
    fn remove(&self) -> Result<(), JournalError>;

    /// Move an unusable journal aside, keeping its bytes, and return where it went.
    ///
    /// # Errors
    /// [`JournalError`] when it could not be moved.
    fn quarantine(&self, stamp: u64) -> Result<PathBuf, JournalError>;
}

/// The journal as a file.
#[derive(Debug, Clone)]
pub struct FileJournalStore {
    path: PathBuf,
}

impl FileJournalStore {
    /// The journal file inside `data_dir`.
    pub fn in_dir(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(JOURNAL_FILE),
        }
    }

    /// The journal file path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn io_error(what: &str, path: &Path, e: &std::io::Error) -> JournalError {
    JournalError(format!("{what} {}: {e}", path.display()))
}

impl JournalStore for FileJournalStore {
    fn read(&self) -> JournalRead {
        let bytes = match fs::read(&self.path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return JournalRead::Absent,
            Err(e) => {
                return JournalRead::Corrupt {
                    reason: format!("unreadable: {e}"),
                };
            }
        };
        match serde_json::from_slice::<Journal>(&bytes) {
            Ok(j) if j.schema_version == SCHEMA_VERSION => JournalRead::Valid(j),
            Ok(j) => JournalRead::Corrupt {
                reason: format!("unsupported schema_version {}", j.schema_version),
            },
            Err(e) => JournalRead::Corrupt {
                reason: format!("not a journal: {e}"),
            },
        }
    }

    fn write(&self, journal: &Journal) -> Result<(), JournalError> {
        let bytes = serde_json::to_vec_pretty(journal).map_err(|e| JournalError(e.to_string()))?;
        let tmp = self.path.with_extension("json.tmp");
        let write_tmp = || -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(&bytes)?;
            // The bytes are on disk before the rename publishes them.
            file.sync_all()
        };
        write_tmp().map_err(|e| io_error("write", &tmp, &e))?;
        if let Err(e) = fs::rename(&tmp, &self.path) {
            let _ = fs::remove_file(&tmp);
            return Err(io_error("rename to", &self.path, &e));
        }
        // Make the rename itself durable; a failure here does not undo it.
        if let Some(dir) = self.path.parent()
            && let Ok(d) = fs::File::open(dir)
        {
            let _ = d.sync_all();
        }
        Ok(())
    }

    fn remove(&self) -> Result<(), JournalError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(io_error("remove", &self.path, &e)),
        }
    }

    fn quarantine(&self, stamp: u64) -> Result<PathBuf, JournalError> {
        let aside = self
            .path
            .with_file_name(format!("routing-journal.corrupt-{stamp}.json"));
        fs::rename(&self.path, &aside).map_err(|e| io_error("move aside", &self.path, &e))?;
        Ok(aside)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn journal() -> Journal {
        Journal {
            schema_version: SCHEMA_VERSION,
            saved_default_input_uid: Some("BuiltIn".to_owned()),
            saved_default_input_name: Some("MacBook Pro Microphone".to_owned()),
            our_mic_uid: "org.openprx.prx-voice-client.microphone".to_owned(),
            pid: 4242,
            started_at: 1_790_000_000,
        }
    }

    #[test]
    fn write_read_remove_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileJournalStore::in_dir(dir.path());
        assert_eq!(store.read(), JournalRead::Absent);
        store.write(&journal()).unwrap();
        assert_eq!(store.read(), JournalRead::Valid(journal()));
        assert!(
            !dir.path().join("routing-journal.json.tmp").exists(),
            "no temp file is left"
        );
        store.remove().unwrap();
        assert_eq!(store.read(), JournalRead::Absent);
        store.remove().unwrap();
    }

    #[test]
    fn a_second_write_replaces_the_first_completely() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileJournalStore::in_dir(dir.path());
        store.write(&journal()).unwrap();
        let mut second = journal();
        second.saved_default_input_uid = None;
        second.saved_default_input_name = None;
        store.write(&second).unwrap();
        assert_eq!(store.read(), JournalRead::Valid(second));
    }

    #[test]
    fn garbage_and_foreign_schema_versions_are_corrupt() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileJournalStore::in_dir(dir.path());
        fs::write(store.path(), b"{ not json").unwrap();
        assert!(matches!(store.read(), JournalRead::Corrupt { .. }));
        let mut future = journal();
        future.schema_version = 99;
        fs::write(store.path(), serde_json::to_vec(&future).unwrap()).unwrap();
        assert!(matches!(store.read(), JournalRead::Corrupt { reason } if reason.contains("99")));
        fs::write(store.path(), b"").unwrap();
        assert!(matches!(store.read(), JournalRead::Corrupt { .. }));
    }

    #[test]
    fn quarantine_keeps_the_bytes_and_frees_the_name() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileJournalStore::in_dir(dir.path());
        fs::write(store.path(), b"garbage").unwrap();
        let aside = store.quarantine(123).unwrap();
        assert_eq!(fs::read(&aside).unwrap(), b"garbage");
        assert_eq!(store.read(), JournalRead::Absent);
        assert!(
            aside
                .file_name()
                .unwrap()
                .to_string_lossy()
                .contains("corrupt-123")
        );
    }

    #[test]
    fn the_documented_example_parses() {
        let text = r#"{"schema_version":1,"saved_default_input_uid":"BuiltInMicrophoneDevice",
 "saved_default_input_name":"MacBook Pro Microphone",
 "our_mic_uid":"org.openprx.prx-voice-client.microphone","pid":4242,"started_at":1790000000}"#;
        let j: Journal = serde_json::from_str(text).unwrap();
        assert_eq!(j.pid, 4242);
    }
}
