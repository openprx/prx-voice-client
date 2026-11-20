//! Link and isolation checks for the two native runtimes in one process (CT-9).
//!
//! Needs both features: `cargo test -p prxvc-native-link --features qwentts,transcribe`.
//! The tests without `#[ignore]` need no model files and no GPU: they inspect the built dynamic
//! libraries and call version and layout entries. The tests that need the benchmark models and the
//! GPU are in `live.rs` (all `#[ignore]`d).
#![cfg(all(feature = "qwentts", feature = "transcribe"))]

use prxvc_native_link::{asr, tts};
use std::path::PathBuf;
use std::process::Command;

fn lib(env_dir: &str, name: &str) -> PathBuf {
    PathBuf::from(env_dir).join(format!("lib{name}.dylib"))
}

fn qwentts_lib() -> PathBuf {
    lib(env!("PRXVC_QWENTTS_LIB_DIR"), "prxvc_qwentts")
}

fn transcribe_lib() -> PathBuf {
    lib(env!("PRXVC_TRANSCRIBE_LIB_DIR"), "prxvc_transcribe")
}

fn tool(cmd: &str, args: &[&str], path: &PathBuf) -> String {
    let out = Command::new(cmd)
        .args(args)
        .arg(path)
        .output()
        .unwrap_or_else(|e| panic!("cannot run {cmd}: {e}"));
    assert!(
        out.status.success(),
        "{cmd} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Exported symbol names of a library (`nm -gU`).
fn exported(path: &PathBuf) -> Vec<String> {
    tool("nm", &["-gU"], path)
        .lines()
        .filter_map(|l| l.split_whitespace().last())
        .map(str::to_string)
        .collect()
}

fn check_exports(path: &PathBuf, prefix: &str) -> usize {
    let syms = exported(path);
    assert!(!syms.is_empty(), "{} exports nothing", path.display());
    let bad: Vec<&String> = syms.iter().filter(|s| !s.starts_with(prefix)).collect();
    assert!(
        bad.is_empty(),
        "{} exports symbols outside `{prefix}*`: {bad:?}",
        path.display()
    );
    let leaked: Vec<&String> = syms
        .iter()
        .filter(|s| s.to_lowercase().contains("ggml") || s.to_lowercase().contains("gguf"))
        .collect();
    assert!(
        leaked.is_empty(),
        "{} exports ggml or gguf symbols: {leaked:?}",
        path.display()
    );
    syms.len()
}

#[test]
fn qwentts_library_exports_only_its_c_api() {
    let n = check_exports(&qwentts_lib(), "_qt_");
    println!("libprxvc_qwentts.dylib exports {n} symbols, all `_qt_*`");
}

#[test]
fn transcribe_library_exports_only_its_c_api() {
    let n = check_exports(&transcribe_lib(), "_transcribe_");
    println!("libprxvc_transcribe.dylib exports {n} symbols, all `_transcribe_*`");
}

#[test]
fn libraries_depend_only_on_system_libraries_and_themselves() {
    for (path, own) in [
        (qwentts_lib(), "libprxvc_qwentts.dylib"),
        (transcribe_lib(), "libprxvc_transcribe.dylib"),
    ] {
        let listing = tool("otool", &["-L"], &path);
        for dep in listing
            .lines()
            .skip(1)
            .filter_map(|l| l.split_whitespace().next())
        {
            let ok = dep == format!("@rpath/{own}")
                || dep.starts_with("/System/Library/")
                || dep.starts_with("/usr/lib/");
            assert!(ok, "{own} depends on {dep}");
        }
    }
}

#[test]
fn both_runtimes_load_in_one_process_and_answer() {
    let tts_version = tts::version();
    let asr_version = asr::version();
    println!("qwentts.cpp {tts_version}; transcribe.cpp {asr_version}");
    assert!(
        tts_version.starts_with("2edb35f"),
        "unexpected qwentts build: {tts_version}"
    );
    assert!(!asr_version.is_empty());
}

#[test]
fn transcribe_struct_layouts_match_the_library() {
    use prxvc_transcribe_sys as s;
    use std::mem::{align_of, size_of};
    let cases: [(&str, i32, usize, usize); 7] = [
        (
            "model_load_params",
            s::TRANSCRIBE_ABI_MODEL_LOAD_PARAMS,
            size_of::<s::transcribe_model_load_params>(),
            align_of::<s::transcribe_model_load_params>(),
        ),
        (
            "session_params",
            s::TRANSCRIBE_ABI_SESSION_PARAMS,
            size_of::<s::transcribe_session_params>(),
            align_of::<s::transcribe_session_params>(),
        ),
        (
            "run_params",
            s::TRANSCRIBE_ABI_RUN_PARAMS,
            size_of::<s::transcribe_run_params>(),
            align_of::<s::transcribe_run_params>(),
        ),
        (
            "stream_params",
            s::TRANSCRIBE_ABI_STREAM_PARAMS,
            size_of::<s::transcribe_stream_params>(),
            align_of::<s::transcribe_stream_params>(),
        ),
        (
            "segment",
            s::TRANSCRIBE_ABI_SEGMENT,
            size_of::<s::transcribe_segment>(),
            align_of::<s::transcribe_segment>(),
        ),
        (
            "stream_update",
            s::TRANSCRIBE_ABI_STREAM_UPDATE,
            size_of::<s::transcribe_stream_update>(),
            align_of::<s::transcribe_stream_update>(),
        ),
        (
            "stream_text",
            s::TRANSCRIBE_ABI_STREAM_TEXT,
            size_of::<s::transcribe_stream_text>(),
            align_of::<s::transcribe_stream_text>(),
        ),
    ];
    for (name, id, size, align) in cases {
        // SAFETY: both entries only read a static table.
        let (lib_size, lib_align) = unsafe {
            (
                s::transcribe_abi_struct_size(id),
                s::transcribe_abi_struct_align(id),
            )
        };
        assert_eq!((size, align), (lib_size, lib_align), "layout of {name}");
    }
}

#[test]
fn qwentts_default_params_have_the_documented_values() {
    use prxvc_qwentts_sys as s;
    let mut init = std::mem::MaybeUninit::<s::qt_init_params>::uninit();
    // SAFETY: the call fully initialises the struct.
    let init = unsafe {
        s::qt_init_default_params(init.as_mut_ptr());
        init.assume_init()
    };
    // Header: use_fa true, clamp_fp16 false, max_batch 1, codec_chunk_sec 24.0, device NULL.
    assert_eq!(init.abi_version, s::QT_ABI_VERSION);
    assert!(init.use_fa && !init.clamp_fp16);
    assert_eq!(init.max_batch, 1);
    assert!((init.codec_chunk_sec - 24.0).abs() < 1e-6);
    assert!(init.device.is_null() && init.talker_path.is_null());

    let mut p = std::mem::MaybeUninit::<s::qt_tts_params>::uninit();
    // SAFETY: the call fully initialises the struct.
    let p = unsafe {
        s::qt_tts_default_params(p.as_mut_ptr());
        p.assume_init()
    };
    assert_eq!(p.abi_version, s::QT_ABI_VERSION);
    assert_eq!(p.seed, -1);
    assert!(p.on_chunk.is_none() && p.cancel.is_none() && p.text.is_null());
}
