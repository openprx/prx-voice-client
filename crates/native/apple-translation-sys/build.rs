//! Build script of `prxvc-apple-translation-sys`: with the `apple-translation` feature, compiles the
//! Swift bridge in `native/` with `swiftc` (Command Line Tools, no Xcode) into
//! `libprxvc_apple_translation.dylib`. Without the feature it does nothing.
//!
//! The deployment target is the app's (`MACOSX_DEPLOYMENT_TARGET`, 14.4 when unset). The bridge uses
//! the Translation framework behind `#available(macOS 26.0)` checks and links it weakly, so the
//! library loads on older systems and answers `PRXAT_OS_TOO_OLD` there. Only the `prxat_*` symbols
//! are exported. macOS only.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_APPLE_TRANSLATION").is_none() {
        return;
    }
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    assert!(
        target_os == "macos",
        "the Apple translation bridge is built for macOS only (target is {target_os})"
    );
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let native = manifest.join("native");
    println!("cargo:rerun-if-changed={}", native.display());
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let deployment =
        std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.4".to_string());
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported target architecture {other}"),
    };

    let mut sources: Vec<PathBuf> = fs::read_dir(&native)
        .expect("read native/")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "swift"))
        .collect();
    sources.sort();
    assert!(
        !sources.is_empty(),
        "no Swift sources in {}",
        native.display()
    );

    let lib_name = "prxvc_apple_translation";
    let dylib = out.join(format!("lib{lib_name}.dylib"));
    let mut swiftc = Command::new("swiftc");
    swiftc
        .args(["-emit-library", "-o"])
        .arg(&dylib)
        .args(["-module-name", "PrxvcAppleTranslation"])
        .args(["-target", &format!("{arch}-apple-macos{deployment}")])
        .args(["-swift-version", "5", "-O", "-parse-as-library"])
        .args(["-Xlinker", "-weak_framework", "-Xlinker", "Translation"])
        .args(["-framework", "Foundation"])
        .args(["-Xlinker", "-install_name"])
        .arg("-Xlinker")
        .arg(format!("@rpath/lib{lib_name}.dylib"))
        .args(["-Xlinker", "-exported_symbols_list"])
        .arg("-Xlinker")
        .arg(native.join("exports.txt"))
        .args(["-Xlinker", "-dead_strip"])
        .args(&sources);
    let status = swiftc.status().unwrap_or_else(|e| {
        panic!(
            "cannot start swiftc: {e}. The apple-translation feature needs the Swift toolchain of the Command Line Tools (xcode-select --install)."
        )
    });
    assert!(status.success(), "swiftc failed with {status}");

    // Where the bundle script picks the library up (target/<profile>/native-libs/).
    if let Some(profile_dir) = out.ancestors().nth(3) {
        let dir = profile_dir.join("native-libs");
        fs::create_dir_all(&dir).expect("create native-libs");
        fs::copy(&dylib, dir.join(dylib.file_name().expect("file name"))).expect("copy dylib");
    }
    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib={lib_name}");
    println!("cargo:lib_dir={}", out.display());
}
