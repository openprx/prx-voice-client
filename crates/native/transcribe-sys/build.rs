//! Build script of `prxvc-transcribe-sys`: with the `transcribe` feature, builds the vendored
//! transcribe.cpp and its ggml with CMake (upstream's own CMakeLists.txt, static posture)
//! and links them into `libprxvc_transcribe.dylib`. Without the feature it does nothing.

include!("../build_common.rs");

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_TRANSCRIBE").is_none() {
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

    build_runtime(&Runtime {
        lib_name: "prxvc_transcribe",
        cmake_source: manifest.join("vendor/transcribe.cpp"),
        cmake_defs: [
            "TRANSCRIBE_BUILD_SHARED=OFF",
            "TRANSCRIBE_BUILD_TESTS=OFF",
            "TRANSCRIBE_BUILD_EXAMPLES=OFF",
            "TRANSCRIBE_BUILD_TOOLS=OFF",
            "TRANSCRIBE_INSTALL=OFF",
            "TRANSCRIBE_METAL=ON",
        ]
        .iter()
        .map(|d| format!("-D{d}"))
        .collect(),
        cmake_target: "transcribe",
        main_archive: "libtranscribe.a",
        exports: manifest.join("native/exports.txt"),
    });
}
