//! Catalogue: the embedded file, validation rules, warnings, candidates and
//! recommendation.

use prxvc_models::catalogue::{
    Badge, Catalogue, CatalogueError, CatalogueWarning, FileSource, ModelId, NotSelectable, Role,
};
use prxvc_models::{Arch, HardwareInfo, LangKey, Os};
use serde_json::{Value, json};

const ALL_RUNTIMES: [&str; 4] = ["whisper.cpp", "llama.cpp", "qwentts.cpp", "onnxruntime"];

fn embedded_json() -> Value {
    serde_json::to_value(Catalogue::embedded()).unwrap()
}

fn parse(v: &Value) -> Result<Catalogue, CatalogueError> {
    Catalogue::parse(&v.to_string())
}

/// Apply `f` to the embedded catalogue's JSON and return the validation errors.
fn rejected(f: impl FnOnce(&mut Value)) -> Vec<String> {
    let mut v = embedded_json();
    f(&mut v);
    match parse(&v) {
        Err(CatalogueError::Invalid(e)) => e,
        Err(other) => panic!("expected Invalid, got {other:?}"),
        Ok(_) => panic!("expected the catalogue to be rejected"),
    }
}

fn hw(gb: u32, os: Os, arch: Arch) -> HardwareInfo {
    HardwareInfo::new(gb, os, arch)
}

fn mac16() -> HardwareInfo {
    hw(16, Os::Macos, Arch::Aarch64)
}

#[test]
fn embedded_catalogue_parses_and_validates() {
    let cat = Catalogue::embedded();
    assert_eq!(cat.schema_version, 1);
    assert_eq!(cat.catalogue_version, 2026112001);
    let warnings = cat.validate(&semver::Version::new(0, 2, 0), &ALL_RUNTIMES);
    assert!(warnings.is_empty(), "{warnings:?}");
    let ids: Vec<_> = cat.models().iter().map(|m| m.id.0.as_str()).collect();
    assert_eq!(
        ids,
        [
            "silero-vad-6.2",
            "silero-vad-5.1.2",
            "whisper-small",
            "hy-mt2-7b-q4km",
            "qwen3-tts-1.7b-base-q8",
            "kokoro-82m-q8f16"
        ]
    );
    assert!(
        cat.get(&ModelId::new("silero-vad-5.1.2"))
            .unwrap()
            .is_hidden()
    );
    assert_eq!(cat.providers.len(), 5);
    assert_eq!(
        cat.setup_presets
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        ["local", "hybrid", "cloud"]
    );
}

#[test]
fn embedded_pins_are_exactly_the_ones_in_the_design() {
    let cat = Catalogue::embedded();
    let pin = |id: &str, idx: usize| {
        let f = &cat.get(&ModelId::new(id)).unwrap().files[idx];
        let rev = match &f.source {
            FileSource::Huggingface { revision, .. } => revision.clone(),
            FileSource::Url { .. } => panic!(),
        };
        (f.name.clone(), f.size, f.sha256.clone(), rev)
    };
    assert_eq!(
        pin("silero-vad-6.2", 0),
        (
            "ggml-silero-v6.2.0.bin".into(),
            885098,
            "2aa269b785eeb53a82983a20501ddf7c1d9c48e33ab63a41391ac6c9f7fb6987".into(),
            "9ffd54a1e1ee413ddf265af9913beaf518d1639b".into()
        )
    );
    assert_eq!(
        pin("whisper-small", 0),
        (
            "ggml-small.bin".into(),
            487601967,
            "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b".into(),
            "5359861c739e955e79d9a303bcbc70fb988958b1".into()
        )
    );
    assert_eq!(
        pin("hy-mt2-7b-q4km", 0),
        (
            "Hy-MT2-7B-Q4_K_M.gguf".into(),
            4624648896,
            "9f96256500f3fc1ab4d64336b58f52a949a95ad7516b0c229476eef782f9f77b".into(),
            "ab8472660ac61fac25f1af43fac2599d52a8a775".into()
        )
    );
    assert_eq!(
        pin("qwen3-tts-1.7b-base-q8", 0),
        (
            "Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf".into(),
            1847874400,
            "ac7931aeb2e7aad1a6ed6602d353a5679c9d096b18ce8204ac730a8408d572e1".into(),
            "ca27d74bc954b73dadab5b71ca265d87fc861a7c".into()
        )
    );
    assert_eq!(
        pin("qwen3-tts-1.7b-base-q8", 1),
        (
            "mmproj-Qwen3-TTS-12Hz-1.7B-Base-Q8_0.gguf".into(),
            446422912,
            "6fd65188839bcd6ecc91b277ad471e22a0edfada4699a0fe82f1165c18cfcce2".into(),
            "ca27d74bc954b73dadab5b71ca265d87fc861a7c".into()
        )
    );
    assert_eq!(
        pin("kokoro-82m-q8f16", 0),
        (
            "model_q8f16.onnx".into(),
            86033585,
            "04c658aec1b6008857c2ad10f8c589d4180d0ec427e7e6118ceb487e215c3cd0".into(),
            "1939ad2a8e416c0acfeecc08a694d14ef25f2231".into()
        )
    );
    // Hidden legacy entry used for migration.
    let v5 = &cat.get(&ModelId::new("silero-vad-5.1.2")).unwrap().files[0];
    assert_eq!(
        v5.sha256,
        "29940d98d42b91fbd05ce489f3ecf7c72f0a42f027e4875919a28fb4c04ea2cf"
    );
    assert_eq!(v5.size, 885098);
}

