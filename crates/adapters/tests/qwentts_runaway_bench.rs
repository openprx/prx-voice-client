//! Runaway rate of the synthesiser per fragment length, silent (nothing is played). Ignored: it
//! needs the models and the GPU.
//!
//! ```text
//! PRXVC_RUNAWAY_MODE=natural|policy cargo test -p prxvc-adapters --features qwentts --release \
//!     --test qwentts_runaway_bench -- --ignored --nocapture --test-threads=1
//! ```
//!
//! * `natural`: every fragment gets a generous 8 s cap and no retry, so the audio length shows what
//!   the engine does with the text (a runaway runs to the cap).
//! * `policy`: the engine as the live call loads it (`prxvc-providers::tts_loader`): the cap derived
//!   from the text and the runaway policy of the build.
//!
//! The fragments are cut from plain English sentences at every word count from 1 to 10, the way
//! an early commit with a context-free engine cuts them: some start mid-sentence, some end on a
//! function word. The sentences are test data; the bench prints lengths and counts only.
#![cfg(feature = "qwentts")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::Instant;

use prxvc_adapters::tts::qwentts::{
    QwenTts, QwenTtsConfig, RunawayRetry, VoiceCache, cap, hash_model_files, load_reference_wav,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{
    CancellationToken, SpeechSynth, TtsItem, TtsRequest, TtsStream, VoiceSelection,
};

const SENTENCES: &[&str] = &[
    "I think we should move the meeting to Thursday afternoon because the team is travelling.",
    "Could you send me the report before the end of the day so that I can read it tonight.",
    "The project is going well but we still need two more weeks to finish the testing.",
    "Please tell them that I will be late and that they can start without me.",
    "We talked about the budget yesterday and everyone agreed that it is too small.",
    "If the weather is good tomorrow we will go to the lake and have lunch there.",
    "My brother called me last night and said that he wants to visit us next month.",
    "I am not sure whether the shop is open today, so let me check the website first.",
    "They have changed the schedule again and now the flight leaves earlier than before.",
    "Thank you very much for your help, I really appreciate everything you have done.",
    "The new phone is faster than the old one but the battery does not last as long.",
    "After dinner we walked along the river and talked about our plans for the summer.",
    "Do you remember the restaurant that we went to when your parents were in town.",
    "I will send you the address in a moment, and then you can book a taxi.",
    "The doctor said that I need to rest for a few days and drink a lot of water.",
    "We could not find the key anywhere, so in the end we called a locksmith.",
    "She has been working on this problem for a long time and she is close to a solution.",
    "Let us meet in front of the station at nine and then decide where to go.",
    "It was a long day, and all I want now is a quiet evening at home.",
    "He told me that the price will go up next week, so we should buy it now.",
];

struct Lcg(u64);
impl Lcg {
    fn below(&mut self, n: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % n as u64) as usize
    }
}

fn fragments(per_length: usize) -> Vec<(usize, String)> {
    let mut rng = Lcg(2026);
    let mut out = Vec::new();
    for words in 1..=10usize {
        for _ in 0..per_length {
            let sentence = loop {
                let s = SENTENCES[rng.below(SENTENCES.len())];
                if s.split_whitespace().count() >= words {
                    break s;
                }
            };
            let all: Vec<&str> = sentence.split_whitespace().collect();
            let start = rng.below(all.len() - words + 1);
            let mut text = all[start..start + words].join(" ");
            if start > 0 {
                text = text.to_lowercase();
            }
            let text = text.trim_end_matches(['.', ',']).to_owned();
            let text = match rng.below(3) {
                0 => format!("{text},"),
                1 => format!("{text}."),
                _ => text,
            };
            out.push((words, text));
        }
    }
    out
}

