//! Finding the `llama-server` binary (shared by the application's server manager and the readiness report).

use std::path::{Path, PathBuf};

/// The directories searched before `PATH`.
pub(crate) const DEFAULT_DIRS: [&str; 2] = ["/opt/homebrew/bin", "/usr/local/bin"];

/// The binary: the configured path when set (and then only that), else `llama-server` in the
/// default directories, then in `PATH`.
///
/// # Errors
/// Every place that was looked at.
pub(crate) fn find_binary(
    configured: &str,
    path_var: Option<&str>,
    is_executable: &dyn Fn(&Path) -> bool,
) -> Result<PathBuf, Vec<PathBuf>> {
    let configured = configured.trim();
    if !configured.is_empty() {
        let p = PathBuf::from(configured);
        return if is_executable(&p) {
            Ok(p)
        } else {
            Err(vec![p])
        };
    }
    let mut searched: Vec<PathBuf> = DEFAULT_DIRS
        .iter()
        .map(|d| Path::new(d).join("llama-server"))
        .collect();
    if let Some(path_var) = path_var {
        for dir in std::env::split_paths(path_var) {
            let candidate = dir.join("llama-server");
            if !searched.contains(&candidate) {
                searched.push(candidate);
            }
        }
    }
    searched
        .iter()
        .find(|p| is_executable(p))
        .cloned()
        .ok_or(searched)
}

/// Whether `path` is a regular file with an execute bit.
pub(crate) fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_configured_path_is_the_only_place_looked_at() {
        let exists = |p: &Path| p == Path::new("/opt/homebrew/bin/llama-server");
        let err = find_binary("/nope/llama-server", None, &exists).unwrap_err();
        assert_eq!(err, vec![PathBuf::from("/nope/llama-server")]);
        assert_eq!(
            find_binary("/opt/homebrew/bin/llama-server", None, &exists).unwrap(),
            PathBuf::from("/opt/homebrew/bin/llama-server")
        );
    }

    #[test]
    fn the_default_directories_come_before_path_and_every_place_is_reported() {
        let both = |p: &Path| p.ends_with("llama-server");
        assert_eq!(
            find_binary("", Some("/x/bin"), &both).unwrap(),
            PathBuf::from("/opt/homebrew/bin/llama-server")
        );
        let only_path = |p: &Path| p == Path::new("/x/bin/llama-server");
        assert_eq!(
            find_binary(" ", Some("/x/bin:/y/bin"), &only_path).unwrap(),
            PathBuf::from("/x/bin/llama-server")
        );
        let none = |_: &Path| false;
        let Err(searched) = find_binary("", Some("/x/bin:/opt/homebrew/bin"), &none) else {
            panic!("expected a missing binary");
        };
        assert_eq!(searched.len(), 3, "no place twice: {searched:?}");
        assert!(searched.contains(&PathBuf::from("/usr/local/bin/llama-server")));
    }
}
