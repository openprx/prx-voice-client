// Build script: compile the Core Audio process-tap Objective-C shim on macOS.
//
// On other platforms this is a no-op, so the crate stays cross-platform. The
// shim requires the macOS 14.4+ SDK (CATapDescription / AudioHardwareCreateProcessTap).

fn main() {
    // CARGO_CFG_TARGET_OS reflects the *target* (correct under cross-compilation),
    // unlike cfg!(target_os = ...) which reflects the build host.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "macos" {
        return;
    }

    println!("cargo:rerun-if-changed=native/prxvc_tap.m");
    println!("cargo:rerun-if-changed=native/prxvc_tap.h");

    cc::Build::new()
        .file("native/prxvc_tap.m")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .warnings(false)
        .compile("prxvc_tap");

    println!("cargo:rustc-link-lib=framework=CoreAudio");
    println!("cargo:rustc-link-lib=framework=AudioToolbox");
    println!("cargo:rustc-link-lib=framework=Foundation");
}