fn bench_root() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT").map_or_else(
        || PathBuf::from("/Users/nn/worker/voice/bench"),
        PathBuf::from,
    )
}

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn runaway_rate_by_fragment_length() {
    let natural = std::env::var("PRXVC_RUNAWAY_MODE").is_ok_and(|m| m == "natural");
    let per_length: usize = std::env::var("PRXVC_RUNAWAY_PER_LENGTH")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(32);
    let models = bench_root().join("tts/models");
    let talker = models.join("qwen-talker-1.7b-base-Q8_0.gguf");
    let codec = models.join("qwen-tokenizer-12hz-Q8_0.gguf");
    let hash = hash_model_files(&talker, &codec).expect("hash");
    let mut cfg = QwenTtsConfig::new("qwen3-tts-1.7b-base-q8", hash);
    cfg.workers = 1;
    if natural {
        cfg.cap_frames_override = Some(100);
    } else {
        cfg.runaway_retry = RunawayRetry::ShortOnce;
    }
    let tts = QwenTts::load_native(&talker, &codec, cfg).expect("load");
    let reference = load_reference_wav(
        &bench_root().join("tts/refs/en_owner.wav"),
        "owner-en",
        Lang::En,
        None,
    )
    .expect("reference");
    let cache_dir = tempfile::tempdir().expect("temp dir");
    let cache = VoiceCache::new(cache_dir.path().join("owner-en"));
    tts.prepare_voice_cached(&cache, &reference).expect("prepare");

    let never = CancellationToken::new();
    // (words, runaway attempts, failed, audio secs, expected secs, cap secs, wall ms, first ms)
    let mut rows = Vec::new();
    for (words, text) in fragments(per_length) {
        let before = tts.stats();
        let opened = Instant::now();
        let mut stream = tts
            .synthesize(TtsRequest {
                clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
                text: text.clone(),
                lang: Lang::En,
                voice: VoiceSelection::Cloned {
                    profile_id: "owner-en".to_owned(),
                },
            })
            .expect("open");
        let (mut samples, mut failed, mut first) = (0usize, false, f64::NAN);
        while let Some(item) = stream.next(&never) {
            match item {
                Ok(TtsItem::Chunk(c)) => {
                    if first.is_nan() {
                        first = opened.elapsed().as_secs_f64() * 1000.0;
                    }
                    samples += c.samples.len();
                }
                Ok(TtsItem::Done { .. }) => {}
                Err(_) => failed = true,
            }
        }
        let wall = opened.elapsed().as_secs_f64() * 1000.0;
        let after = tts.stats();
        let cap_s = f64::from(cap::cap_frames(&text, Lang::En)) / cap::FRAMES_PER_SEC;
        rows.push((
            words,
            after.runaways - before.runaways,
            failed,
            samples as f64 / 24_000.0,
            cap::expected_secs(&text, Lang::En),
            cap_s,
            wall,
            first,
        ));
    }
    println!(
        "mode={} fragments={}",
        if natural { "natural(cap 8 s, no retry)" } else { "policy" },
        rows.len()
    );
    println!("words  n  runaway_attempts  failed  audio_s(mean)  audio_s(p95)  audio_s(max)  cap_s(mean)  audio/expected(mean,ok only)  wall_ms(mean)  wall_ms(max)  first_ms(mean)");
    let mut total = (0usize, 0u64, 0usize);
    for words in 1..=10 {
        let r: Vec<_> = rows.iter().filter(|r| r.0 == words).collect();
        let n = r.len();
        let runs: u64 = r.iter().map(|r| r.1).sum();
        let failed = r.iter().filter(|r| r.2).count();
        let ok: Vec<_> = r.iter().filter(|r| !r.2 && r.1 == 0).collect();
        let ratio = ok.iter().map(|r| r.3 / r.4.max(1e-9)).sum::<f64>() / ok.len().max(1) as f64;
        let mean = |f: fn(&&(usize, u64, bool, f64, f64, f64, f64, f64)) -> f64| {
            r.iter().map(f).sum::<f64>() / n as f64
        };
        let maxwall = r.iter().map(|r| r.6).fold(0.0, f64::max);
        let mut lens: Vec<f64> = r.iter().filter(|r| !r.2 && r.1 == 0).map(|r| r.3).collect();
        lens.sort_by(f64::total_cmp);
        let p95 = lens.get((lens.len() * 95 / 100).min(lens.len().saturating_sub(1))).copied().unwrap_or(0.0);
        let max = lens.last().copied().unwrap_or(0.0);
        println!(
            "{words:>5} {n:>3} {runs:>17} {failed:>7} {:>14.2} {p95:>13.2} {max:>13.2} {:>12.2} {ratio:>29.2} {:>14.0} {maxwall:>13.0} {:>15.0}",
            mean(|r| r.3),
            mean(|r| r.5),
            mean(|r| r.6),
            mean(|r| r.7)
        );
        total = (total.0 + n, total.1 + runs, total.2 + failed);
    }
    println!(
        "total fragments {} runaway attempts {} ({:.1} %) failed fragments {} ({:.1} %)",
        total.0,
        total.1,
        100.0 * total.1 as f64 / total.0 as f64,
        total.2,
        100.0 * total.2 as f64 / total.0 as f64
    );
    let longest_ok = rows
        .iter()
        .filter(|r| !r.2 && r.1 == 0)
        .map(|r| r.3 / r.4.max(1e-9))
        .fold(0.0, f64::max);
    println!("largest audio/expected among good fragments: {longest_ok:.2}");
}

/// Short and odd texts, the kind a context-free engine returns for a cut fragment, by class.
const ODD: &[(&str, &[&str])] = &[
    ("lone function word", &["the", "a", "and", "of", "to", "that", "in", "so"]),
    ("function word pair", &["and the", "of the", "to be", "in a", "that the", "so we"]),
    ("interjection", &["Mm.", "Uh,", "Ah.", "Oh,", "Hmm.", "Yeah.", "Well,", "Okay."]),
    ("punctuation led", &[", and the", ". Then", "- the", ", so", "... and then", "; however"]),
    ("number or letter", &["3", "2026", "10:30", "I", "A", "OK", "No. 5", "50%"]),
    ("proper noun", &["WhatsApp", "Thursday", "Beijing", "Mr. Chen", "iPhone", "Zhang Wei"]),
    ("two or three plain words", &["I think so.", "Thank you.", "See you later.", "It is good.", "We can go."]),
    ("four plain words", &["We should go there.", "I do not know.", "Please send it now.", "That is what happened."]),
    ("cjk left untranslated", &["\u{55ef}", "\u{7136}\u{540e}", "\u{6240}\u{4ee5}", "\u{6211}\u{89c9}\u{5f97}"]),
    ("quoted or bracketed", &["\"Yes.\"", "(laughs)", "\"the end\"", "[music]"]),
    ("repetition", &["no, no, no", "yes yes yes", "the the", "very very very good"]),
];

