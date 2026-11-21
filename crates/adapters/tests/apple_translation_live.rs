//! Live check of system translation against Apple's Translation framework.
//!
//! Not a libtest binary: the framework delivers its work through the main dispatch queue, so the
//! main thread runs the run loop (`prxat_run_main_loop`) and the checks run on a second thread.
//! Ignored unless `--ignored` is passed:
//!
//! ```text
//! cargo test -p prxvc-adapters --features apple-translation --test apple_translation_live -- --ignored
//! ```
//!
//! Needs macOS 26 and the Chinese (Simplified) and English packs (System Settings, General,
//! Language and Region, Translation Languages). Without them it reports what is missing and
//! passes: a missing pack is the typed problem the product shows, not a test failure. Nothing is
//! played or recorded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use prxvc_adapters::mt::apple::ffi::AppleFramework;
use prxvc_adapters::mt::apple::{
    AppleSystemMt, Availability, SystemTranslationApi, SystemTranslationConfig,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtRequest, Translator};

fn request(text: &str, source: Lang, target: Lang) -> MtRequest {
    MtRequest {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
        source,
        target,
        text: text.to_owned(),
        context: Vec::new(),
        glossary: Vec::new(),
        strict_language: false,
    }
}

fn translate(mt: &AppleSystemMt, text: &str, source: Lang, target: Lang) -> (String, Duration) {
    let started = Instant::now();
    let mut stream = mt.translate(request(text, source, target)).expect("starts");
    let cancel = CancellationToken::new();
    let mut out = String::new();
    while let Some(delta) = stream.next(&cancel) {
        match delta.expect("no failure") {
            MtDelta::Text(t) => out.push_str(&t),
            MtDelta::Done { .. } => {}
        }
    }
    (out, started.elapsed())
}

fn checks() {
    let api = AppleFramework;
    if let Err(e) = api.os_support() {
        println!("skipped: {e}");
        return;
    }
    for (s, t) in [("zh-Hans", "en"), ("en", "zh-Hans")] {
        if api.availability(s, t).expect("availability") != Availability::Installed {
            println!("skipped: the {s} to {t} pack is not installed");
            return;
        }
    }
    // A pair that does not exist is a typed problem, not a hang.
    match api.availability("xx", "en") {
        Ok(Availability::Unsupported) => {}
        other => panic!("an invented language must be unsupported, got {other:?}"),
    }

    let zh_en = AppleSystemMt::open(&api, SystemTranslationConfig::new(Lang::Zh, Lang::En))
        .expect("opens zh to en");
    let en_zh = AppleSystemMt::open(&api, SystemTranslationConfig::new(Lang::En, Lang::Zh))
        .expect("opens en to zh");
    let warm = zh_en.warm_up().expect("warms up");
    en_zh.warm_up().expect("warms up");
    println!("warm-up zh to en: {} ms", warm.as_millis());

    let (text, took) = translate(
        &zh_en,
        "你好，今天的会议改到下午三点十五分。",
        Lang::Zh,
        Lang::En,
    );
    println!("zh to en: {text} ({} ms)", took.as_millis());
    assert!(text.contains("3:15"), "{text}");
    assert!(took < Duration::from_secs(2), "{took:?}");

    let (text, took) = translate(
        &en_zh,
        "The budget is 3.5 million, not 5.3 million.",
        Lang::En,
        Lang::Zh,
    );
    println!("en to zh: {text} ({} ms)", took.as_millis());
    assert!(text.contains("350") || text.contains("3.5"), "{text}");
    assert!(took < Duration::from_secs(2), "{took:?}");

    // A cancelled request returns promptly and the engine keeps working.
    let mut stream = zh_en
        .translate(request("我觉得这个方案不行。", Lang::Zh, Lang::En))
        .unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(stream.next(&cancel).expect("an item").is_err());
    drop(stream);
    let (text, _) = translate(&zh_en, "你能听到我说话吗？", Lang::Zh, Lang::En);
    assert!(!text.trim().is_empty());

    // A 1 ms budget times out as a typed timeout, then the session recovers.
    let mut cfg = SystemTranslationConfig::new(Lang::Zh, Lang::En);
    cfg.timeouts.translate = Duration::from_millis(1);
    let tight = AppleSystemMt::open(&api, cfg).expect("opens");
    let mut stream = tight
        .translate(request("这个项目的预算是三百五十万。", Lang::Zh, Lang::En))
        .unwrap();
    match stream.next(&CancellationToken::new()) {
        Some(Err(e)) if e.is_timeout() => println!("1 ms budget: timed out as expected"),
        Some(Ok(_)) => println!("1 ms budget: the framework answered in time"),
        other => panic!("unexpected {other:?}"),
    }
    println!("live system translation checks passed");
}

fn main() {
    if !std::env::args().any(|a| a == "--ignored") {
        println!("apple_translation_live: ignored by default (pass --ignored to run it)");
        return;
    }
    static LOOP_EXITED: AtomicBool = AtomicBool::new(false);
    let worker = std::thread::spawn(|| {
        let result = std::panic::catch_unwind(checks);
        // Stop the main thread's run loop; repeated, because a stop that arrives before the loop
        // has started is forgotten.
        while !LOOP_EXITED.load(Ordering::Acquire) {
            // SAFETY: `prxat_stop_main_loop` may be called from any thread at any time.
            unsafe { prxvc_apple_translation_sys::prxat_stop_main_loop() };
            std::thread::sleep(Duration::from_millis(10));
        }
        result
    });
    // SAFETY: called on the main thread; returns when the worker stops the loop.
    unsafe { prxvc_apple_translation_sys::prxat_run_main_loop() };
    LOOP_EXITED.store(true, Ordering::Release);
    if worker.join().expect("worker thread").is_err() {
        std::process::exit(1);
    }
}
