//! Atomic file writes and "keep the old file aside" helpers.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Write `bytes` to `path` so that a reader sees either the old content or the
/// new content, never a mixture: the bytes go to `<path>.tmp` in the same
/// directory, are flushed to disk, and the temporary file is renamed over
/// `path`. A stale temporary file from an interrupted write is overwritten.
pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent)?;
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = parent.join(tmp_name);

    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
        return result;
    }
    sync_dir(parent);
    Ok(())
}

/// Flush a directory entry change to disk (Unix; best effort elsewhere).
pub(crate) fn sync_dir(dir: &Path) {
    #[cfg(unix)]
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
    #[cfg(not(unix))]
    let _ = dir;
}

/// `<path>.<label>-<UTC timestamp>`, with a numeric suffix if that exists.
fn aside_name(path: &Path, label: &str) -> PathBuf {
    let stamp = utc_stamp(SystemTime::now());
    let base = {
        let mut n = path.file_name().unwrap_or_default().to_os_string();
        n.push(format!(".{label}-{stamp}"));
        path.with_file_name(n)
    };
    if !base.exists() {
        return base;
    }
    (1u32..)
        .map(|i| {
            let mut n = base.file_name().unwrap_or_default().to_os_string();
            n.push(format!("-{i}"));
            base.with_file_name(n)
        })
        .find(|p| !p.exists())
        .unwrap_or(base)
}

/// Rename `path` to `<path>.<label>-<timestamp>` and return the new name.
///
/// If the rename fails the file is copied instead, and only counts as set aside
/// when the copy is complete; the original is then removed on a best-effort
/// basis. If neither works the error is returned and the file is untouched.
pub(crate) fn set_aside(path: &Path, label: &str) -> io::Result<PathBuf> {
    let target = aside_name(path, label);
    if fs::rename(path, &target).is_ok() {
        return Ok(target);
    }
    fs::copy(path, &target)?;
    if fs::read(path)? != fs::read(&target)? {
        let _ = fs::remove_file(&target);
        return Err(io::Error::other(
            "copy of the file does not match the original",
        ));
    }
    fs::remove_file(path)?;
    Ok(target)
}

/// Copy `path` to `<path>.<label>-<timestamp>`, unless an earlier copy with the
/// same label and identical content already exists. Returns the copy's name.
pub(crate) fn copy_aside(path: &Path, label: &str) -> io::Result<PathBuf> {
    let original = fs::read(path)?;
    if let (Some(dir), Some(name)) = (path.parent(), path.file_name()) {
        let prefix = format!("{}.{label}-", name.to_string_lossy());
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                let matches = p
                    .file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with(&prefix));
                if matches && fs::read(&p).is_ok_and(|b| b == original) {
                    return Ok(p);
                }
            }
        }
    }
    let target = aside_name(path, label);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)?;
    file.write_all(&original)?;
    file.sync_all()?;
    Ok(target)
}

/// `YYYYMMDDTHHMMSSZ` in UTC.
fn utc_stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}{m:02}{d:02}T{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::TempDir;
    use std::time::Duration;

    #[test]
    fn stamp_is_utc_civil_time() {
        let t = SystemTime::UNIX_EPOCH + Duration::from_secs(1_795_000_000);
        assert_eq!(utc_stamp(t), "20261118T110640Z");
        assert_eq!(utc_stamp(SystemTime::UNIX_EPOCH), "19700101T000000Z");
        let leap = SystemTime::UNIX_EPOCH + Duration::from_secs(951_782_400); // 2000-02-29
        assert_eq!(utc_stamp(leap), "20000229T000000Z");
    }

    #[test]
    fn write_replaces_content_and_leaves_no_temp_file() {
        let dir = TempDir::new("atomic_write");
        let p = dir.path().join("f.toml");
        write_atomic(&p, b"one").unwrap();
        write_atomic(&p, b"two").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"two");
        assert!(!dir.path().join("f.toml.tmp").exists());
    }

    #[test]
    fn a_stale_temp_file_is_overwritten() {
        let dir = TempDir::new("atomic_stale");
        let p = dir.path().join("f.toml");
        fs::write(&p, b"good").unwrap();
        fs::write(dir.path().join("f.toml.tmp"), b"half-wri").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"good");
        write_atomic(&p, b"new").unwrap();
        assert_eq!(fs::read(&p).unwrap(), b"new");
        assert!(!dir.path().join("f.toml.tmp").exists());
    }

    #[test]
    fn set_aside_picks_unused_names() {
        let dir = TempDir::new("atomic_aside");
        let p = dir.path().join("config.toml");
        fs::write(&p, b"a").unwrap();
        let first = set_aside(&p, "unreadable").unwrap();
        fs::write(&p, b"b").unwrap();
        let second = set_aside(&p, "unreadable").unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(first).unwrap(), b"a");
        assert_eq!(fs::read(second).unwrap(), b"b");
        assert!(!p.exists());
    }

    #[test]
    fn copy_aside_does_not_pile_up_identical_copies() {
        let dir = TempDir::new("atomic_copy_aside");
        let p = dir.path().join("config.toml");
        fs::write(&p, b"a").unwrap();
        let first = copy_aside(&p, "secrets").unwrap();
        let again = copy_aside(&p, "secrets").unwrap();
        assert_eq!(first, again);
        assert!(p.exists());
    }
}