#[test]
fn revision_key_is_the_first_twelve_hex_of_the_first_revision() {
    let cat = Catalogue::embedded();
    assert_eq!(
        cat.get(&ModelId::new("whisper-small"))
            .unwrap()
            .revision_key(),
        "5359861c739e"
    );
}

#[test]
fn unknown_schema_version_is_ignored_as_a_whole() {
    let mut v = embedded_json();
    v["schema_version"] = json!(2);
    v["models"] = json!("this part would not even parse");
    assert!(matches!(
        parse(&v),
        Err(CatalogueError::UnsupportedSchema(2))
    ));
    assert!(matches!(
        Catalogue::parse("not json"),
        Err(CatalogueError::Json(_))
    ));
}

#[test]
fn each_validation_rule_rejects_a_crafted_bad_entry() {
    let has = |errs: &[String], needle: &str| {
        assert!(
            errs.iter().any(|e| e.contains(needle)),
            "no error mentioning {needle:?} in {errs:?}"
        )
    };
    has(
        &rejected(|v| v["models"][1]["id"] = json!("whisper-small")),
        "duplicate model id",
    );
    has(
        &rejected(|v| v["models"][0]["id"] = json!("Silero-VAD")),
        "id must match",
    );
    has(
        &rejected(|v| v["models"][0]["id"] = json!("x")),
        "id must match",
    );
    has(
        &rejected(|v| v["models"][0]["id"] = json!("-bad")),
        "id must match",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["name"] = json!("dir/file.bin")),
        "invalid file name",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["name"] = json!("..model")),
        "invalid file name",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["name"] = json!(".hidden")),
        "invalid file name",
    );
    has(
        &rejected(|v| v["models"][0]["files"] = json!([])),
        "files is empty",
    );
    has(
        &rejected(|v| {
            let f = v["models"][4]["files"][0].clone();
            v["models"][4]["files"][1] = f;
        }),
        "duplicate file name",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["sha256"] = json!("A".repeat(64))),
        "64 lowercase hex",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["sha256"] = json!("abc")),
        "64 lowercase hex",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["size"] = json!(0)),
        "size 0",
    );
    has(
        &rejected(|v| v["models"][0]["files"][0]["source"]["revision"] = json!("main")),
        "40-hex commit",
    );
    has(
        &rejected(|v| {
            v["models"][0]["files"][0]["source"] =
                json!({"kind": "url", "url": "http://example.org/a.bin"})
        }),
        "https://",
    );
    has(
        &rejected(|v| v["models"][0]["languages"] = json!([{"kind": "lang", "code": "en"}])),
        "language kind",
    );
    has(
        &rejected(|v| v["models"][2]["languages"] = json!([{"kind": "any"}])),
        "language kind",
    );
    has(
        &rejected(|v| {
            v["models"][2]["languages"] = json!([{"kind": "pair", "source": "zh", "target": "en"}])
        }),
        "language kind",
    );
    has(
        &rejected(|v| v["models"][3]["languages"] = json!([{"kind": "lang", "code": "zh"}])),
        "language kind",
    );
    has(
        &rejected(|v| v["models"][0]["languages"] = json!([])),
        "languages is empty",
    );
    has(
        &rejected(|v| v["min_app_version"] = json!("not-semver")),
        "semver",
    );
    has(
        &rejected(|v| v["models"][0]["legacy_file_names"] = json!(["a/b"])),
        "legacy file name",
    );
    has(
        &rejected(|v| v["setup_presets"][1]["roles"]["mt"]["template"] = json!("nope")),
        "unknown template",
    );
    has(
        &rejected(|v| v["providers"][3]["ineligible_reason"] = Value::Null),
        "ineligible_reason",
    );
    has(
        &rejected(|v| v["providers"][1]["id"] = json!("openai")),
        "duplicate provider id",
    );
    has(
        &rejected(|v| v["providers"][0]["roles"] = json!([])),
        "no roles",
    );
    has(
        &rejected(|v| {
            v["presets"] = json!([{"id": "p", "model_id": "ghost", "language": "en", "display_name": "x",
                "file": v["models"][0]["files"][0].clone(), "licence": "MIT"}])
        }),
        "unknown model",
    );
}

