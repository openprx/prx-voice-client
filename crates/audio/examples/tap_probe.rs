//! Validation probe for the Core Audio process tap (macOS 14.4+).
//!
//! Proves that we can capture a given application's audio and that the stream
//! is non-silent (some apps can deliver silence through a tap). Run it against
//! the meeting app while audio is playing.
//!
//! Usage:
//!   cargo run -p prxvc-audio --example tap_probe -- <PID | bundle.id> [SECONDS] [--mute MODE]
//!
//! `MODE` is `unmuted` (default), `muted` or `muted-when-tapped`. The muted
//! modes silence the target application on this computer while the probe runs;
//! do not use them on an application someone is listening to.
//!
//! A first run may trigger the macOS system-audio-recording permission prompt.
//! If denied or on macOS < 14.4, start fails with a message saying what to do.

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    use prxvc_audio::core_audio_tap::{CoreAudioTap, frame_peak_amplitude};
    use prxvc_audio::tap_types::{TapConfig, TapMuteMode, TapTarget};

    let mut positional = Vec::new();
    let mut mute = TapMuteMode::Unmuted;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--mute" {
            let Some(mode) = args.next().as_deref().and_then(TapMuteMode::parse) else {
                eprintln!("--mute needs one of: unmuted, muted, muted-when-tapped");
                std::process::exit(2);
            };
            mute = mode;
        } else {
            positional.push(arg);
        }
    }
    let Some(target_arg) = positional.first() else {
        eprintln!("usage: tap_probe <PID | bundle.id> [SECONDS] [--mute MODE]");
        eprintln!("  find a PID with e.g.  pgrep -x zoom.us");
        std::process::exit(2);
    };
    let target = match target_arg.parse::<i32>() {
        Ok(pid) => TapTarget::Pid(pid),
        Err(_) => TapTarget::BundleId(target_arg.clone()),
    };
    let secs: u64 = positional.get(1).and_then(|s| s.parse().ok()).unwrap_or(5);
    let label = target.describe();

    println!(
        "[tap_probe] starting tap on {label} for {secs}s, mute={mute:?}, bundle-id form available={}, permission={:?}",
        prxvc_audio::core_audio_tap::bundle_form_available(),
        prxvc_audio::core_audio_tap::permission_status(),
    );
    let (tap, mut rx) = match CoreAudioTap::start(TapConfig { target, mute }) {
        Ok(pair) => pair,
        Err(e) => {
            eprintln!("[tap_probe] failed to start tap: {e}");
            std::process::exit(1);
        }
    };

    let deadline = Instant::now() + Duration::from_secs(secs);
    let mut frames: u64 = 0;
    let mut samples: u64 = 0;
    let mut peak: f32 = 0.0;
    let mut sample_rate: u32 = 0;
    let mut channels: u16 = 0;

    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Some(frame) => {
                frames += 1;
                sample_rate = frame.sample_rate;
                channels = frame.channels;
                let bytes_per_sample = std::mem::size_of::<f32>() as u64;
                samples += frame.data.len() as u64 / bytes_per_sample;
                let p = frame_peak_amplitude(&frame);
                if p > peak {
                    peak = p;
                }
            }
            None => println!("[tap_probe] ...no frames in last 500ms (is audio playing?)"),
        }
        while let Ok(event) = tap.events().try_recv() {
            println!("[tap_probe] event: {event:?}");
        }
    }
    let stats = tap.stats();

    println!("--------------------------------------------------");
    println!("[tap_probe] frames={frames} samples={samples} format={sample_rate}Hz/{channels}ch");
    println!("[tap_probe] peak amplitude = {peak:.4}");
    println!("[tap_probe] stats: {stats:?}");
    if frames == 0 {
        println!("[tap_probe] RESULT: NO FRAMES — tap not delivering (check pid / permission).");
        std::process::exit(1);
    } else if peak < 1e-4 {
        println!("[tap_probe] RESULT: SILENT — tap delivers frames but no signal.");
        std::process::exit(1);
    } else {
        println!("[tap_probe] RESULT: OK — capturing non-silent audio from {label}.");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tap_probe is macOS-only (Core Audio process tap).");
    std::process::exit(2);
}