#[test]
#[ignore = "needs the TTS models and the GPU"]
fn runaway_rate_by_class_of_short_text() {
    let tries: usize = std::env::var("PRXVC_RUNAWAY_TRIES").ok().and_then(|v| v.parse().ok()).unwrap_or(6);
    let models = bench_root().join("tts/models");
    let talker = models.join("qwen-talker-1.7b-base-Q8_0.gguf");
    let codec = models.join("qwen-tokenizer-12hz-Q8_0.gguf");
    let hash = hash_model_files(&talker, &codec).expect("hash");
    let mut cfg = QwenTtsConfig::new("qwen3-tts-1.7b-base-q8", hash);
    cfg.workers = 1;
    cfg.cap_frames_override = Some(60);
    let tts = QwenTts::load_native(&talker, &codec, cfg).expect("load");
    let reference = load_reference_wav(&bench_root().join("tts/refs/en_owner.wav"), "owner-en", Lang::En, None).expect("reference");
    let cache_dir = tempfile::tempdir().expect("temp dir");
    let cache = VoiceCache::new(cache_dir.path().join("owner-en"));
    tts.prepare_voice_cached(&cache, &reference).expect("prepare");
    let never = CancellationToken::new();
    println!("class  texts  tries  runaways  mean_audio_s");
    for (class, texts) in ODD {
        let (mut n, mut runs, mut secs) = (0usize, 0u64, 0.0f64);
        for text in *texts {
            for _ in 0..tries {
                let before = tts.stats();
                let mut stream = tts
                    .synthesize(TtsRequest {
                        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
                        text: (*text).to_owned(),
                        lang: Lang::En,
                        voice: VoiceSelection::Cloned { profile_id: "owner-en".to_owned() },
                    })
                    .expect("open");
                let mut samples = 0usize;
                while let Some(item) = stream.next(&never) {
                    if let Ok(TtsItem::Chunk(c)) = item {
                        samples += c.samples.len();
                    }
                }
                n += 1;
                runs += tts.stats().runaways - before.runaways;
                secs += samples as f64 / 24_000.0;
            }
        }
        println!("{class:<28} {:>3} {n:>5} {runs:>8} {:>8.2}", texts.len(), secs / n as f64);
    }
}

/// The runaway rate of the texts in `PRXVC_RUNAWAY_TEXTS` (separated by `|`), `PRXVC_RUNAWAY_TRIES`
/// attempts each, with a cap of 8 s and no retry: what a given text does, seed after seed.
#[test]
#[ignore = "needs the TTS models and the GPU"]
fn runaway_rate_of_given_texts() {
    let texts = std::env::var("PRXVC_RUNAWAY_TEXTS").unwrap_or_default();
    let tries: usize = std::env::var("PRXVC_RUNAWAY_TRIES").ok().and_then(|v| v.parse().ok()).unwrap_or(8);
    let models = bench_root().join("tts/models");
    let talker = models.join("qwen-talker-1.7b-base-Q8_0.gguf");
    let codec = models.join("qwen-tokenizer-12hz-Q8_0.gguf");
    let hash = hash_model_files(&talker, &codec).expect("hash");
    let mut cfg = QwenTtsConfig::new("qwen3-tts-1.7b-base-q8", hash);
    cfg.workers = 1;
    cfg.cap_frames_override = Some(60);
    let tts = QwenTts::load_native(&talker, &codec, cfg).expect("load");
    let reference = load_reference_wav(&bench_root().join("tts/refs/en_owner.wav"), "owner-en", Lang::En, None).expect("reference");
    let cache_dir = tempfile::tempdir().expect("temp dir");
    let cache = VoiceCache::new(cache_dir.path().join("owner-en"));
    tts.prepare_voice_cached(&cache, &reference).expect("prepare");
    let never = CancellationToken::new();
    println!("text  tries  runaways  mean_audio_s");
    for text in texts.split('|').filter(|t| !t.is_empty()) {
        let (mut runs, mut secs) = (0u64, 0.0f64);
        for _ in 0..tries {
            let before = tts.stats();
            let mut stream = tts
                .synthesize(TtsRequest {
                    clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
                    text: text.to_owned(),
                    lang: Lang::En,
                    voice: VoiceSelection::Cloned { profile_id: "owner-en".to_owned() },
                })
                .expect("open");
            let mut samples = 0usize;
            while let Some(item) = stream.next(&never) {
                if let Ok(TtsItem::Chunk(c)) = item {
                    samples += c.samples.len();
                }
            }
            runs += tts.stats().runaways - before.runaways;
            secs += samples as f64 / 24_000.0;
        }
        println!("{text:<48} {tries:>5} {runs:>8} {:>8.2}", secs / tries as f64);
    }
}
