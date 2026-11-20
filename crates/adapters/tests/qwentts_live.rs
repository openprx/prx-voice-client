//! Live checks of `QwenTts` with the real runtime on Metal (CT-10, voice-cloning work package C3).
//! Every test is `#[ignore]`d: it needs the models and the GPU, and nothing is played.
//!
//! ```text
//! cargo test -p prxvc-adapters --features qwentts --release --test qwentts_live \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! Environment: `PRXVC_BENCH_ROOT` is the workspace `bench` directory (default
//! `/Users/nn/worker/voice/bench`); the models are read in place from `<root>/tts/models`
//! (`qwen-talker-1.7b-base-Q8_0.gguf`, `qwen-tokenizer-12hz-Q8_0.gguf`) and the owner's references
//! from `<root>/tts/refs` (`zh_owner.wav`, `en_owner.wav`, and `en_ref.wav` as the preset's stand-in
//! reference). Synthesised audio goes to WAV files under the system temp directory
//! (`prxvc-qwentts-live-<pid>`), never to a speaker.
#![cfg(feature = "qwentts")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use prxvc_adapters::tts::qwentts::{
    QwenTts, QwenTtsConfig, VoiceCache, cap, hash_model_files, load_reference_wav,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{
    CancellationToken, SpeechSynth, TtsError, TtsItem, TtsRequest, TtsStream, VoiceSelection,
};

const EN: &str = "The meeting has been moved to Thursday afternoon, so please update your calendar before the call.";
const ZH: &str = "\u{4f1a}\u{8bae}\u{6539}\u{5230}\u{4e86}\u{661f}\u{671f}\u{56db}\u{4e0b}\u{5348}\u{ff0c}\u{8bf7}\u{5728}\u{901a}\u{8bdd}\u{4e4b}\u{524d}\u{66f4}\u{65b0}\u{4f60}\u{7684}\u{65e5}\u{5386}\u{3002}";

fn bench_root() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT").map_or_else(
        || PathBuf::from("/Users/nn/worker/voice/bench"),
        PathBuf::from,
    )
}

fn models() -> (PathBuf, PathBuf) {
    let dir = bench_root().join("tts/models");
    (
        dir.join("qwen-talker-1.7b-base-Q8_0.gguf"),
        dir.join("qwen-tokenizer-12hz-Q8_0.gguf"),
    )
}

fn model_hash() -> &'static str {
    static HASH: OnceLock<String> = OnceLock::new();
    HASH.get_or_init(|| {
        let (talker, codec) = models();
        let t0 = Instant::now();
        let h = hash_model_files(&talker, &codec).expect("hash the model files");
        println!(
            "model hash {} ({:.1} s)",
            &h[..16],
            t0.elapsed().as_secs_f64()
        );
        h
    })
}

fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("prxvc-qwentts-live-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("create the output directory");
    dir
}