#[test]
fn unknown_runtime_names_are_allowed_but_warned_about() {
    let mut v = embedded_json();
    v["models"][2]["runtime"]["name"] = json!("future.cpp");
    let cat = parse(&v).expect("unknown runtime is not a parse error");
    let w = cat.validate(&semver::Version::new(0, 2, 0), &ALL_RUNTIMES);
    assert_eq!(
        w,
        vec![CatalogueWarning::UnsupportedRuntime {
            model: ModelId::new("whisper-small"),
            runtime: "future.cpp".into()
        }]
    );
}

#[test]
fn warnings_for_newer_app_requirement_and_unknown_supersedes() {
    let mut v = embedded_json();
    v["models"][0]["supersedes"] = json!(["silero-vad-5.1.2", "gone-model"]);
    let cat = parse(&v).unwrap();
    let w = cat.validate(&semver::Version::new(0, 1, 0), &ALL_RUNTIMES);
    assert!(w.contains(&CatalogueWarning::NeedsNewerApp {
        required: "0.2.0".into(),
        current: "0.1.0".into()
    }));
    assert!(w.contains(&CatalogueWarning::UnknownSupersedes {
        model: ModelId::new("silero-vad-6.2"),
        id: ModelId::new("gone-model")
    }));
    assert_eq!(
        w.len(),
        2,
        "the existing superseded id is not warned about: {w:?}"
    );
}

#[test]
fn lang_key_config_keys() {
    assert_eq!(LangKey::Any.config_key(), "");
    assert_eq!(LangKey::Lang("zh".into()).config_key(), "zh");
    assert_eq!(
        LangKey::Pair {
            source: "zh".into(),
            target: "en".into()
        }
        .config_key(),
        "zh>en"
    );
    for k in ["", "zh", "zh>en"] {
        assert_eq!(LangKey::from_config_key(k).config_key(), k);
    }
}

#[test]
fn candidates_follow_role_language_runtime_and_hidden() {
    let cat = Catalogue::embedded();
    let rt = ALL_RUNTIMES.as_slice();
    let vad = cat.candidates(Role::Vad, &LangKey::Any, &mac16(), rt);
    assert_eq!(vad.len(), 1, "the hidden Silero v5.1.2 is never offered");
    assert_eq!(vad[0].entry.id.0, "silero-vad-6.2");
    assert_eq!(vad[0].badge, Some(Badge::Recommended));

    let asr_zh = cat.candidates(Role::Asr, &LangKey::Lang("zh".into()), &mac16(), rt);
    assert_eq!(asr_zh.len(), 1);
    assert!(
        cat.candidates(Role::Asr, &LangKey::Lang("fr".into()), &mac16(), rt)
            .is_empty()
    );

    // Without the whisper.cpp runtime nothing is offered for asr or vad.
    let rt_small = ["llama.cpp"];
    assert!(
        cat.candidates(Role::Asr, &LangKey::Lang("zh".into()), &mac16(), &rt_small)
            .is_empty()
    );
    assert!(
        cat.candidates(Role::Vad, &LangKey::Any, &mac16(), &rt_small)
            .is_empty()
    );

    let mt = cat.candidates(
        Role::Mt,
        &LangKey::Pair {
            source: "zh".into(),
            target: "en".into(),
        },
        &mac16(),
        rt,
    );
    assert_eq!(mt.len(), 1);
    assert!(
        cat.candidates(
            Role::Mt,
            &LangKey::Pair {
                source: "fr".into(),
                target: "en".into()
            },
            &mac16(),
            rt
        )
        .is_empty()
    );
}

