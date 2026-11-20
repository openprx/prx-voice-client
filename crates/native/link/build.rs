//! Build script of `prxvc-native-link`: tells the test binaries where the dynamic libraries
//! live (run-time search path for development builds) and exports the directories to the tests.
//!
//! The libraries carry the install name `@rpath/lib<name>.dylib`. A development build finds them
//! through an absolute rpath into the cargo output directory; the app bundle uses
//! `@executable_path/../Frameworks` instead (see README.md, "Packaging").

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    for (feature, dep, env) in [
        (
            "QWENTTS",
            "DEP_PRXVC_QWENTTS_LIB_DIR",
            "PRXVC_QWENTTS_LIB_DIR",
        ),
        (
            "TRANSCRIBE",
            "DEP_PRXVC_TRANSCRIBE_LIB_DIR",
            "PRXVC_TRANSCRIBE_LIB_DIR",
        ),
    ] {
        if std::env::var_os(format!("CARGO_FEATURE_{feature}")).is_none() {
            continue;
        }
        let dir =
            std::env::var(dep).unwrap_or_else(|_| panic!("{dep} is not set by the -sys crate"));
        println!("cargo:rustc-link-arg-tests=-Wl,-rpath,{dir}");
        println!("cargo:rustc-env={env}={dir}");
    }
}
