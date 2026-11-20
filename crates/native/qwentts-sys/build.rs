//! Build script of `prxvc-qwentts-sys`: with the `qwentts` feature, builds the vendored
//! qwentts.cpp and its ggml with CMake and links them into `libprxvc_qwentts.dylib`.
//! Without the feature it does nothing.

include!("../build_common.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_QWENTTS").is_none() {
        return;
    }
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("native").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("vendor").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        manifest.join("VENDOR.toml").display()
    );

    // qt_version() reports "<short commit> (<commit date>)": the pin in VENDOR.toml,
    // because the vendored tree carries no .git directory.
    let version = "2edb35f (2026-11-17)";
    build_runtime(&Runtime {
        lib_name: "prxvc_qwentts",
        cmake_source: manifest.join("native"),
        cmake_defs: vec![format!("-DQWEN_VERSION_STRING={version}")],
        cmake_target: "qwen-core",
        main_archive: "libqwen-core.a",
        exports: manifest.join("native/exports.txt"),
    });
}
