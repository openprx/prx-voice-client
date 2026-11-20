//! `prxvc-dev install-voices`: install the owner's voice profile and a development preset voice, set them in the configuration when nothing is set, and prepare the owner's voice for the synthesis model.
//!
//! * The owner's recordings (`zh_owner.wav`, `en_owner.wav` with their texts) become the
//!   profiles `owner-zh` and `owner-en` under the voices directory. The registry picks the profile
//!   of the language it speaks (`<voice.self_profile_id>-<language>`), so the configured id is
//!   `owner`. Profiles use the embedding-only mode (`x_vector_only`), the default of the voice
//!   cloning decision: the texts are stored with the profile but not used for conditioning.
//! * The stand-in recordings made with the system synthesizer become the presets `dev-standin`
//!   (Chinese, the default listening language) and `dev-standin-en` under `voice-presets`. They
//!   are local test material, not the owner's voice, and are never shipped.
//! * The configuration changes only when `voice.self_profile_id` or `voice.incoming_preset` is
//!   empty; the file is backed up first.

use std::fs;
use std::path::{Path, PathBuf};

use prxvc_adapters::tts::qwentts::load_reference_wav;
use prxvc_config::paths::AppPaths;
use prxvc_media::Lang;

use crate::call_kit::config::{load_current, paths_for};
use crate::call_kit::providers::ProviderHost;

const DEFAULT_REFS: &str = "/Users/nn/worker/voice/bench/tts/refs";
/// `voice.self_profile_id` when none is set.
const PROFILE_ID: &str = "owner";
/// `voice.incoming_preset` when none is set.
const PRESET_ID: &str = "dev-standin";

struct Options {
    refs: PathBuf,
    prepare: bool,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut refs = PathBuf::from(DEFAULT_REFS);
    let mut prepare = true;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--refs" => refs = PathBuf::from(it.next().ok_or("--refs needs a directory")?),
            "--no-prepare" => prepare = false,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(Options { refs, prepare })
}

/// Copy `src` to `dst` unless `dst` already holds the same bytes; says what happened.
fn install_file(src: &Path, dst: &Path) -> Result<&'static str, String> {
    let bytes = fs::read(src).map_err(|e| format!("{}: {e}", src.display()))?;
    if fs::read(dst).is_ok_and(|old| old == bytes) {
        return Ok("unchanged");
    }
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::write(dst, bytes).map_err(|e| format!("{}: {e}", dst.display()))?;
    Ok("installed")
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
}

/// The profile `id` for `lang` from a recording and its text.
fn install_profile(
    paths: &AppPaths,
    refs: &Path,
    stem: &str,
    id: &str,
    lang: Lang,
) -> Result<(), String> {
    let wav = refs.join(format!("{stem}.wav"));
    let text = fs::read_to_string(refs.join(format!("{stem}.txt")))
        .map_err(|e| format!("{stem}.txt: {e}"))?
        .trim()
        .to_owned();
    let dir = paths.voices_dir.join(id);
    let state = install_file(&wav, &dir.join("reference.wav"))?;
    let reference = load_reference_wav(&dir.join("reference.wav"), id, lang, None)
        .map_err(|e| e.to_string())?;
    let profile_file = dir.join("profile.json");
    let created = fs::read_to_string(&profile_file)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("created_at").cloned())
        .unwrap_or_else(|| serde_json::Value::String(chrono::Utc::now().to_rfc3339()));
    let existing_id = fs::read_to_string(&profile_file)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("id").cloned())
        .unwrap_or_else(|| serde_json::Value::String(uuid::Uuid::new_v4().to_string()));
    write_json(
        &profile_file,
        &serde_json::json!({
            "schema_version": 1,
            "id": existing_id,
            "display_name": format!("Owner ({})", lang.code()),
            "source": "captured",
            "language": lang.code(),
            "transcript": text,
            "x_vector_only": true,
            "duration_s": reference.samples.len() as f64 / f64::from(reference.rate.hz()),
            "sample_rate": reference.rate.hz(),
            "created_at": created,
        }),
    )?;
    println!(
        "voice profile {id} ({}): reference {state}, {}",
        lang.code(),
        dir.display()
    );
    Ok(())
}

/// A preset directory with its recording and description.
fn install_preset(
    paths: &AppPaths,
    refs: &Path,
    stem: &str,
    id: &str,
    lang: Lang,
) -> Result<(), String> {
    let dir = paths.data_dir.join("voice-presets").join(id);
    let state = install_file(
        &refs.join(format!("{stem}.wav")),
        &dir.join("reference.wav"),
    )?;
    write_json(
        &dir.join("preset.json"),
        &serde_json::json!({
            "language": lang.code(),
            "description": "DEVELOPMENT STAND-IN made with the system synthesizer; not a product voice and not the owner's voice",
        }),
    )?;
    println!(
        "preset voice {id} ({}): reference {state}, {}",
        lang.code(),
        dir.display()
    );
    Ok(())
}

