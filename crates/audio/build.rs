// Build script: compile the Core Audio process-tap and device-enumeration Objective-C shims on macOS.
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
    println!("cargo:rerun-if-changed=native/prxvc_procs.m");
    println!("cargo:rerun-if-changed=native/prxvc_procs.h");
    println!("cargo:rerun-if-changed=native/prxvc_devices.m");
    println!("cargo:rerun-if-changed=native/prxvc_devices.h");
    println!("cargo:rerun-if-changed=native/prxvc_defaults.m");
    println!("cargo:rerun-if-changed=native/prxvc_defaults.h");
    println!("cargo:rerun-if-changed=native/prxvc_power.m");
    println!("cargo:rerun-if-changed=native/prxvc_power.h");
    println!("cargo:rerun-if-changed=native/prxvc_output.m");
    println!("cargo:rerun-if-changed=native/prxvc_output.h");
    println!("cargo:rerun-if-changed=native/prxvc_mic.m");
    println!("cargo:rerun-if-changed=native/prxvc_mic.h");

    cc::Build::new()
        .file("native/prxvc_tap.m")
        .file("native/prxvc_procs.m")
        .file("native/prxvc_devices.m")
        .file("native/prxvc_defaults.m")
        .file("native/prxvc_power.m")
        .file("native/prxvc_output.m")
        .file("native/prxvc_mic.m")
        .flag("-fobjc-arc")
        .flag("-fmodules")
        .warnings(false)
        .compile("prxvc_tap");

    println!("cargo:rustc-link-lib=framework=CoreAudio");
    println!("cargo:rustc-link-lib=framework=AudioToolbox");
    println!("cargo:rustc-link-lib=framework=Foundation");
    println!("cargo:rustc-link-lib=framework=AVFoundation");
    println!("cargo:rustc-link-lib=framework=IOKit");
}