#[test]
fn recommendation_depends_on_the_hardware() {
    let cat = Catalogue::embedded();
    let rt = ALL_RUNTIMES.as_slice();
    let pair = LangKey::Pair {
        source: "zh".into(),
        target: "en".into(),
    };
    let rec = |role, key: &LangKey, h| cat.recommended(role, key, &h, rt).map(|e| e.id.0.clone());

    assert_eq!(
        rec(Role::Vad, &LangKey::Any, mac16()).as_deref(),
        Some("silero-vad-6.2")
    );
    // The MT rule needs 16 GB: with 8 GB it has no badge, but is still the
    // first candidate.
    assert_eq!(
        rec(Role::Mt, &pair, mac16()).as_deref(),
        Some("hy-mt2-7b-q4km")
    );
    let small = hw(8, Os::Linux, Arch::X86_64);
    let c = cat.candidates(Role::Mt, &pair, &small, rt);
    assert_eq!(c[0].badge, None);
    assert_eq!(
        rec(Role::Mt, &pair, small).as_deref(),
        Some("hy-mt2-7b-q4km")
    );
    // TTS: the "recommended" rule needs macOS on Apple silicon.
    let en = LangKey::Lang("en".into());
    let tts_mac = cat.candidates(Role::Tts, &en, &mac16(), rt);
    assert_eq!(tts_mac[0].badge, Some(Badge::Recommended));
    let tts_win = cat.candidates(Role::Tts, &en, &hw(32, Os::Windows, Arch::X86_64), rt);
    assert_eq!(tts_win[0].badge, None);
    // ASR has only a "smallest" rule: used when nothing is recommended.
    let zh = LangKey::Lang("zh".into());
    assert_eq!(
        cat.candidates(Role::Asr, &zh, &mac16(), rt)[0].badge,
        Some(Badge::Smallest)
    );
    assert_eq!(
        rec(Role::Asr, &zh, mac16()).as_deref(),
        Some("whisper-small")
    );
    // No candidates, no recommendation.
    assert_eq!(rec(Role::Asr, &LangKey::Lang("fr".into()), mac16()), None);
}

#[test]
fn candidate_order_is_rule_rank_then_name_and_non_streaming_is_not_selectable() {
    let mut v = embedded_json();
    let template = v["models"][2].clone();
    let mut clone = |id: &str, name: &str, rank: Option<u32>, streaming: &str| {
        let mut m = template.clone();
        m["id"] = json!(id);
        m["display_name"] = json!(name);
        m["files"][0]["name"] = json!(format!("{id}.bin"));
        m["streaming"] = json!(streaming);
        m["recommended_for"] = match rank {
            Some(r) => json!([{"badge": "recommended", "rank": r}]),
            None => json!([]),
        };
        v["models"].as_array_mut().unwrap().push(m);
    };
    clone("asr-zeta", "Zeta", None, "native");
    clone("asr-alpha", "Alpha", None, "native");
    clone("asr-rank5", "Rank five", Some(5), "native");
    clone("asr-rank1", "Rank one", Some(1), "native");
    clone("asr-batch", "Batch", Some(0), "none");
    let cat = parse(&v).unwrap();
    let c = cat.candidates(
        Role::Asr,
        &LangKey::Lang("zh".into()),
        &mac16(),
        ALL_RUNTIMES.as_slice(),
    );
    let order: Vec<_> = c.iter().map(|c| c.entry.id.0.as_str()).collect();
    assert_eq!(
        order,
        [
            "asr-batch",
            "asr-rank1",
            "asr-rank5",
            "whisper-small",
            "asr-alpha",
            "asr-zeta"
        ]
    );
    assert!(!c[0].selectable);
    assert_eq!(c[0].reason, Some(NotSelectable::NotStreaming));
    assert!(c[1..].iter().all(|c| c.selectable && c.reason.is_none()));
    // The recommendation skips the entry that cannot be made active.
    let rec = cat
        .recommended(
            Role::Asr,
            &LangKey::Lang("zh".into()),
            &mac16(),
            ALL_RUNTIMES.as_slice(),
        )
        .unwrap();
    assert_eq!(rec.id.0, "asr-rank1");
}
