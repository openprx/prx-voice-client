//! Validation probe for the Core Audio process tap (macOS 14.4+).
//!
//! Proves that we can capture a given process's audio and that the stream is
//! non-silent (some apps can deliver silence through a tap). Run it against the
//! meeting app while audio is playing.
//!
//! Usage:
//!   1. Find the meeting app's PID, e.g.:  pgrep -x zoom.us   (or 'ps aux | grep -i zoom')
//!   2. cargo run -p prxvc-audio --example tap_probe -- <PID> [SECONDS]
//!
//! First run triggers the macOS audio-capture permission prompt. If denied or on
//! macOS < 14.4, start() fails with a clear error.

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    use prxvc_audio::core_audio_tap::{CoreAudioTap, frame_peak_amplitude};

    let mut args = std::env::args().skip(1);
    let pid: i32 = match args.next().and_then(|s| s.parse().ok()) {
        Some(p) => p,
        None => {
            eprintln!("usage: tap_probe <PID> [SECONDS]");
            eprintln!("  find PID with e.g.  pgrep -x zoom.us");
            std::process::exit(2);
        }
    };
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(5);

    println!("[tap_probe] starting tap on pid {pid} for {secs}s ...");
    let (_tap, rx) = match CoreAudioTap::start(pid) {
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
            Ok(frame) => {
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
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                println!("[tap_probe] ...no frames in last 500ms (is audio playing?)");
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                eprintln!("[tap_probe] capture channel disconnected");
                break;
            }
        }
    }

    println!("--------------------------------------------------");
    println!("[tap_probe] frames={frames} samples={samples} format={sample_rate}Hz/{channels}ch");
    println!("[tap_probe] peak amplitude = {peak:.4}");
    if frames == 0 {
        println!("[tap_probe] RESULT: NO FRAMES — tap not delivering (check pid / permission).");
        std::process::exit(1);
    } else if peak < 1e-4 {
        println!("[tap_probe] RESULT: SILENT — tap delivers frames but no signal.");
        std::process::exit(1);
    } else {
        println!("[tap_probe] RESULT: OK — capturing non-silent audio from pid {pid}. ✅");
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("tap_probe is macOS-only (Core Audio process tap).");
    std::process::exit(2);
}
