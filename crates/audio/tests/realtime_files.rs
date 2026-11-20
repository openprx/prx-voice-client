//! Mechanical check of the real-time convention (code standard CS-4.1.1): code that runs on a device callback lives in `*_rt.rs` files and never allocates, locks, logs or sends on a channel.
//!
//! The convention is described in the crate docs of `prxvc-audio`. This test
//! reads the sources as text, so it needs no audio hardware:
//!
//! 1. A `*_rt.rs` file, outside its `#[cfg(test)]` section and with comments
//!    removed, must not mention any token of [`FORBIDDEN`].
//! 2. Every source file that registers a device callback
//!    (`build_input_stream(` or `build_output_stream(`) must pass a closure
//!    whose body is a single call of a function imported from a `*_rt` module.

use std::fs;
use std::path::{Path, PathBuf};

/// Tokens that must not appear in real-time code: locks, allocation, logging,
/// channel sends.
const FORBIDDEN: &[&str] = &[
    "Mutex",
    "RwLock",
    "lock(",
    "parking_lot",
    "Vec::new",
    "vec!",
    "Vec::with_capacity",
    "Box::new",
    "Arc::new",
    "String::",
    "format!",
    ".to_vec(",
    ".to_string(",
    ".to_owned(",
    ".collect(",
    "tracing::",
    "println!",
    "eprintln!",
    "mpsc",
    "Sender",
    ".send(",
    "try_send",
];

fn src_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn source_files() -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(src_dir())
        .expect("src directory")
        .map(|e| e.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|x| x == "rs"))
        .collect();
    files.sort();
    files
}

fn is_rt_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.ends_with("_rt.rs"))
}

/// The code of a file without its test section and without comments.
fn production_code(text: &str) -> String {
    let mut code = String::new();
    for line in text.lines() {
        if line.trim() == "#[cfg(test)]" {
            break;
        }
        let line = line.split("//").next().unwrap_or("");
        code.push_str(line);
        code.push('\n');
    }
    code
}

/// Forbidden tokens found in real-time code.
fn forbidden_tokens(code: &str) -> Vec<&'static str> {
    FORBIDDEN
        .iter()
        .copied()
        .filter(|t| code.contains(t))
        .collect()
}

/// Names imported from `*_rt` modules (`use crate::x_rt::name;` or `{a, b}`).
fn imported_rt_functions(code: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in code.lines().map(str::trim) {
        if !line.starts_with("use crate::") || !line.contains("_rt::") {
            continue;
        }
        let tail = line.rsplit("::").next().unwrap_or("");
        names.extend(
            tail.trim_end_matches(';')
                .trim_matches(|c| c == '{' || c == '}')
                .split(',')
                .map(|n| n.trim().to_owned())
                .filter(|n| !n.is_empty()),
        );
    }
    names
}

/// The body of the first closure (`move |...| { ... }`) after each device
/// callback registration, as `(registration, body)` pairs.
fn registered_callback_bodies(code: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    for call in ["build_input_stream(", "build_output_stream("] {
        let mut rest = code;
        while let Some(at) = rest.find(call) {
            rest = &rest[at + call.len()..];
            let Some(closure) = rest.find("move |") else {
                bodies.push(String::new());
                continue;
            };
            let after = &rest[closure..];
            let Some(open) = after.find('{') else {
                bodies.push(String::new());
                continue;
            };
            let mut depth = 0usize;
            let mut end = after.len();
            for (i, c) in after[open..].char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = open + i;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            bodies.push(after[open + 1..end].trim().to_owned());
        }
    }
    bodies
}

/// Whether a callback body is one call of one of `rt_functions`.
fn is_single_rt_call(body: &str, rt_functions: &[String]) -> bool {
    rt_functions
        .iter()
        .any(|f| body.starts_with(&format!("{f}(")))
        && body.matches(';').count() <= 1
        && !body.contains('\n')
}

#[test]
fn real_time_files_exist() {
    let rt: Vec<_> = source_files()
        .into_iter()
        .filter(|p| is_rt_file(p))
        .collect();
    assert!(
        rt.len() >= 3,
        "expected the input callback, the output callback and the playback ring in `*_rt.rs` files, found {rt:?}"
    );
}

#[test]
fn real_time_files_use_no_lock_allocation_logging_or_channel_send() {
    let mut violations = Vec::new();
    for path in source_files().into_iter().filter(|p| is_rt_file(p)) {
        let code = production_code(&fs::read_to_string(&path).expect("readable source"));
        for token in forbidden_tokens(&code) {
            violations.push(format!("{} mentions `{token}`", path.display()));
        }
    }
    assert!(
        violations.is_empty(),
        "real-time code rules broken:\n{}",
        violations.join("\n")
    );
}

#[test]
fn registered_device_callbacks_only_call_real_time_functions() {
    let mut registrations = 0;
    let mut violations = Vec::new();
    for path in source_files() {
        let code = production_code(&fs::read_to_string(&path).expect("readable source"));
        let bodies = registered_callback_bodies(&code);
        if bodies.is_empty() {
            continue;
        }
        let rt_functions = imported_rt_functions(&code);
        for body in bodies {
            registrations += 1;
            if !is_single_rt_call(&body, &rt_functions) {
                violations.push(format!(
                    "{}: device callback body is not a single call of a `*_rt` function: `{body}`",
                    path.display()
                ));
            }
        }
    }
    assert!(
        registrations >= 2,
        "found {registrations} device callbacks, expected the input and the output one"
    );
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

#[test]
fn the_checker_flags_what_it_should_and_ignores_comments_and_tests() {
    let bad = "fn cb(m: &Mutex<u8>) { let v = vec![1]; tracing::info!(\"x\"); }";
    let found = forbidden_tokens(&production_code(bad));
    assert!(found.contains(&"Mutex") && found.contains(&"vec!") && found.contains(&"tracing::"));

    let quiet = "// Mutex, vec! and lock( are only named in this comment\nfn cb() {}\n#[cfg(test)]\nmod t { fn x() { let _ = Vec::new(); } }\n";
    assert!(forbidden_tokens(&production_code(quiet)).is_empty());

    let rt = vec!["fill".to_owned()];
    assert!(is_single_rt_call("fill(&a, &b, data);", &rt));
    assert!(!is_single_rt_call("fill(&a, &b, data);\nlog();", &rt));
    assert!(!is_single_rt_call("other(data);", &rt));

    let registered =
        "dev.build_output_stream(&c, move |d: &mut [f32], _: &I| { fill(&r, d); }, |e| {}, None)";
    assert_eq!(
        registered_callback_bodies(registered),
        vec!["fill(&r, d);".to_owned()]
    );
    let sneaky =
        "dev.build_input_stream(&c, move |d: &[f32], _| { let v = Vec::new(); fill(d); }, e, None)";
    assert!(!is_single_rt_call(
        &registered_callback_bodies(sneaky)[0],
        &rt
    ));
}