/// Set the empty voice keys; backs up the file first. Returns what changed.
fn update_config(paths: &AppPaths) -> Result<Vec<String>, String> {
    let current = load_current();
    let mut changes = Vec::new();
    if current.voice.self_profile_id.trim().is_empty() {
        changes.push(format!("voice.self_profile_id = \"{PROFILE_ID}\""));
    }
    if current.voice.incoming_preset.trim().is_empty() {
        changes.push(format!("voice.incoming_preset = \"{PRESET_ID}\""));
    }
    if changes.is_empty() {
        return Ok(changes);
    }
    let file = paths.config_dir.join(prxvc_config::CONFIG_FILE_NAME);
    if file.is_file() {
        let backup = paths.config_dir.join(format!(
            "{}.bak-{}",
            prxvc_config::CONFIG_FILE_NAME,
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ));
        fs::copy(&file, &backup).map_err(|e| format!("backup of the configuration: {e}"))?;
        println!("configuration backed up to {}", backup.display());
    }
    let manager = prxvc_config::AppConfigManager::with_dir(paths.config_dir.clone());
    manager
        .update(|c| {
            if c.voice.self_profile_id.trim().is_empty() {
                c.voice.self_profile_id = PROFILE_ID.to_owned();
            }
            if c.voice.incoming_preset.trim().is_empty() {
                c.voice.incoming_preset = PRESET_ID.to_owned();
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(changes)
}

/// Install, configure and prepare; `true` when everything went through.
pub(crate) fn run(args: &[String]) -> Result<bool, String> {
    let opts = parse(args)?;
    let config = load_current();
    let paths = paths_for(&config)?;
    install_profile(&paths, &opts.refs, "zh_owner", "owner-zh", Lang::Zh)?;
    install_profile(&paths, &opts.refs, "en_owner", "owner-en", Lang::En)?;
    install_preset(&paths, &opts.refs, "zh_ref", PRESET_ID, Lang::Zh)?;
    install_preset(&paths, &opts.refs, "en_ref", "dev-standin-en", Lang::En)?;
    let changes = update_config(&paths)?;
    if changes.is_empty() {
        println!("configuration unchanged (a voice profile and a preset are already set)");
    } else {
        for c in &changes {
            println!("configuration: {c}");
        }
    }
    if !opts.prepare {
        println!("preparation skipped (--no-prepare)");
        return Ok(true);
    }
    // The registry reads the configuration as it is now, the keys just set included.
    let config = load_current();
    let registry = ProviderHost::registry_for(&config, None).map_err(|f| f.detail)?;
    let mut ok = true;
    for lang in [Lang::Zh, Lang::En] {
        match registry.prepare_cloned_voice(lang) {
            Ok(done) => println!(
                "owner's voice for {}: prepared (profile {}, {}, {:.1} s)",
                lang.code(),
                done.profile_id,
                if done.cache_hit {
                    "already prepared"
                } else {
                    "derived now"
                },
                done.elapsed.as_secs_f32()
            ),
            Err(e) => {
                println!("owner's voice for {}: NOT prepared: {e}", lang.code());
                ok = false;
            }
        }
    }
    drop(registry);
    Ok(ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(dir: &Path) -> AppPaths {
        AppPaths::from_roots(
            dir.join("config"),
            dir.join("data"),
            dir.join("data/models"),
            &prxvc_config::AppConfig::default(),
        )
    }

    fn write_wav(path: &Path) {
        let spec = hound_like_header(24_000, 24_000);
        fs::write(path, spec).unwrap();
    }

    /// A valid mono 16-bit WAV of one second of silence.
    fn hound_like_header(rate: u32, samples: u32) -> Vec<u8> {
        let data_len = samples * 2;
        let mut v = Vec::new();
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * 2).to_le_bytes());
        v.extend_from_slice(&2u16.to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        v.extend(std::iter::repeat_n(0u8, data_len as usize));
        v
    }

    #[test]
    fn a_profile_is_installed_once_and_describes_its_recording() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let refs = dir.path().join("refs");
        fs::create_dir_all(&refs).unwrap();
        write_wav(&refs.join("zh_owner.wav"));
        fs::write(refs.join("zh_owner.txt"), "你好。\n").unwrap();
        let paths = paths(dir.path());
        install_profile(&paths, &refs, "zh_owner", "owner-zh", Lang::Zh).expect("installed");
        let file = paths.voices_dir.join("owner-zh/profile.json");
        let first: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(first["language"], "zh");
        assert_eq!(first["transcript"], "你好。");
        assert_eq!(first["x_vector_only"], true);
        assert_eq!(first["sample_rate"], 24_000);
        assert!((first["duration_s"].as_f64().unwrap() - 1.0).abs() < 1e-6);
        // Installing again keeps the identity and the creation time.
        install_profile(&paths, &refs, "zh_owner", "owner-zh", Lang::Zh).expect("installed again");
        let second: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(first["id"], second["id"]);
        assert_eq!(first["created_at"], second["created_at"]);
    }

    #[test]
    fn a_preset_carries_its_language_and_says_it_is_a_stand_in() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let refs = dir.path().join("refs");
        fs::create_dir_all(&refs).unwrap();
        write_wav(&refs.join("zh_ref.wav"));
        let paths = paths(dir.path());
        install_preset(&paths, &refs, "zh_ref", "dev-standin", Lang::Zh).expect("installed");
        let preset = paths.data_dir.join("voice-presets/dev-standin");
        assert!(preset.join("reference.wav").is_file());
        let meta: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(preset.join("preset.json")).unwrap()).unwrap();
        assert_eq!(meta["language"], "zh");
        assert!(meta["description"].as_str().unwrap().contains("STAND-IN"));
    }

    #[test]
    fn a_file_is_replaced_only_when_its_content_differs() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let (a, b) = (dir.path().join("a"), dir.path().join("sub/b"));
        fs::write(&a, b"1").unwrap();
        assert_eq!(install_file(&a, &b).unwrap(), "installed");
        assert_eq!(install_file(&a, &b).unwrap(), "unchanged");
        fs::write(&a, b"2").unwrap();
        assert_eq!(install_file(&a, &b).unwrap(), "installed");
    }

    #[test]
    fn the_arguments_choose_the_recordings_and_whether_to_prepare() {
        let o = parse(&["--refs".into(), "/r".into(), "--no-prepare".into()]).unwrap();
        assert_eq!(o.refs, PathBuf::from("/r"));
        assert!(!o.prepare);
        assert!(parse(&["--x".into()]).is_err());
    }
}
