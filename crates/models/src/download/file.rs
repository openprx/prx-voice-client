//! Download of one file: pieces, retry with backoff, redirect reuse and
//! disk-full mapping (design 4.2 rules 3, 5, 6, 7). Single responsibility:
//! get the bytes of one file into its partial.

use std::fs::OpenOptions;

use crate::catalogue::{FileSpec, ModelEntry};
use crate::control::{JobControl, JobProgress, Stop};
use crate::store::ModelStore;
use crate::types::FailReason;

use super::DownloadEngine;
use super::http::{FileWindow, PartWriter, PieceError};

pub(super) enum FileStop {
    Stop(Stop),
    Fail(FailReason),
}

const ENOSPC_UNIX: i32 = 28;
const ENOSPC_WINDOWS: i32 = 112;

fn is_disk_full(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(ENOSPC_UNIX) | Some(ENOSPC_WINDOWS))
        || e.kind() == std::io::ErrorKind::StorageFull
}

impl DownloadEngine {
    /// Fill the partial of `file` up to its full size. `base_done` is the
    /// number of bytes of the job already finished before this file.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn download_file(
        &self,
        store: &ModelStore,
        entry: &ModelEntry,
        file: &FileSpec,
        url: &str,
        base_done: u64,
        ctl: &JobControl,
        progress: &JobProgress,
        reached: &mut bool,
    ) -> Result<(), FileStop> {
        let have = store.prepare_partial(entry, file, url).map_err(|e| {
            FileStop::Fail(FailReason::Io {
                message: e.to_string(),
            })
        })?;
        let part = store.part_path(entry, file);
        let handle = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&part)
            .map_err(|e| {
                FileStop::Fail(FailReason::Io {
                    message: e.to_string(),
                })
            })?;
        let mut w = PartWriter { file: handle, have };
        progress.set_done(base_done + have);
        let on_have = |h: u64| progress.set_done(base_done + h);

        let mut current_url = url.to_string();
        let mut failures = 0u32;
        while w.have < file.size {
            if let Some(s) = ctl.signal() {
                return Err(FileStop::Stop(s));
            }
            match self.http.fetch_piece(
                &current_url,
                &mut w,
                FileWindow {
                    size: file.size,
                    piece: self.settings.piece_size,
                },
                ctl,
                &on_have,
                reached,
            ) {
                Ok(final_url) => {
                    failures = 0;
                    current_url = final_url;
                }
                Err(PieceError::Stop(s)) => return Err(FileStop::Stop(s)),
                Err(PieceError::Fatal(r)) => return Err(FileStop::Fail(r)),
                Err(PieceError::Io(e)) => {
                    if is_disk_full(&e) {
                        let free = store.free_space_fn();
                        let available = free(store.root()).unwrap_or(0);
                        return Err(FileStop::Fail(FailReason::DiskFull {
                            needed: Self::space_needed(file.size - w.have),
                            available,
                        }));
                    }
                    return Err(FileStop::Fail(FailReason::Io {
                        message: e.to_string(),
                    }));
                }
                Err(PieceError::Retry(message)) => {
                    failures += 1;
                    // The signed CDN URL may have expired: start again from
                    // the original URL.
                    current_url = url.to_string();
                    tracing::debug!(file = %file.name, failures, %message, "piece failed");
                    match self.settings.backoff.delay_after(failures) {
                        None => return Err(FileStop::Fail(FailReason::Network { message })),
                        Some(d) => (self.settings.sleeper)(d, ctl),
                    }
                }
            }
        }
        Ok(())
    }
}
