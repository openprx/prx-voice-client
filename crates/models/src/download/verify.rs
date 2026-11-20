//! SHA-256 verification of a finished file with progress and cancellation
//! (design 4.2 rule 10).

use std::path::Path;

use crate::catalogue::FileSpec;
use crate::control::{JobControl, JobProgress, Stop};
use crate::store::hash_file;
use crate::types::StoreError;

pub(super) enum VerifyError {
    Stop(Stop),
    Io(String),
}

/// Ok(true) when the file hashes to the pin, Ok(false) on mismatch.
pub(super) fn verify_file(
    path: &Path,
    spec: &FileSpec,
    ctl: &JobControl,
    progress: &JobProgress,
) -> Result<bool, VerifyError> {
    progress.start_verify(spec.size);
    let stopped = || ctl.is_stopped();
    let res = hash_file(path, Some(&stopped), &mut |d| progress.set_verify_done(d));
    progress.end_verify();
    match res {
        Ok(sum) => Ok(sum == spec.sha256),
        Err(StoreError::Cancelled) => Err(VerifyError::Stop(ctl.signal().unwrap_or(Stop::Pause))),
        Err(e) => Err(VerifyError::Io(e.to_string())),
    }
}
