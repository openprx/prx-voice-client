//! Build script of `prxvc-adapters`: for the `qwentts` feature, tells this crate's own test and
//! example binaries where the native dynamic library lives (an absolute run-time search path for
//! development builds). The library has the install name `@rpath/libprxvc_qwentts.dylib`; the app
//! bundle uses `@executable_path/../Frameworks` instead (crates/native/README.md, "Packaging").
//!
//! Link arguments of a library crate do not reach the crates that depend on it: an executable
//! that links `prxvc-adapters` with the `qwentts` feature adds the same rpath itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_FEATURE_QWENTTS").is_none() {
        return;
    }
    let dir = std::env::var("DEP_PRXVC_QWENTTS_LIB_DIR")
        .unwrap_or_else(|_| panic!("DEP_PRXVC_QWENTTS_LIB_DIR is not set by prxvc-qwentts-sys"));
    println!("cargo:rustc-link-arg=-Wl,-rpath,{dir}");
    println!("cargo:rustc-env=PRXVC_QWENTTS_LIB_DIR={dir}");
}
