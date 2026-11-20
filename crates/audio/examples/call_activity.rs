//! Read-only probe of one application's audio activity and the call judgement (pipeline-design PD-4.1.2).
//!
//! Usage:
//!   cargo run -p prxvc-audio --example call_activity -- <bundle.id> [--watch] [--secs N] [--uid MIC_UID]
//!
//! Prints whether the application is known to the audio system, whether its
//! input and output run, the devices it uses, and the judgement: a call active
//! on our virtual microphone, active on another microphone, idle or not running.
//! With `--watch` it polls once per second and prints each debounced change
//! until interrupted (or for `--secs N` seconds). `--uid` is the UID of our
//! virtual microphone (default `org.openprx.prx-voice-client.microphone`).
//!
//! It only reads Core Audio's process list: it launches, quits and touches no
//! application, creates no tap, records and plays no audio and asks for no
//! permission.

#[cfg(target_os = "macos")]
fn main() {
    use std::time::{Duration, Instant};

    use prxvc_audio::call_activity::native::{NativeActivitySource, read_app_activity};
    use prxvc_audio::call_activity::{
        AppAudioActivity, CallActivityEvent, CallActivityWatcher, CallJudgement, WatcherConfig, judge_call,
    };

    const DEFAULT_UID: &str = "org.openprx.prx-voice-client.microphone";

    fn print_activity(a: &AppAudioActivity, judgement: &CallJudgement) {
        println!("application     : {}", a.bundle_id);
        println!("known to audio  : {}", a.present);
        println!("pid             : {}", a.pid.map_or("-".to_owned(), |p| p.to_string()));
        println!("audio running   : {}", a.audio_running);
        println!("input running   : {}", a.input_running);
        println!("output running  : {}", a.output_running);
        for d in &a.input_devices {
            println!("  input device  : \"{}\" uid={} (in {} ch, out {} ch)", d.name, d.uid, d.input_channels, d.output_channels);
        }
        for d in &a.output_devices {
            println!("  output device : \"{}\" uid={} (in {} ch, out {} ch)", d.name, d.uid, d.input_channels, d.output_channels);
        }
        println!("judgement       : {judgement:?}");
        println!("call active     : {}", judgement.call_active());
        println!("uses our mic    : {}", judgement.uses_our_microphone());
    }

    let mut bundle = None;
    let mut watch = false;
    let mut secs: Option<u64> = None;
    let mut uid = DEFAULT_UID.to_owned();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--watch" => watch = true,
            "--secs" => secs = args.next().and_then(|s| s.parse().ok()),
            "--uid" => uid = args.next().unwrap_or_else(|| DEFAULT_UID.to_owned()),
            _ => bundle = Some(arg),
        }
    }
    let Some(bundle) = bundle else {
        eprintln!("usage: call_activity <bundle.id> [--watch] [--secs N] [--uid MIC_UID]");
        std::process::exit(2);
    };

    let activity = match read_app_activity(&bundle) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("call_activity: {e}");
            std::process::exit(1);
        }
    };
    print_activity(&activity, &judge_call(&activity, &uid));
    if !watch {
        return;
    }

    println!("\nwatching (one poll per second, one second of debounce); Ctrl-C to stop");
    let source = Box::new(NativeActivitySource::new(bundle));
    let watcher = match CallActivityWatcher::spawn(source, WatcherConfig::new(uid)) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("call_activity: cannot start the watcher: {e}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    loop {
        if secs.is_some_and(|s| started.elapsed() >= Duration::from_secs(s)) {
            break;
        }
        match watcher.events().recv_timeout(Duration::from_millis(250)) {
            Ok(CallActivityEvent::Changed { judgement, activity }) => {
                println!("\n[{:>5.1} s] changed", started.elapsed().as_secs_f32());
                print_activity(&activity, &judgement);
            }
            Ok(CallActivityEvent::Unavailable { error }) => {
                println!("\n[{:>5.1} s] detection unavailable: {error}", started.elapsed().as_secs_f32());
            }
            Err(_) => {}
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("call_activity: needs macOS 14.4 or later");
}
