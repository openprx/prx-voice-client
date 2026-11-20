//! The installed voice profile and the preset voices that can be chosen (read-only).
//!
//! Profile layout (voice-cloning 5.3): `<voices>/<id>/profile.json` with an optional `language`,
//! and prepared voices in `<voices>/<id>/derived/qwentts.cpp-<model fingerprint>/`.

use std::path::Path;

use prxvc_desktop_ui::settings_data::VoiceProfileView;

/// Prefix of the directories of prepared voices under `derived/`.
const DERIVED_PREFIX: &str = "qwentts.cpp-";

/// The profile `id` under `voices_dir`; `None` when none is configured or it is not on disk.
pub(crate) fn profile(voices_dir: &Path, id: &str) -> Option<VoiceProfileView> {
    let id = id.trim();
    let plain = !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !plain {
        return None;
    }
    let dir = voices_dir.join(id);
    if !dir.join("reference.wav").is_file() {
        return None;
    }
    let language = std::fs::read_to_string(dir.join("profile.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("language")?.as_str().map(str::to_owned))
        .filter(|l| !l.trim().is_empty());
    let mut prepared_for: Vec<String> = std::fs::read_dir(dir.join("derived"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter_map(|n| {
            n.strip_prefix(DERIVED_PREFIX)
                .map(|h| h.chars().take(12).collect())
        })
        .collect();
    prepared_for.sort();
    Some(VoiceProfileView {
        id: id.to_owned(),
        language,
        prepared_for,
    })
}

/// The ids of the preset voices: the catalogue's and the ones installed by hand under
/// `presets_dir` (a directory per preset), without duplicates, sorted.
pub(crate) fn presets(
    catalogue_ids: impl IntoIterator<Item = String>,
    presets_dir: &Path,
) -> Vec<String> {
    let mut ids: Vec<String> = catalogue_ids.into_iter().collect();
    ids.extend(
        std::fs::read_dir(presets_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok()),
    );
    ids.sort();
    ids.dedup();
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, text).expect("write");
    }

    #[test]
    fn a_profile_shows_its_language_and_the_models_it_was_prepared_for() {
        let dir = tempfile::tempdir().expect("tmp");
        write(&dir.path().join("me/reference.wav"), "x");
        write(
            &dir.path().join("me/profile.json"),
            r#"{"language":"zh","transcript":"hi"}"#,
        );
        write(
            &dir.path()
                .join("me/derived/qwentts.cpp-0123456789abcdef/meta.json"),
            "{}",
        );
        write(
            &dir.path().join("me/derived/qwentts.cpp-ffff/meta.json"),
            "{}",
        );
        write(
            &dir.path().join("me/derived/other-engine-1/meta.json"),
            "{}",
        );
        let p = profile(dir.path(), "me").expect("a profile");
        assert_eq!(p.id, "me");
        assert_eq!(p.language.as_deref(), Some("zh"));
        assert_eq!(p.prepared_for, ["0123456789ab", "ffff"]);
    }

    #[test]
    fn no_profile_is_shown_for_an_unset_unsafe_or_missing_id() {
        let dir = tempfile::tempdir().expect("tmp");
        write(&dir.path().join("me/reference.wav"), "x");
        assert!(profile(dir.path(), "").is_none());
        assert!(profile(dir.path(), "../me").is_none());
        assert!(profile(dir.path(), "nobody").is_none());
        // A profile without a readable json still exists.
        let p = profile(dir.path(), "me").expect("profile without json");
        assert_eq!(p.language, None);
        assert!(p.prepared_for.is_empty());
    }

    #[test]
    fn presets_join_the_catalogue_and_the_hand_installed_ones() {
        let dir = tempfile::tempdir().expect("tmp");
        write(&dir.path().join("calm-zh/reference.wav"), "x");
        write(&dir.path().join("warm-en/reference.wav"), "x");
        let ids = presets(["warm-en".to_owned(), "bright".to_owned()], dir.path());
        assert_eq!(ids, ["bright", "calm-zh", "warm-en"]);
        assert!(presets([], &dir.path().join("missing")).is_empty());
    }
}
