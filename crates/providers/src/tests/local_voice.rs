//! Voices installed by hand: the owner's profile per language, and presets that are directories under the presets folder.

use prxvc_media::{Direction, Lang};

use super::fixtures::*;
use crate::problem::ProblemKind;
use crate::resolve::VoiceChoice;
use crate::spec::VoicePlan;

fn write_preset(rig: &Rig, id: &str, language: Option<&str>) {
    let dir = rig.dirs.presets.join(id);
    write_wav(&dir.join("reference.wav"), 2.0);
    if let Some(language) = language {
        std::fs::write(
            dir.join("preset.json"),
            format!(r#"{{"language": "{language}"}}"#),
        )
        .unwrap();
    }
}

#[test]
fn a_hand_installed_preset_resolves_by_its_directory_name() {
    let mut rig = Rig::new();
    write_preset(&rig, "dev-standin", Some("en"));
    rig.cfg.voice.incoming_preset = "dev-standin".into();
    let resolved = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::Preset("dev-standin".into()))
        .expect("the preset resolves");
    match resolved.primary.voice {
        VoicePlan::Preset {
            name,
            reference,
            lang,
            ..
        } => {
            assert_eq!(name, "dev-standin");
            assert_eq!(lang, Lang::En);
            assert!(reference.ends_with("dev-standin/reference.wav"));
        }
        other => panic!("expected a preset, got {other:?}"),
    }
}

#[test]
fn a_preset_without_a_language_file_is_accepted_for_any_language() {
    let rig = Rig::new();
    write_preset(&rig, "plain", None);
    for lang in [Lang::En, Lang::Zh] {
        assert!(
            rig.registry()
                .resolve_tts(lang, &VoiceChoice::Preset("plain".into()))
                .is_ok()
        );
    }
}

#[test]
fn a_preset_for_another_language_is_a_typed_problem() {
    let rig = Rig::new();
    write_preset(&rig, "dev-standin", Some("zh"));
    let problems = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::Preset("dev-standin".into()))
        .unwrap_err();
    assert!(matches!(
        problems.as_slice(),
        [p] if matches!(&p.kind, ProblemKind::VoicePresetLanguage { preset, wanted: Lang::En, found } if preset == "dev-standin" && found == "zh")
    ));
}

#[test]
fn a_preset_directory_without_its_recording_or_with_a_broken_description_is_typed() {
    let rig = Rig::new();
    std::fs::create_dir_all(rig.dirs.presets.join("empty")).unwrap();
    let problems = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::Preset("empty".into()))
        .unwrap_err();
    assert!(matches!(
        problems.as_slice(),
        [p] if matches!(&p.kind, ProblemKind::VoicePresetFileMissing { preset, .. } if preset == "empty")
    ));
    write_preset(&rig, "broken", None);
    std::fs::write(rig.dirs.presets.join("broken/preset.json"), "{ nope").unwrap();
    let problems = rig
        .registry()
        .resolve_tts(Lang::En, &VoiceChoice::Preset("broken".into()))
        .unwrap_err();
    assert!(matches!(
        problems.as_slice(),
        [p] if matches!(&p.kind, ProblemKind::VoicePresetUnreadable { preset, .. } if preset == "broken")
    ));
}

#[test]
fn a_missing_preset_directory_and_an_unsafe_name_are_unknown_presets() {
    let rig = Rig::new();
    for name in ["ghost", "../voices"] {
        let problems = rig
            .registry()
            .resolve_tts(Lang::En, &VoiceChoice::Preset(name.into()))
            .unwrap_err();
        assert!(matches!(
            problems.as_slice(),
            [p] if matches!(&p.kind, ProblemKind::VoicePresetUnknown { .. })
        ));
    }
}

#[test]
fn the_owners_profile_for_a_language_wins_over_the_plain_profile() {
    let rig = Rig::new();
    // `OWNER` is the configured profile; `<OWNER>-zh` exists for Chinese only.
    let zh_profile = format!("{OWNER}-zh");
    rig.write_profile(&zh_profile, true);
    let plan = |lang| {
        rig.registry()
            .resolve_tts(lang, &VoiceChoice::OwnClone)
            .expect("the owner's voice resolves")
            .primary
            .voice
    };
    match plan(Lang::Zh) {
        VoicePlan::Cloned { profile_id, .. } => assert_eq!(profile_id, zh_profile),
        other => panic!("expected a cloned voice, got {other:?}"),
    }
    match plan(Lang::En) {
        VoicePlan::Cloned { profile_id, .. } => assert_eq!(profile_id, OWNER),
        other => panic!("expected a cloned voice, got {other:?}"),
    }
}

#[test]
fn directions_can_be_resolved_one_at_a_time() {
    let mut rig = Rig::new();
    // The incoming direction has no preset: resolving both fails, resolving the outgoing one alone works.
    rig.cfg.voice.incoming_preset = "ghost".into();
    let both = rig.registry().resolve_call().unwrap_err();
    assert!(
        both.iter()
            .any(|p| matches!(p.kind, ProblemKind::VoicePresetUnknown { .. }))
    );
    let outgoing = rig
        .registry()
        .resolve_directions(&[Direction::Outgoing])
        .expect("the outgoing direction needs no preset");
    assert_eq!(outgoing.directions.len(), 1);
    assert_eq!(outgoing.directions[0].direction, Direction::Outgoing);
    let incoming = rig
        .registry()
        .resolve_directions(&[Direction::Incoming])
        .unwrap_err();
    assert!(
        incoming
            .iter()
            .any(|p| matches!(p.kind, ProblemKind::VoicePresetUnknown { .. }))
    );
}
