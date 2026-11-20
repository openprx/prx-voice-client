//! The per-model install procedure: adopt present files, check disk space,
//! download and verify each file, promote atomically (design 4.2 rules 1, 3,
//! 10, 11).

use std::path::PathBuf;

use crate::catalogue::ModelEntry;
use crate::control::{JobControl, JobProgress, Stop};
use crate::store::ModelStore;
use crate::types::FailReason;

use super::file::FileStop;
use super::verify::{VerifyError, verify_file};
use super::{DownloadEngine, JobOutcome, JobSpec};

/// How many times a file is downloaded when its hash does not match.
const HASH_ATTEMPTS: u32 = 2;

enum Halt {
    Stop(Stop),
    Fail(FailReason),
}

impl From<FileStop> for Halt {
    fn from(s: FileStop) -> Self {
        match s {
            FileStop::Stop(s) => Halt::Stop(s),
            FileStop::Fail(r) => Halt::Fail(r),
        }
    }
}

fn io_fail(e: impl std::fmt::Display) -> Halt {
    Halt::Fail(FailReason::Io {
        message: e.to_string(),
    })
}

impl DownloadEngine {
    /// Run one job to completion, pause, cancel or failure. Blocking.
    pub fn run_job(
        &self,
        store: &ModelStore,
        spec: JobSpec<'_>,
        ctl: &JobControl,
        progress: &JobProgress,
    ) -> JobOutcome {
        match self.run_inner(store, &spec, ctl, progress) {
            Ok(()) => JobOutcome::Installed,
            Err(Halt::Fail(r)) => JobOutcome::Failed(r),
            Err(Halt::Stop(Stop::Pause)) => JobOutcome::Paused,
            Err(Halt::Stop(Stop::Cancel)) => {
                store.delete_partials(spec.entry);
                JobOutcome::Cancelled
            }
        }
    }

    fn run_inner(
        &self,
        store: &ModelStore,
        spec: &JobSpec<'_>,
        ctl: &JobControl,
        progress: &JobProgress,
    ) -> Result<(), Halt> {
        let entry = spec.entry;
        progress.set_total(entry.total_size());
        if store.ready(entry).is_some() {
            progress.set_done(entry.total_size());
            return Ok(());
        }
        let urls: Vec<String> = entry
            .files
            .iter()
            .map(|f| spec.endpoint.url_for(&f.source))
            .collect();
        // Blocked URLs fail as Offline before any connection or write.
        for u in &urls {
            self.http.check_policy(u).map_err(Halt::Fail)?;
        }

        // Files already sitting in the final directory (adopted from the
        // legacy folder) count if they hash correctly; a wrong one is removed.
        let rev = store.rev_dir(entry);
        let mut sources: Vec<Option<PathBuf>> = vec![None; entry.files.len()];
        for (i, f) in entry.files.iter().enumerate() {
            let p = rev.join(&f.name);
            if !p.exists() {
                continue;
            }
            match verify_file(&p, f, ctl, progress) {
                Ok(true) => sources[i] = Some(p),
                Ok(false) => {
                    let _ = std::fs::remove_file(&p);
                }
                Err(VerifyError::Stop(s)) => return Err(Halt::Stop(s)),
                Err(VerifyError::Io(m)) => return Err(io_fail(m)),
            }
        }

        let mut reached = false;
        let mut base = 0u64;
        for (i, f) in entry.files.iter().enumerate() {
            if let Some(s) = ctl.signal() {
                return Err(Halt::Stop(s));
            }
            if sources[i].is_some() {
                base += f.size;
                progress.set_done(base);
                continue;
            }
            self.check_disk(store, entry, i, &sources, &urls)?;
            let mut verified = false;
            for attempt in 1..=HASH_ATTEMPTS {
                self.download_file(store, entry, f, &urls[i], base, ctl, progress, &mut reached)?;
                match verify_file(&store.part_path(entry, f), f, ctl, progress) {
                    Ok(true) => {
                        verified = true;
                        break;
                    }
                    Ok(false) => {
                        store.delete_partial_file(entry, f);
                        progress.set_done(base);
                        if attempt == HASH_ATTEMPTS {
                            return Err(Halt::Fail(FailReason::ChecksumMismatch {
                                file: f.name.clone(),
                            }));
                        }
                    }
                    Err(VerifyError::Stop(s)) => return Err(Halt::Stop(s)),
                    Err(VerifyError::Io(m)) => return Err(io_fail(m)),
                }
            }
            debug_assert!(verified);
            sources[i] = Some(store.part_path(entry, f));
            base += f.size;
            progress.set_done(base);
        }

        let parts: Vec<PathBuf> = sources
            .into_iter()
            .map(|s| s.expect("every file verified"))
            .collect();
        store
            .promote(entry, spec.catalogue_version, &parts)
            .map_err(io_fail)?;
        progress.set_done(entry.total_size());
        Ok(())
    }

    /// Disk pre-check before file `from` (and the files after it).
    fn check_disk(
        &self,
        store: &ModelStore,
        entry: &ModelEntry,
        from: usize,
        sources: &[Option<PathBuf>],
        urls: &[String],
    ) -> Result<(), Halt> {
        let mut remaining = 0u64;
        for (j, f) in entry.files.iter().enumerate().skip(from) {
            if sources[j].is_some() {
                continue;
            }
            let have = store.prepare_partial(entry, f, &urls[j]).map_err(io_fail)?;
            remaining += f.size - have;
        }
        if remaining == 0 {
            return Ok(());
        }
        let needed = Self::space_needed(remaining);
        let probe = self
            .settings
            .free_space
            .clone()
            .unwrap_or_else(|| store.free_space_fn());
        let available = probe(store.root()).map_err(io_fail)?;
        if available < needed {
            return Err(Halt::Fail(FailReason::DiskFull { needed, available }));
        }
        Ok(())
    }
}
