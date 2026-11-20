//! Whether the process runs from its macOS application bundle.
//!
//! macOS attributes microphone and audio-capture permission to the
//! application bundle. A bare binary started from a terminal inherits the
//! terminal's permission state, and a Core Audio tap then delivers digital
//! silence without any error. The decision is a pure function of the
//! executable path and the bundle identifier so that it can be tested; the
//! rest of this module only gathers the two inputs.

use std::path::{Path, PathBuf};

/// Where the process runs, as far as permissions are concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BundleStatus {
    /// Inside `<Name>.app/Contents/MacOS/` with a non-empty bundle identifier.
    Bundled {
        /// `CFBundleIdentifier` of the bundle.
        identifier: String,
        /// The `<Name>.app` directory.
        app_dir: PathBuf,
    },
    /// A bare binary, or a bundle without an identifier.
    Unbundled,
}

impl BundleStatus {
    /// True when the permissions row "App bundle" must be shown.
    pub(crate) fn is_outside_bundle(&self) -> bool {
        matches!(self, Self::Unbundled)
    }
}

/// The `<Name>.app` directory when `exe` is `<Name>.app/Contents/MacOS/<exe>`.
pub(crate) fn app_dir_of(exe: &Path) -> Option<PathBuf> {
    let macos_dir = exe.parent()?;
    let contents_dir = macos_dir.parent()?;
    let app_dir = contents_dir.parent()?;
    let in_layout = macos_dir.file_name()? == "MacOS"
        && contents_dir.file_name()? == "Contents"
        && app_dir.extension()? == "app";
    in_layout.then(|| app_dir.to_path_buf())
}

/// Decide "bundled or not" from the executable path and the bundle
/// identifier read from `Contents/Info.plist` (`None` when unreadable).
pub(crate) fn classify(exe: &Path, identifier: Option<&str>) -> BundleStatus {
    let identifier = identifier.map(str::trim).filter(|id| !id.is_empty());
    match (app_dir_of(exe), identifier) {
        (Some(app_dir), Some(identifier)) => BundleStatus::Bundled {
            identifier: identifier.to_owned(),
            app_dir,
        },
        _ => BundleStatus::Unbundled,
    }
}

/// The value of `CFBundleIdentifier` in an XML property list.
///
/// The packaging script writes an XML plist, which is all this reads; a
/// binary plist yields `None`, which classifies as unbundled.
pub(crate) fn bundle_identifier_from_plist(xml: &str) -> Option<String> {
    let after_key = xml.split("<key>CFBundleIdentifier</key>").nth(1)?;
    let value = after_key
        .split("<string>")
        .nth(1)?
        .split("</string>")
        .next()?;
    Some(value.trim().to_owned())
}

/// The status of the running process, detected once.
pub(crate) fn current() -> &'static BundleStatus {
    static STATUS: std::sync::OnceLock<BundleStatus> = std::sync::OnceLock::new();
    STATUS.get_or_init(detect)
}

/// Inspect the running executable.
fn detect() -> BundleStatus {
    let Ok(exe) = std::env::current_exe() else {
        return BundleStatus::Unbundled;
    };
    let identifier = app_dir_of(&exe)
        .and_then(|app| std::fs::read_to_string(app.join("Contents/Info.plist")).ok())
        .and_then(|xml| bundle_identifier_from_plist(&xml));
    classify(&exe, identifier.as_deref())
}

/// The directory that holds bundled resources: `Contents/Resources` inside a
/// bundle, `None` otherwise (resources compiled into the binary need no path).
pub(crate) fn resources_dir(status: &BundleStatus) -> Option<PathBuf> {
    match status {
        BundleStatus::Bundled { app_dir, .. } => Some(app_dir.join("Contents/Resources")),
        BundleStatus::Unbundled => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXE: &str = "/Applications/PRX Voice.app/Contents/MacOS/prxvc-desktop";

    #[test]
    fn executable_in_bundle_layout_with_identifier_is_bundled() {
        let status = classify(Path::new(EXE), Some("org.openprx.prx-voice-client"));
        assert_eq!(
            status,
            BundleStatus::Bundled {
                identifier: "org.openprx.prx-voice-client".into(),
                app_dir: PathBuf::from("/Applications/PRX Voice.app"),
            }
        );
        assert!(!status.is_outside_bundle());
    }

    #[test]
    fn bare_binary_is_unbundled() {
        for exe in [
            "/repo/target/debug/prxvc-desktop",
            "/usr/local/bin/prxvc-desktop",
        ] {
            assert!(
                classify(Path::new(exe), Some("x.y.z")).is_outside_bundle(),
                "{exe}"
            );
        }
    }

    #[test]
    fn missing_or_blank_identifier_is_unbundled() {
        assert!(classify(Path::new(EXE), None).is_outside_bundle());
        assert!(classify(Path::new(EXE), Some("  ")).is_outside_bundle());
    }

    #[test]
    fn wrong_directory_names_are_unbundled() {
        for exe in [
            "/x/PRX Voice.app/Contents/Resources/prxvc-desktop",
            "/x/PRX Voice.app/MacOS/prxvc-desktop",
            "/x/PRX Voice/Contents/MacOS/prxvc-desktop",
            "/x/PRX Voice.app/Contents/MacOS/sub/prxvc-desktop",
            "prxvc-desktop",
        ] {
            assert!(
                classify(Path::new(exe), Some("x.y.z")).is_outside_bundle(),
                "{exe}"
            );
        }
    }

    #[test]
    fn identifier_is_read_from_an_xml_plist() {
        let xml = "<dict>\n  <key>CFBundleExecutable</key>\n  <string>prxvc-desktop</string>\n  \
                   <key>CFBundleIdentifier</key>\n  <string>org.openprx.prx-voice-client</string>\n</dict>";
        assert_eq!(
            bundle_identifier_from_plist(xml).as_deref(),
            Some("org.openprx.prx-voice-client")
        );
        assert_eq!(bundle_identifier_from_plist("<dict></dict>"), None);
        assert_eq!(bundle_identifier_from_plist("bplist00\u{1}"), None);
    }

    #[test]
    fn resources_dir_exists_only_in_a_bundle() {
        let bundled = classify(Path::new(EXE), Some("a.b.c"));
        assert_eq!(
            resources_dir(&bundled),
            Some(PathBuf::from(
                "/Applications/PRX Voice.app/Contents/Resources"
            ))
        );
        assert_eq!(resources_dir(&BundleStatus::Unbundled), None);
    }
}
