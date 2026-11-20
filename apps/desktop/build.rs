//! Build script of `prxvc-desktop`: the development run-time search path of the native runtime
//! libraries.
//!
//! The libraries (`libprxvc_qwentts.dylib`, `libprxvc_transcribe.dylib`) carry the install name `@rpath/lib<name>.dylib` and
//! are copied by their `-sys` crates to `target/<profile>/native-libs/`. Link arguments of a library
//! crate do not reach the final binary, so the binary's own build script adds the rpath, and only when
//! a native feature is on. The app bundle script (`scripts/package-macos.sh`) replaces it with
//! `@executable_path/../Frameworks`.

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Every native feature builds one library into the same directory: one rpath serves them all.
    let native = ["CARGO_FEATURE_QWENTTS", "CARGO_FEATURE_TRANSCRIBE"];
    if native.iter().all(|f| std::env::var_os(f).is_none()) {
        return;
    }
    // OUT_DIR is <profile dir>/build/<package>-<hash>/out; the libraries sit in <profile dir>/native-libs.
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR is set by cargo"));
    let profile_dir = out
        .ancestors()
        .nth(3)
        .expect("OUT_DIR lies below the profile directory");
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-rpath,{}",
        profile_dir.join("native-libs").display()
    );
}
