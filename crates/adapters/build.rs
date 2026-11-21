//! Build script of `prxvc-adapters`: for the `qwentts` and `transcribe` features, tells this crate's own test and
//! example binaries where the native dynamic library lives (an absolute run-time search path for
//! development builds). The library has the install name `@rpath/libprxvc_qwentts.dylib`; the app
//! bundle uses `@executable_path/../Frameworks` instead (crates/native/README.md, "Packaging").
//!
//! Link arguments of a library crate do not reach the crates that depend on it: an executable
//! that links `prxvc-adapters` with the `qwentts` feature adds the same rpath itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_QWENTTS").is_some() {
        let dir = std::env::var("DEP_PRXVC_QWENTTS_LIB_DIR").unwrap_or_else(|_| {
            panic!("DEP_PRXVC_QWENTTS_LIB_DIR is not set by prxvc-qwentts-sys")
        });
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        println!("cargo:rustc-env=PRXVC_QWENTTS_LIB_DIR={dir}");
    }
    // The `transcribe` feature adds a second library (`libprxvc_transcribe.dylib`) with its own
    // run-time search path entry.
    if std::env::var_os("CARGO_FEATURE_TRANSCRIBE").is_some() {
        let dir = std::env::var("DEP_PRXVC_TRANSCRIBE_LIB_DIR").unwrap_or_else(|_| {
            panic!("DEP_PRXVC_TRANSCRIBE_LIB_DIR is not set by prxvc-transcribe-sys")
        });
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        println!("cargo:rustc-env=PRXVC_TRANSCRIBE_LIB_DIR={dir}");
    }
    // The `apple-translation` feature adds the Swift bridge library.
    if std::env::var_os("CARGO_FEATURE_APPLE_TRANSLATION").is_some() {
        let dir = std::env::var("DEP_PRXVC_APPLE_TRANSLATION_LIB_DIR").unwrap_or_else(|_| {
            panic!("DEP_PRXVC_APPLE_TRANSLATION_LIB_DIR is not set by prxvc-apple-translation-sys")
        });
        println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
        println!("cargo:rustc-env=PRXVC_APPLE_TRANSLATION_LIB_DIR={dir}");
    }
}
