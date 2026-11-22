//! The voice of a synthesis spec: the owner's recorded profile, or a catalogue preset.
//!
//! Profile layout (voice-cloning 5.3): `<profiles>/<id>/profile.json`, `reference.wav` and
//! `derived/<engine>-<model fingerprint>/...`. A profile holds one reference recording, so the
//! owner's voice in two languages is two profiles: for the language `en` the profile
//! `<voice.self_profile_id>-en` is used when its directory exists, else the configured profile
//! itself (the speaker embedding carries across languages).
//!
//! A catalogue preset's reference recording sits at `<presets>/<preset id>/<file name from the
//! catalogue>`. The catalogue ships no presets yet, so a preset can also be installed by hand as
//! `<presets>/<preset id>/reference.wav` with an optional `preset.json` (`{"language": "zh"}`,
//! checked against the language asked for); it is referenced by its directory name in
//! `voice.incoming_preset`. A catalogue preset of the same id wins.

use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use prxvc_adapters::tts::qwentts::VoiceCache;
use prxvc_config::paths::AppPaths;
use prxvc_media::Lang;
use prxvc_models::ModelEntry;
use serde::Deserialize;

use super::Resolver;
use crate::problem::ProblemKind;
use crate::runtime::Runtime;
use crate::spec::VoicePlan;

/// Where voice data lives.
#[derive(Clone, Debug)]
pub struct VoiceDirs {
    /// Parent of the voice profile directories (`AppPaths::voices_dir`).
    pub profiles: PathBuf,
    /// Parent of the preset directories.
    pub presets: PathBuf,
}

impl VoiceDirs {
    /// The directories of the application: profiles in `voices_dir`, presets in
    /// `<data_dir>/voice-presets`.
    pub fn from_paths(paths: &AppPaths) -> Self {
        Self {
            profiles: paths.voices_dir.clone(),
            presets: paths.data_dir.join("voice-presets"),
        }
    }
}

/// Which voice a direction asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceChoice {
    /// The voice cloned from `voice.self_profile_id`.
    OwnClone,
    /// A catalogue preset; an empty name is the catalogue's default for the model and language.
    Preset(String),
}

#[derive(Deserialize)]
struct PresetFile {
    #[serde(default)]
    language: Option<String>,
}

#[derive(Deserialize)]
struct ProfileFile {
    #[serde(default)]
    language: Option<String>,
    #[serde(default)]
    transcript: Option<String>,
    #[serde(default)]
    x_vector_only: bool,
}

/// A profile id is a directory name: letters, digits, `-` and `_`.
fn plain_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Short form of a fingerprint for messages.
fn short(hash: &str) -> String {
    hash.chars().take(12).collect()
}

