// Shared build-script logic of the native `-sys` crates (pulled in with `include!`).
//
// One runtime = one CMake project built as static archives (the runtime and its ggml),
// then linked here into ONE dynamic library that exports only the runtime's own C API.
// ggml stays inside as private code, so two runtimes with two different ggml copies
// can live in one process (pipeline-design PD-2.7.2). macOS only.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// What differs between runtimes.
struct Runtime<'a> {
    /// Dynamic library base name: `lib<lib_name>.dylib`.
    lib_name: &'a str,
    /// Directory holding the CMake project to configure.
    cmake_source: PathBuf,
    /// Extra `-D` definitions for the configure step.
    cmake_defs: Vec<String>,
    /// CMake target that builds the runtime archive.
    cmake_target: &'a str,
    /// File name of the archive that holds the C API objects (force-loaded).
    main_archive: &'a str,
    /// Exported-symbols list (glob patterns, one per line).
    exports: PathBuf,
}

fn run(cmd: &mut Command, what: &str) {
    let status = cmd.status().unwrap_or_else(|e| {
        panic!("{what}: cannot start {cmd:?}: {e}. The native build needs cmake and clang on PATH.")
    });
    assert!(status.success(), "{what} failed with {status}");
}

fn find_archives(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            find_archives(&p, out);
        } else if p.extension().is_some_and(|e| e == "a") {
            out.push(p);
        }
    }
}

/// Build the runtime and its dynamic library; returns the directory that holds it.
fn build_runtime(rt: &Runtime<'_>) -> PathBuf {
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    assert!(
        target_os == "macos",
        "the native runtimes are built for macOS only (target is {target_os})"
    );

    let out = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let build = out.join("cmake-build");
    let deployment =
        std::env::var("MACOSX_DEPLOYMENT_TARGET").unwrap_or_else(|_| "14.4".to_string());
    let jobs = std::env::var("NUM_JOBS").unwrap_or_else(|_| "4".to_string());
    // The target architecture, not the host's: `--target x86_64-apple-darwin` on an arm64 host.
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH")
        .unwrap_or_default()
        .as_str()
    {
        "aarch64" => "arm64",
        "x86_64" => "x86_64",
        other => panic!("unsupported target architecture {other}"),
    };

    let mut configure = Command::new("cmake");
    configure
        .arg("-S")
        .arg(&rt.cmake_source)
        .arg("-B")
        .arg(&build)
        .arg("-DCMAKE_BUILD_TYPE=Release")
        .arg("-DCMAKE_POSITION_INDEPENDENT_CODE=ON")
        .arg(format!("-DCMAKE_OSX_ARCHITECTURES={arch}"))
        .arg(format!("-DCMAKE_OSX_DEPLOYMENT_TARGET={deployment}"))
        .arg("-DGGML_METAL=ON")
        .arg("-DGGML_METAL_EMBED_LIBRARY=ON")
        .args(&rt.cmake_defs);
    run(&mut configure, "cmake configure");

    let mut compile = Command::new("cmake");
    compile
        .arg("--build")
        .arg(&build)
        .arg("--target")
        .arg(rt.cmake_target)
        .arg("-j")
        .arg(&jobs);
    run(&mut compile, "cmake build");

    let mut archives = Vec::new();
    find_archives(&build, &mut archives);
    let main = archives
        .iter()
        .position(|p| p.file_name().is_some_and(|n| n == rt.main_archive))
        .unwrap_or_else(|| panic!("{} not found under {}", rt.main_archive, build.display()));
    let main = archives.remove(main);

    let dylib = out.join(format!("lib{}.dylib", rt.lib_name));
    let mut link = Command::new("clang++");
    link.arg("-dynamiclib")
        .arg("-o")
        .arg(&dylib)
        .arg("-install_name")
        .arg(format!("@rpath/lib{}.dylib", rt.lib_name))
        .arg("-arch")
        .arg(arch)
        .arg(format!("-mmacosx-version-min={deployment}"))
        .arg(format!("-Wl,-force_load,{}", main.display()))
        .args(&archives)
        .args([
            "-framework",
            "Accelerate",
            "-framework",
            "Foundation",
            "-framework",
            "Metal",
            "-framework",
            "MetalKit",
        ])
        .arg("-lc++")
        .arg("-Wl,-exported_symbols_list")
        .arg(&rt.exports)
        .arg("-Wl,-dead_strip");
    run(&mut link, "link dynamic library");

    // A copy next to the cargo profile directory is where the app bundle script picks the
    // libraries up (target/<profile>/native-libs/).
    if let Some(profile_dir) = out.ancestors().nth(3) {
        let dir = profile_dir.join("native-libs");
        fs::create_dir_all(&dir).expect("create native-libs");
        fs::copy(&dylib, dir.join(dylib.file_name().expect("file name"))).expect("copy dylib");
    }

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=dylib={}", rt.lib_name);
    // Visible to dependent build scripts as DEP_<LINKS>_LIB_DIR (they add the run-time rpath).
    println!("cargo:lib_dir={}", out.display());
    println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
    println!("cargo:rerun-if-changed={}", rt.exports.display());
    out
}
