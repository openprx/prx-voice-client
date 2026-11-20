//! Log output: stderr in a terminal, a size-capped rotating file otherwise.
//!
//! An app started from Finder has no terminal, so without a file its log is
//! lost. The file lives in the application's log directory
//! (`AppPaths::logs_dir`). `RUST_LOG` selects the filter in both cases; the
//! default is `info`. Recognised and translated text is never logged above
//! `debug`, because this file persists on disk.

use std::fs::{self, File, OpenOptions};
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use tracing_subscriber::EnvFilter;

/// Name of the active log file.
pub(crate) const LOG_FILE_NAME: &str = "prx-voice.log";
/// Size at which the active file is rotated.
const MAX_FILE_BYTES: u64 = 5 * 1024 * 1024;
/// Rotated files kept next to the active one (`.1` is the newest).
const KEPT_BACKUPS: u32 = 3;

/// An append-only file that rotates when it would exceed a size cap.
#[derive(Debug)]
pub(crate) struct RotatingFile {
    path: PathBuf,
    max_bytes: u64,
    backups: u32,
    file: File,
    written: u64,
}

impl RotatingFile {
    /// Open (creating) `path` for appending.
    pub(crate) fn open(path: PathBuf, max_bytes: u64, backups: u32) -> io::Result<Self> {
        let file = open_append(&path)?;
        let written = file.metadata()?.len();
        Ok(Self {
            path,
            max_bytes,
            backups,
            file,
            written,
        })
    }

    fn backup_path(&self, n: u32) -> PathBuf {
        let mut name = self.path.clone().into_os_string();
        name.push(format!(".{n}"));
        PathBuf::from(name)
    }

    /// Shift `log` to `log.1`, `log.1` to `log.2`, and so on; the oldest goes.
    fn rotate(&mut self) -> io::Result<()> {
        if self.backups == 0 {
            self.file.set_len(0)?;
        } else {
            let _ = fs::remove_file(self.backup_path(self.backups));
            for n in (1..self.backups).rev() {
                let from = self.backup_path(n);
                if from.exists() {
                    fs::rename(&from, self.backup_path(n + 1))?;
                }
            }
            fs::rename(&self.path, self.backup_path(1))?;
            self.file = open_append(&self.path)?;
        }
        self.written = 0;
        Ok(())
    }
}

impl Write for RotatingFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.written > 0 && self.written + buf.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        let n = self.file.write(buf)?;
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Open for appending, private to the user (0600 on Unix).
fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path)
}

/// A cloneable writer over one shared [`RotatingFile`].
#[derive(Clone)]
struct SharedFile(Arc<Mutex<RotatingFile>>);

impl Write for SharedFile {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().flush()
    }
}

/// Where the log goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LogTarget {
    Stderr,
    File(PathBuf),
}

/// Pure choice: a terminal gets stderr; otherwise the file in `logs_dir`
/// when there is one.
pub(crate) fn choose_target(stderr_is_terminal: bool, logs_dir: Option<&Path>) -> LogTarget {
    match (stderr_is_terminal, logs_dir) {
        (false, Some(dir)) => LogTarget::File(dir.join(LOG_FILE_NAME)),
        _ => LogTarget::Stderr,
    }
}

fn env_filter() -> EnvFilter {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
}

/// Install the global subscriber. Returns the log file path when logging to
/// a file. A file that cannot be opened falls back to stderr (the failure is
/// printed there, never silent).
pub(crate) fn init(logs_dir: Option<&Path>) -> Option<PathBuf> {
    let target = choose_target(io::stderr().is_terminal(), logs_dir);
    if let LogTarget::File(path) = &target {
        match RotatingFile::open(path.clone(), MAX_FILE_BYTES, KEPT_BACKUPS) {
            Ok(file) => {
                let shared = SharedFile(Arc::new(Mutex::new(file)));
                tracing_subscriber::fmt()
                    .with_env_filter(env_filter())
                    .with_ansi(false)
                    .with_writer(move || shared.clone())
                    .init();
                return Some(path.clone());
            }
            Err(e) => eprintln!(
                "cannot open the log file {}: {e}; logging to stderr",
                path.display()
            ),
        }
    }
    tracing_subscriber::fmt()
        .with_env_filter(env_filter())
        .init();
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_logs_to_stderr_and_a_bundle_launch_to_the_file() {
        let dir = Path::new("/logs");
        assert_eq!(choose_target(true, Some(dir)), LogTarget::Stderr);
        assert_eq!(
            choose_target(false, Some(dir)),
            LogTarget::File(PathBuf::from("/logs").join(LOG_FILE_NAME))
        );
        assert_eq!(choose_target(false, None), LogTarget::Stderr);
    }

    #[test]
    fn rotation_caps_the_size_and_keeps_a_fixed_number_of_backups() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        let mut log = RotatingFile::open(path.clone(), 100, 2).unwrap();
        for i in 0..10 {
            log.write_all(format!("{i}{}\n", "x".repeat(39)).as_bytes())
                .unwrap();
        }
        log.flush().unwrap();
        let size = |p: &Path| fs::metadata(p).unwrap().len();
        assert!(size(&path) <= 100);
        assert!(path.with_extension("log.1").exists());
        assert!(path.with_extension("log.2").exists());
        assert!(!path.with_extension("log.3").exists());
        // The newest record is in the active file, the oldest is gone.
        let all: String = ["log", "log.1", "log.2"]
            .iter()
            .map(|e| fs::read_to_string(path.with_extension(e)).unwrap())
            .collect();
        assert!(fs::read_to_string(&path).unwrap().starts_with('8'));
        assert!(all.contains(&format!("9{}", "x".repeat(39))));
        assert!(!all.contains(&format!("0{}", "x".repeat(39))));
    }

    #[test]
    fn reopening_appends_and_counts_existing_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        RotatingFile::open(path.clone(), 100, 1)
            .unwrap()
            .write_all(b"first\n")
            .unwrap();
        let mut again = RotatingFile::open(path.clone(), 100, 1).unwrap();
        assert_eq!(again.written, 6);
        again.write_all(b"second\n").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "first\nsecond\n");
    }

    #[cfg(unix)]
    #[test]
    fn the_log_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("app.log");
        RotatingFile::open(path.clone(), 100, 1).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