impl Resolver<'_> {
    pub(super) fn voice_plan(
        &self,
        entry: &ModelEntry,
        runtime: &Runtime,
        fingerprint: &str,
        lang: Lang,
        choice: &VoiceChoice,
    ) -> Result<VoicePlan, Vec<ProblemKind>> {
        match choice {
            VoiceChoice::OwnClone => self.cloned_voice(runtime, fingerprint, lang),
            VoiceChoice::Preset(name) => self.preset_voice(entry, lang, name),
        }
    }

    fn cloned_voice(
        &self,
        runtime: &Runtime,
        fingerprint: &str,
        lang: Lang,
    ) -> Result<VoicePlan, Vec<ProblemKind>> {
        let configured = self.cfg.voice.self_profile_id.trim().to_owned();
        if configured.is_empty() {
            return Err(vec![ProblemKind::VoiceProfileNotSet]);
        }
        let for_language = format!("{configured}-{}", lang.code());
        let profile_id =
            if plain_id(&for_language) && self.dirs.profiles.join(&for_language).is_dir() {
                for_language
            } else {
                configured
            };
        let dir = self.dirs.profiles.join(&profile_id);
        let missing = || {
            vec![ProblemKind::VoiceProfileMissing {
                profile_id: profile_id.clone(),
                dir: dir.clone(),
            }]
        };
        let unreadable = |detail: String| {
            vec![ProblemKind::VoiceProfileUnreadable {
                profile_id: profile_id.clone(),
                detail,
            }]
        };
        if !plain_id(&profile_id) || !dir.is_dir() {
            return Err(missing());
        }
        let reference = dir.join("reference.wav");
        if !reference.is_file() {
            return Err(missing());
        }
        let text = match fs::read_to_string(dir.join("profile.json")) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(missing()),
            Err(e) => return Err(unreadable(e.to_string())),
        };
        let profile: ProfileFile =
            serde_json::from_str(&text).map_err(|e| unreadable(format!("profile.json: {e}")))?;
        if *runtime == Runtime::QwenTts && self.require_prepared {
            self.check_prepared(&profile_id, &dir, fingerprint)?;
        }
        let transcript = if profile.x_vector_only {
            None
        } else {
            profile.transcript.filter(|t| !t.trim().is_empty())
        };
        Ok(VoicePlan::Cloned {
            profile_id,
            profile_dir: dir,
            reference,
            transcript,
            lang: profile
                .language
                .and_then(|l| Lang::from_str(&l).ok())
                .unwrap_or(lang),
        })
    }

    /// The profile must hold a voice prepared for this model's fingerprint.
    fn check_prepared(
        &self,
        profile_id: &str,
        dir: &Path,
        fingerprint: &str,
    ) -> Result<(), Vec<ProblemKind>> {
        let cache = VoiceCache::new(dir);
        let entry = cache.entry_dir(fingerprint).map_err(|e| {
            vec![ProblemKind::VoiceProfileUnreadable {
                profile_id: profile_id.to_owned(),
                detail: e.to_string(),
            }]
        })?;
        if entry.join("meta.json").is_file() && entry.join("spk.bin").is_file() {
            return Ok(());
        }
        let prefix = "qwentts.cpp-";
        let mut other: Vec<String> = fs::read_dir(dir.join("derived"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .filter_map(|n| n.strip_prefix(prefix).map(short))
            .collect();
        other.sort();
        Err(vec![if other.is_empty() {
            ProblemKind::VoiceNotPrepared {
                profile_id: profile_id.to_owned(),
            }
        } else {
            ProblemKind::VoicePreparedForOtherModel {
                profile_id: profile_id.to_owned(),
                prepared_for: other,
                current: short(fingerprint),
            }
        }])
    }

    fn preset_voice(
        &self,
        entry: &ModelEntry,
        lang: Lang,
        name: &str,
    ) -> Result<VoicePlan, Vec<ProblemKind>> {
        let name = name.trim();
        let mut candidates = self
            .catalogue
            .presets
            .iter()
            .filter(|p| p.model_id == entry.id && p.language == lang.code());
        let preset = if name.is_empty() {
            candidates.next()
        } else {
            candidates.find(|p| p.id == name)
        };
        let Some(preset) = preset else {
            // A catalogue preset of that id that does not fit this model or language is not
            // overridden by a hand-installed directory.
            if self.catalogue.presets.iter().any(|p| p.id == name) {
                return Err(vec![ProblemKind::VoicePresetUnknown {
                    name: name.to_owned(),
                }]);
            }
            return self.local_preset(name, lang);
        };
        let dir = self.dirs.presets.join(&preset.id);
        let reference = dir.join(&preset.file.name);
        let intact =
            fs::metadata(&reference).is_ok_and(|m| m.is_file() && m.len() == preset.file.size);
        if !intact {
            return Err(vec![ProblemKind::VoicePresetFileMissing {
                preset: preset.id.clone(),
                path: reference,
            }]);
        }
        Ok(VoicePlan::Preset {
            name: preset.id.clone(),
            dir,
            reference,
            lang,
        })
    }

    /// A preset installed by hand under the presets directory (see the module docs).
    fn local_preset(&self, name: &str, lang: Lang) -> Result<VoicePlan, Vec<ProblemKind>> {
        if !plain_id(name) {
            return Err(vec![ProblemKind::VoicePresetUnknown {
                name: name.to_owned(),
            }]);
        }
        let dir = self.dirs.presets.join(name);
        if !dir.is_dir() {
            return Err(vec![ProblemKind::VoicePresetUnknown {
                name: name.to_owned(),
            }]);
        }
        let reference = dir.join("reference.wav");
        if !reference.is_file() {
            return Err(vec![ProblemKind::VoicePresetFileMissing {
                preset: name.to_owned(),
                path: reference,
            }]);
        }
        match fs::read_to_string(dir.join("preset.json")) {
            Ok(text) => {
                let file: PresetFile = serde_json::from_str(&text).map_err(|e| {
                    vec![ProblemKind::VoicePresetUnreadable {
                        preset: name.to_owned(),
                        detail: format!("preset.json: {e}"),
                    }]
                })?;
                let declared = file
                    .language
                    .as_deref()
                    .map(str::trim)
                    .filter(|l| !l.is_empty());
                if declared.is_some_and(|l| Lang::from_str(l).ok() != Some(lang)) {
                    return Err(vec![ProblemKind::VoicePresetLanguage {
                        preset: name.to_owned(),
                        wanted: lang,
                        found: declared.unwrap_or_default().to_owned(),
                    }]);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                return Err(vec![ProblemKind::VoicePresetUnreadable {
                    preset: name.to_owned(),
                    detail: e.to_string(),
                }]);
            }
        }
        Ok(VoicePlan::Preset {
            name: name.to_owned(),
            dir,
            reference,
            lang,
        })
    }
}