fn write_wav(path: &Path, samples: &[f32]) {
    let data: Vec<u8> = samples
        .iter()
        .flat_map(|s| ((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())
        .collect();
    let mut b = Vec::new();
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&24_000u32.to_le_bytes());
    b.extend_from_slice(&48_000u32.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&(data.len() as u32).to_le_bytes());
    b.extend_from_slice(&data);
    std::fs::write(path, b).expect("write the wav file");
}

struct Loaded {
    tts: QwenTts,
    cache_dir: tempfile::TempDir,
}

fn load(workers: usize, tweak: impl FnOnce(&mut QwenTtsConfig)) -> Loaded {
    let (talker, codec) = models();
    let mut cfg = QwenTtsConfig::new("qwen3-tts-1.7b-base-q8", model_hash());
    cfg.workers = workers;
    tweak(&mut cfg);
    let t0 = Instant::now();
    let tts = QwenTts::load_native(&talker, &codec, cfg).expect("load the engine");
    println!(
        "loaded 1.7B Q8_0 with {workers} worker(s) in {:.0} ms, of which the warm-up took {:.0} ms",
        t0.elapsed().as_secs_f64() * 1000.0,
        tts.warm_up_duration().as_secs_f64() * 1000.0
    );
    Loaded {
        tts,
        cache_dir: tempfile::tempdir().expect("temp dir"),
    }
}

impl Loaded {
    /// Prepare the owner's two references (through the cache, twice to show the hit) and one preset.
    fn voices(&self) {
        let refs = bench_root().join("tts/refs");
        for (id, file, lang) in [
            ("owner-zh", "zh_owner.wav", Lang::Zh),
            ("owner-en", "en_owner.wav", Lang::En),
        ] {
            let reference =
                load_reference_wav(&refs.join(file), id, lang, None).expect("reference");
            let cache = VoiceCache::new(self.cache_dir.path().join(id));
            let t0 = Instant::now();
            let (_, hit) = self
                .tts
                .prepare_voice_cached(&cache, &reference)
                .expect("prepare");
            let first = t0.elapsed();
            let t1 = Instant::now();
            let (_, hit2) = self
                .tts
                .prepare_voice_cached(&cache, &reference)
                .expect("prepare");
            println!(
                "voice {id}: prepared in {:.0} ms (hit: {hit}), again from the cache in {:.1} ms (hit: {hit2})",
                first.as_secs_f64() * 1000.0,
                t1.elapsed().as_secs_f64() * 1000.0
            );
            assert!(!hit && hit2);
        }
        let reference =
            load_reference_wav(&refs.join("en_ref.wav"), "x", Lang::En, None).expect("reference");
        let cache = VoiceCache::new(self.cache_dir.path().join("preset-remote"));
        self.tts
            .register_preset("remote", &reference, Some(&cache))
            .expect("preset");
    }

    fn open(
        &self,
        direction: Direction,
        lang: Lang,
        text: &str,
        voice: &str,
    ) -> Box<dyn TtsStream> {
        let voice = match voice {
            "remote" => VoiceSelection::Preset {
                name: "remote".to_owned(),
            },
            id => VoiceSelection::Cloned {
                profile_id: id.to_owned(),
            },
        };
        self.tts
            .synthesize(TtsRequest {
                clause: ClauseId::new(SegmentId::first(direction), 0),
                text: text.to_owned(),
                lang,
                voice,
            })
            .expect("open the stream")
    }
}

#[derive(Default)]
struct Run {
    pcm: Vec<f32>,
    /// Arrival time of each chunk after the request was opened.
    arrivals: Vec<Duration>,
    end: Duration,
    done: Option<f32>,
    error: Option<TtsError>,
}

impl Run {
    fn first_ms(&self) -> f64 {
        self.arrivals
            .first()
            .map_or(f64::NAN, |d| d.as_secs_f64() * 1000.0)
    }
    fn audio_secs(&self) -> f64 {
        self.pcm.len() as f64 / 24_000.0
    }
    fn rtf(&self) -> f64 {
        self.arrivals.last().map_or(f64::NAN, Duration::as_secs_f64) / self.audio_secs().max(1e-9)
    }
}

fn run(mut stream: Box<dyn TtsStream>, opened: Instant, token: &CancellationToken) -> Run {
    let mut r = Run::default();
    while let Some(item) = stream.next(token) {
        match item {
            Ok(TtsItem::Chunk(c)) => {
                assert_eq!(c.rate.hz(), 24_000, "the rate rides on every chunk");
                r.arrivals.push(opened.elapsed());
                r.pcm.extend_from_slice(&c.samples);
            }
            Ok(TtsItem::Done { audio_secs }) => r.done = Some(audio_secs),
            Err(e) => r.error = Some(e),
        }
    }
    r.end = opened.elapsed();
    r
}

fn check_plausible(r: &Run, text: &str, lang: Lang) {
    assert!(r.error.is_none(), "{:?}", r.error);
    let expected = cap::expected_secs(text, lang);
    let got = r.audio_secs();
    assert!(
        got > expected * 0.5 && got < expected * 2.0,
        "audio of {got:.2} s for an expected {expected:.2} s"
    );
    assert!(r.pcm.iter().all(|v| v.is_finite()));
    let peak = r.pcm.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    assert!(peak > 0.05, "silent output (peak {peak})");
    let reported = r.done.expect("Done");
    assert!((f64::from(reported) - got).abs() < 0.01);
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_synthesizes_english_and_mandarin_in_the_owner_voice() {
    let l = load(2, |_| {});
    l.voices();
    let dir = out_dir();
    let never = CancellationToken::new();
    for (name, lang, text, voice) in [
        ("en_owner", Lang::En, EN, "owner-en"),
        ("zh_owner", Lang::Zh, ZH, "owner-zh"),
        ("en_preset_remote", Lang::En, EN, "remote"),
    ] {
        let opened = Instant::now();
        let r = run(
            l.open(Direction::Outgoing, lang, text, voice),
            opened,
            &never,
        );
        check_plausible(&r, text, lang);
        println!(
            "{name}: {:.2} s of audio (expected {:.2} s), first chunk {:.0} ms, {} chunks, RTF {:.2}, wav {}",
            r.audio_secs(),
            cap::expected_secs(text, lang),
            r.first_ms(),
            r.arrivals.len(),
            r.rtf(),
            dir.join(format!("{name}.wav")).display()
        );
        write_wav(&dir.join(format!("{name}.wav")), &r.pcm);
    }
    println!("stats {:?}", l.tts.stats());
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_first_chunk_latency_and_real_time_factor() {
    let l = load(2, |_| {});
    l.voices();
    let never = CancellationToken::new();
    let mut first = Vec::new();
    let mut rtf = Vec::new();
    for i in 0..8 {
        let opened = Instant::now();
        let r = run(
            l.open(Direction::Outgoing, Lang::En, EN, "owner-en"),
            opened,
            &never,
        );
        check_plausible(&r, EN, Lang::En);
        println!(
            "solo run {i}: first chunk {:.1} ms, RTF {:.3}",
            r.first_ms(),
            r.rtf()
        );
        first.push(r.first_ms());
        rtf.push(r.rtf());
    }
    // The first request after load must already be warm (voice-cloning C3: below 100 ms).
    assert!(
        first[0] < 100.0,
        "first request after load took {:.1} ms to its first chunk",
        first[0]
    );
    let median = |v: &mut Vec<f64>| {
        v.sort_by(f64::total_cmp);
        v[v.len() / 2]
    };
    println!(
        "TTFA median {:.1} ms, max {:.1} ms; RTF median {:.3}, max {:.3}",
        median(&mut first),
        first.iter().copied().fold(0.0, f64::max),
        median(&mut rtf),
        rtf.iter().copied().fold(0.0, f64::max)
    );
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_two_concurrent_clauses_are_batched_and_both_complete() {
    let l = load(2, |_| {});
    l.voices();
    let opened = Instant::now();
    let a = l.open(Direction::Outgoing, Lang::En, EN, "owner-en");
    let b = l.open(Direction::Incoming, Lang::Zh, ZH, "remote");
    let (ra, rb) = std::thread::scope(|s| {
        let ta = s.spawn(|| run(a, opened, &CancellationToken::new()));
        let tb = s.spawn(|| run(b, opened, &CancellationToken::new()));
        (ta.join().expect("a"), tb.join().expect("b"))
    });
    check_plausible(&ra, EN, Lang::En);
    // The preset voice speaks English only if its reference is English; the clause is Chinese text
    // read by the English stand-in reference, so only the length bound is checked for it.
    assert!(rb.error.is_none(), "{:?}", rb.error);
    assert!(rb.audio_secs() > 1.0);
    for (name, r) in [("outgoing", &ra), ("incoming", &rb)] {
        println!(
            "{name}: first chunk {:.0} ms, {:.2} s of audio, {} chunks, RTF {:.3}",
            r.first_ms(),
            r.audio_secs(),
            r.arrivals.len(),
            r.rtf()
        );
    }
    let overlap =
        ra.arrivals.first() < rb.arrivals.last() && rb.arrivals.first() < ra.arrivals.last();
    assert!(overlap, "the two streams' chunks interleave in time");
    let interleaved = ra.arrivals.iter().filter(|t| **t > rb.arrivals[0]).count() > 1
        && rb.arrivals.iter().filter(|t| **t > ra.arrivals[0]).count() > 1;
    assert!(interleaved, "chunks of both streams alternate");
    assert!(
        ra.rtf() < 0.8 && rb.rtf() < 0.8,
        "sustained RTF below 0.8 with two streams"
    );
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_cancel_mid_clause_ends_the_stream_fast_and_leaves_the_other_stream_alone() {
    let l = load(2, |_| {});
    l.voices();
    let long = format!("{EN} {EN} {EN}");
    let opened = Instant::now();
    let mut victim = l.open(Direction::Outgoing, Lang::En, &long, "owner-en");
    let bystander = l.open(Direction::Incoming, Lang::En, EN, "remote");
    let by = std::thread::spawn(move || run(bystander, opened, &CancellationToken::new()));

    let token = CancellationToken::new();
    let mut chunks = 0;
    while chunks < 3 {
        match victim.next(&token) {
            Some(Ok(TtsItem::Chunk(_))) => chunks += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    let t0 = Instant::now();
    token.cancel();
    let first = victim.next(&token);
    let to_cancelled = t0.elapsed();
    let second = victim.next(&token);
    let to_end = t0.elapsed();
    println!(
        "cancel: Cancelled after {:.2} ms, stream ended after {:.2} ms",
        to_cancelled.as_secs_f64() * 1000.0,
        to_end.as_secs_f64() * 1000.0
    );
    assert_eq!(first.map(|r| r.err()), Some(Some(TtsError::Cancelled)));
    assert!(second.is_none());
    assert!(
        to_end < Duration::from_millis(50),
        "cancel must end the stream within 50 ms"
    );
    let other = by.join().expect("bystander");
    check_plausible(&other, EN, Lang::En);
    println!(
        "bystander unaffected: {:.2} s of audio, RTF {:.3}",
        other.audio_secs(),
        other.rtf()
    );
    drop(victim);
    assert_eq!(l.tts.stats().cancelled, 1);
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_cancel_frees_the_engine_slot_quickly() {
    // One worker: the next request can only start once the cancelled call has really stopped.
    let l = load(1, |_| {});
    l.voices();
    let long = format!("{EN} {EN} {EN}");
    let mut victim = l.open(Direction::Outgoing, Lang::En, &long, "owner-en");
    let token = CancellationToken::new();
    for _ in 0..2 {
        assert!(matches!(victim.next(&token), Some(Ok(TtsItem::Chunk(_)))));
    }
    let t0 = Instant::now();
    token.cancel();
    assert!(matches!(
        victim.next(&token),
        Some(Err(TtsError::Cancelled))
    ));
    let next = l.open(Direction::Outgoing, Lang::En, "Hello there.", "owner-en");
    let r = run(next, t0, &CancellationToken::new());
    println!(
        "slot reuse: the next request's first chunk came {:.0} ms after the cancel (abort plus a fresh first chunk of about 40 ms)",
        r.first_ms()
    );
    assert!(r.error.is_none());
    assert!(r.first_ms() < 200.0);
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_a_tiny_cap_is_reported_as_a_runaway_and_the_engine_stays_usable() {
    let l = load(2, |c| c.cap_frames_override = Some(10));
    l.voices();
    let never = CancellationToken::new();
    let r = run(
        l.open(Direction::Outgoing, Lang::En, EN, "owner-en"),
        Instant::now(),
        &never,
    );
    println!(
        "tiny cap: {:?} after {:.2} s of audio, {} chunks",
        r.error,
        r.audio_secs(),
        r.arrivals.len()
    );
    let Some(TtsError::Runaway { produced_secs }) = r.error else {
        panic!("runaway expected, got {:?}", r.error)
    };
    assert!(
        produced_secs > 0.3 && produced_secs < 1.5,
        "10 frames are 0.8 s: {produced_secs}"
    );
    assert!(r.done.is_none(), "a runaway is never a success");
    assert_eq!(l.tts.stats().runaways, 1);
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn live_a_normal_clause_never_trips_the_cap() {
    let l = load(2, |_| {});
    l.voices();
    let never = CancellationToken::new();
    let mut worst = 0.0f64;
    for (lang, text, voice) in [
        (Lang::En, EN, "owner-en"),
        (Lang::Zh, ZH, "owner-zh"),
        (Lang::En, "OK.", "owner-en"),
        (Lang::Zh, "\u{597d}\u{7684}\u{3002}", "owner-zh"),
    ] {
        for _ in 0..3 {
            let r = run(
                l.open(Direction::Outgoing, lang, text, voice),
                Instant::now(),
                &never,
            );
            assert!(r.error.is_none(), "{text}: {:?}", r.error);
            let used =
                r.audio_secs() / (f64::from(cap::cap_frames(text, lang)) / cap::FRAMES_PER_SEC);
            worst = worst.max(used);
        }
    }
    println!(
        "largest share of the cap used by a normal clause: {:.0} %",
        worst * 100.0
    );
    assert!(worst < 0.9);
    assert_eq!(l.tts.stats().runaways, 0);
}
