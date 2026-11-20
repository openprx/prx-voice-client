//! Persistence of the job list to `jobs.json` (design 4.2 rule 9). Single
//! responsibility: read and write that one file.

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::catalogue::ModelId;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct JobRecord {
    pub model_id: ModelId,
    /// Paused by the user: stays paused after a restart. Others resume.
    pub paused: bool,
}

pub(crate) fn load(path: &Path) -> Vec<JobRecord> {
    fs::read(path)
        .ok()
        .and_then(|d| serde_json::from_slice(&d).ok())
        .unwrap_or_default()
}

pub(crate) fn save(path: &Path, records: &[JobRecord]) {
    let tmp = path.with_extension("json.tmp");
    let Ok(data) = serde_json::to_vec_pretty(records) else {
        return;
    };
    if fs::write(&tmp, data).is_ok() && fs::rename(&tmp, path).is_err() {
        tracing::warn!(path = %path.display(), "could not persist the job list");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_missing_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("jobs.json");
        assert!(load(&p).is_empty());
        let recs = vec![JobRecord {
            model_id: ModelId::new("a"),
            paused: true,
        }];
        save(&p, &recs);
        assert_eq!(load(&p), recs);
    }
}
