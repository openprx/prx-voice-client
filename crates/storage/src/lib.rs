use chrono::DateTime;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors from the storage subsystem.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("database error: {0}")]
    DatabaseError(String),
    #[error("migration error: {0}")]
    MigrationError(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("serialization error: {0}")]
    SerializationError(String),
}

impl From<rusqlite::Error> for StorageError {
    fn from(err: rusqlite::Error) -> Self {
        StorageError::DatabaseError(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// Records
// ---------------------------------------------------------------------------

/// A persisted session record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub state: String,
    pub created_at: String,
    pub updated_at: String,
    pub ended_at: Option<String>,
    pub turn_count: i64,
    pub config_json: String,
    pub transcript_preview: Option<String>,
}

impl SessionRecord {
    /// Create a [`SessionRecord`] from a live [`prxvc_session::Session`].
    ///
    /// Serialises the session config as JSON and copies timing / state fields.
    /// `transcript_preview` is left as `None`; callers may set it afterwards.
    pub fn from_session(session: &prxvc_session::Session) -> Result<Self, StorageError> {
        let config_json = serde_json::to_string(&session.config)
            .map_err(|e| StorageError::SerializationError(e.to_string()))?;

        Ok(Self {
            id: session.id.0.to_string(),
            state: format!("{}", session.state),
            created_at: session.created_at.to_rfc3339(),
            updated_at: session.updated_at.to_rfc3339(),
            ended_at: session.ended_at.map(|t| t.to_rfc3339()),
            turn_count: session.turn_count as i64,
            config_json,
            transcript_preview: None,
        })
    }

    /// Return a display-friendly title for this session.
    ///
    /// Uses `transcript_preview` when available, otherwise falls back to
    /// `"Voice Session at {created_at}"`.
    pub fn summary_title(&self) -> String {
        if let Some(ref preview) = self.transcript_preview {
            if !preview.is_empty() {
                return preview.clone();
            }
        }
        format!("Voice Session at {}", self.created_at)
    }

    /// Return a human-readable duration string (e.g. `"2m 35s"`).
    ///
    /// If `ended_at` is not set, returns `"\u{2014}"` (em-dash).
    pub fn duration_display(&self) -> String {
        let ended = match self.ended_at.as_deref() {
            Some(s) => s,
            None => return "\u{2014}".to_owned(),
        };

        let parse_start = DateTime::parse_from_rfc3339(&self.created_at);
        let parse_end = DateTime::parse_from_rfc3339(ended);

        match (parse_start, parse_end) {
            (Ok(start), Ok(end)) => {
                let secs = (end - start).num_seconds().max(0);
                let mins = secs / 60;
                let rem = secs % 60;
                if mins > 0 {
                    format!("{mins}m {rem}s")
                } else {
                    format!("{rem}s")
                }
            }
            _ => "\u{2014}".to_owned(),
        }
    }
}

// ---------------------------------------------------------------------------
// StorageManager
// ---------------------------------------------------------------------------

/// High-level storage manager for desktop-app integration.
///
/// Wraps [`Storage`] with convenience methods for saving / querying sessions.
pub struct StorageManager {
    storage: Storage,
}

impl StorageManager {
    /// Create a new [`StorageManager`] backed by the given database path.
    pub fn new(db_path: PathBuf) -> Result<Self, StorageError> {
        let storage = Storage::new(db_path)?;
        Ok(Self { storage })
    }

    /// Save a completed session, optionally attaching a transcript preview.
    pub fn save_session(
        &self,
        session: &prxvc_session::Session,
        transcript_preview: Option<String>,
    ) -> Result<(), StorageError> {
        let mut record = SessionRecord::from_session(session)?;
        record.transcript_preview = transcript_preview;
        self.storage.insert_session(&record)
    }

    /// Load the most recent session summaries for history display.
    pub fn recent_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>, StorageError> {
        self.storage.list_sessions(limit)
    }

    /// Delete a session by its id. Returns `true` if a row was removed.
    pub fn delete_session(&self, id: &str) -> Result<bool, StorageError> {
        self.storage.delete_session(id)
    }

    /// Remove all session history.
    pub fn clear_history(&self) -> Result<(), StorageError> {
        self.storage.clear_all()
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

/// Simple SQLite-backed local storage (V1 -- opens a new connection per call).
pub struct Storage {
    db_path: PathBuf,
}

impl Storage {
    /// Create a new [`Storage`] instance and run migrations.
    pub fn new(db_path: PathBuf) -> Result<Self, StorageError> {
        let storage = Self { db_path };
        storage.initialize()?;
        Ok(storage)
    }

    /// Open a connection to the database file.
    fn open_connection(&self) -> Result<Connection, StorageError> {
        Connection::open(&self.db_path).map_err(|e| StorageError::DatabaseError(e.to_string()))
    }

    /// Create required tables if they do not already exist.
    pub fn initialize(&self) -> Result<(), StorageError> {
        let conn = self.open_connection()?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS sessions (
                id              TEXT PRIMARY KEY,
                state           TEXT NOT NULL,
                created_at      TEXT NOT NULL,
                updated_at      TEXT NOT NULL,
                ended_at        TEXT,
                turn_count      INTEGER NOT NULL DEFAULT 0,
                config_json     TEXT NOT NULL DEFAULT '{}',
                transcript_preview TEXT
            );",
        )
        .map_err(|e| StorageError::MigrationError(e.to_string()))?;
        Ok(())
    }

    /// Insert a new session record.
    pub fn insert_session(&self, record: &SessionRecord) -> Result<(), StorageError> {
        let conn = self.open_connection()?;
        conn.execute(
            "INSERT INTO sessions (id, state, created_at, updated_at, ended_at, turn_count, config_json, transcript_preview)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.id,
                record.state,
                record.created_at,
                record.updated_at,
                record.ended_at,
                record.turn_count,
                record.config_json,
                record.transcript_preview,
            ],
        )?;
        Ok(())
    }

    /// Retrieve a session by its id, returning `None` if it does not exist.
    pub fn get_session(&self, id: &str) -> Result<Option<SessionRecord>, StorageError> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, state, created_at, updated_at, ended_at, turn_count, config_json, transcript_preview
             FROM sessions WHERE id = ?1",
        )?;

        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => Ok(Some(SessionRecord {
                id: row.get(0)?,
                state: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                ended_at: row.get(4)?,
                turn_count: row.get(5)?,
                config_json: row.get(6)?,
                transcript_preview: row.get(7)?,
            })),
            None => Ok(None),
        }
    }

    /// List the most recent sessions, up to `limit`.
    pub fn list_sessions(&self, limit: usize) -> Result<Vec<SessionRecord>, StorageError> {
        let conn = self.open_connection()?;
        let mut stmt = conn.prepare(
            "SELECT id, state, created_at, updated_at, ended_at, turn_count, config_json, transcript_preview
             FROM sessions ORDER BY created_at DESC LIMIT ?1",
        )?;

        let rows = stmt.query_map(params![limit as i64], |row| {
            Ok(SessionRecord {
                id: row.get(0)?,
                state: row.get(1)?,
                created_at: row.get(2)?,
                updated_at: row.get(3)?,
                ended_at: row.get(4)?,
                turn_count: row.get(5)?,
                config_json: row.get(6)?,
                transcript_preview: row.get(7)?,
            })
        })?;

        let mut results = Vec::new();
        for row in rows {
            results.push(row?);
        }
        Ok(results)
    }

    /// Delete a session by id. Returns `true` if a row was removed.
    pub fn delete_session(&self, id: &str) -> Result<bool, StorageError> {
        let conn = self.open_connection()?;
        let affected = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(affected > 0)
    }

    /// Remove all session records.
    pub fn clear_all(&self) -> Result<(), StorageError> {
        let conn = self.open_connection()?;
        conn.execute("DELETE FROM sessions", [])?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Prelude
// ---------------------------------------------------------------------------

pub mod prelude {
    pub use crate::{SessionRecord, Storage, StorageError, StorageManager};
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    /// Return a unique temp file path for a test SQLite database.
    fn temp_db_path(prefix: &str) -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock went backwards")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}_{ts}.db"))
    }

    /// Remove a temp db file (best-effort).
    fn cleanup(path: &PathBuf) {
        let _ = std::fs::remove_file(path);
    }

    /// Create a sample SessionRecord with a given id and created_at timestamp.
    fn make_session(id: &str, created_at: &str) -> SessionRecord {
        SessionRecord {
            id: id.to_owned(),
            state: "active".to_owned(),
            created_at: created_at.to_owned(),
            updated_at: created_at.to_owned(),
            ended_at: None,
            turn_count: 0,
            config_json: "{}".to_owned(),
            transcript_preview: None,
        }
    }

    #[test]
    fn test_storage_initialize() {
        let path = temp_db_path("prxvc_store_init");
        let result = Storage::new(path.clone());
        assert!(result.is_ok());
        cleanup(&path);
    }

    #[test]
    fn test_insert_and_get_session() {
        let path = temp_db_path("prxvc_store_insert_get");
        let storage = Storage::new(path.clone()).expect("init failed");

        let session = SessionRecord {
            id: "sess-001".to_owned(),
            state: "active".to_owned(),
            created_at: "2026-03-27T10:00:00Z".to_owned(),
            updated_at: "2026-03-27T10:00:01Z".to_owned(),
            ended_at: Some("2026-03-27T10:05:00Z".to_owned()),
            turn_count: 5,
            config_json: r#"{"key":"value"}"#.to_owned(),
            transcript_preview: Some("Hello world".to_owned()),
        };

        storage.insert_session(&session).expect("insert failed");
        let fetched = storage.get_session("sess-001").expect("get failed");

        assert!(fetched.is_some());
        let fetched = fetched.unwrap();
        assert_eq!(fetched.id, "sess-001");
        assert_eq!(fetched.state, "active");
        assert_eq!(fetched.turn_count, 5);
        assert_eq!(fetched.config_json, r#"{"key":"value"}"#);
        assert_eq!(fetched.transcript_preview.as_deref(), Some("Hello world"));
        assert_eq!(fetched.ended_at.as_deref(), Some("2026-03-27T10:05:00Z"));

        cleanup(&path);
    }

    #[test]
    fn test_get_nonexistent_session() {
        let path = temp_db_path("prxvc_store_noexist");
        let storage = Storage::new(path.clone()).expect("init failed");

        let result = storage.get_session("does-not-exist").expect("get failed");
        assert!(result.is_none());

        cleanup(&path);
    }

    #[test]
    fn test_list_sessions() {
        let path = temp_db_path("prxvc_store_list");
        let storage = Storage::new(path.clone()).expect("init failed");

        storage
            .insert_session(&make_session("s1", "2026-01-01T00:00:00Z"))
            .expect("insert");
        storage
            .insert_session(&make_session("s2", "2026-01-02T00:00:00Z"))
            .expect("insert");
        storage
            .insert_session(&make_session("s3", "2026-01-03T00:00:00Z"))
            .expect("insert");

        let sessions = storage.list_sessions(2).expect("list failed");
        assert_eq!(sessions.len(), 2);

        cleanup(&path);
    }

    #[test]
    fn test_list_sessions_order() {
        let path = temp_db_path("prxvc_store_order");
        let storage = Storage::new(path.clone()).expect("init failed");

        storage
            .insert_session(&make_session("old", "2026-01-01T00:00:00Z"))
            .expect("insert");
        storage
            .insert_session(&make_session("mid", "2026-01-02T00:00:00Z"))
            .expect("insert");
        storage
            .insert_session(&make_session("new", "2026-01-03T00:00:00Z"))
            .expect("insert");

        let sessions = storage.list_sessions(10).expect("list failed");
        assert_eq!(sessions.len(), 3);
        // Most recent first (DESC order)
        assert_eq!(sessions[0].id, "new");
        assert_eq!(sessions[1].id, "mid");
        assert_eq!(sessions[2].id, "old");

        cleanup(&path);
    }

    #[test]
    fn test_delete_session() {
        let path = temp_db_path("prxvc_store_delete");
        let storage = Storage::new(path.clone()).expect("init failed");

        storage
            .insert_session(&make_session("to-delete", "2026-01-01T00:00:00Z"))
            .expect("insert");
        let deleted = storage.delete_session("to-delete").expect("delete failed");
        assert!(deleted);

        let fetched = storage.get_session("to-delete").expect("get failed");
        assert!(fetched.is_none());

        cleanup(&path);
    }

    #[test]
    fn test_delete_nonexistent() {
        let path = temp_db_path("prxvc_store_del_noexist");
        let storage = Storage::new(path.clone()).expect("init failed");

        let deleted = storage.delete_session("ghost").expect("delete failed");
        assert!(!deleted);

        cleanup(&path);
    }

    #[test]
    fn test_clear_all() {
        let path = temp_db_path("prxvc_store_clear");
        let storage = Storage::new(path.clone()).expect("init failed");

        storage
            .insert_session(&make_session("a", "2026-01-01T00:00:00Z"))
            .expect("insert");
        storage
            .insert_session(&make_session("b", "2026-01-02T00:00:00Z"))
            .expect("insert");

        storage.clear_all().expect("clear failed");
        let sessions = storage.list_sessions(100).expect("list failed");
        assert!(sessions.is_empty());

        cleanup(&path);
    }

    #[test]
    fn test_session_record_from_session() {
        let config = prxvc_session::SessionConfig {
            input_language: "en".to_owned(),
            output_language: "zh".to_owned(),
            auto_play_reply: true,
            save_history: false,
            interrupt_mode: prxvc_session::InterruptMode::VoiceActivity,
            transcript_mode: prxvc_session::TranscriptMode::Full,
        };
        let session = prxvc_session::Session::new(config);
        let record = SessionRecord::from_session(&session).expect("from_session failed");

        assert_eq!(record.id, session.id.0.to_string());
        assert_eq!(record.state, "Idle");
        assert_eq!(record.turn_count, 0);
        assert!(record.ended_at.is_none());
        assert!(record.transcript_preview.is_none());
        // config_json should be valid JSON containing input_language
        assert!(record.config_json.contains("\"input_language\""));
    }

    #[test]
    fn test_session_record_summary_title() {
        let mut record = make_session("t1", "2026-03-27T10:00:00Z");

        // No transcript_preview => fallback
        assert!(record.summary_title().contains("Voice Session at"));

        // Empty transcript_preview => still fallback
        record.transcript_preview = Some(String::new());
        assert!(record.summary_title().contains("Voice Session at"));

        // With preview
        record.transcript_preview = Some("Hello, how are you?".to_owned());
        assert_eq!(record.summary_title(), "Hello, how are you?");
    }

    #[test]
    fn test_session_record_duration_display() {
        let mut record = make_session("d1", "2026-03-27T10:00:00+00:00");

        // No ended_at => em-dash
        assert_eq!(record.duration_display(), "\u{2014}");

        // 5 minutes later
        record.ended_at = Some("2026-03-27T10:05:00+00:00".to_owned());
        assert_eq!(record.duration_display(), "5m 0s");

        // 35 seconds
        record.ended_at = Some("2026-03-27T10:00:35+00:00".to_owned());
        assert_eq!(record.duration_display(), "35s");

        // 2m 35s
        record.ended_at = Some("2026-03-27T10:02:35+00:00".to_owned());
        assert_eq!(record.duration_display(), "2m 35s");
    }

    #[test]
    fn test_storage_manager_save_and_load() {
        let path = temp_db_path("prxvc_store_mgr_roundtrip");
        let mgr = StorageManager::new(path.clone()).expect("init failed");

        let config = prxvc_session::SessionConfig {
            input_language: "en".to_owned(),
            output_language: "en".to_owned(),
            auto_play_reply: true,
            save_history: false,
            interrupt_mode: prxvc_session::InterruptMode::Manual,
            transcript_mode: prxvc_session::TranscriptMode::Off,
        };
        let session = prxvc_session::Session::new(config);
        let session_id = session.id.0.to_string();

        mgr.save_session(&session, Some("test preview".to_owned()))
            .expect("save failed");

        let recent = mgr.recent_sessions(10).expect("list failed");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].id, session_id);
        assert_eq!(
            recent[0].transcript_preview.as_deref(),
            Some("test preview")
        );

        // Delete
        let deleted = mgr.delete_session(&session_id).expect("delete failed");
        assert!(deleted);
        assert!(mgr.recent_sessions(10).expect("list").is_empty());

        // Clear (idempotent)
        mgr.clear_history().expect("clear failed");

        cleanup(&path);
    }

    #[test]
    fn test_insert_duplicate_id() {
        let path = temp_db_path("prxvc_store_dup");
        let storage = Storage::new(path.clone()).expect("init failed");

        storage
            .insert_session(&make_session("dup", "2026-01-01T00:00:00Z"))
            .expect("insert");
        // SQLite PRIMARY KEY constraint should cause an error on duplicate id
        let result = storage.insert_session(&make_session("dup", "2026-01-02T00:00:00Z"));
        assert!(result.is_err());

        cleanup(&path);
    }
}
