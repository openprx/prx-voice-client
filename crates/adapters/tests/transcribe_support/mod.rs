//! Shared helpers of the live `transcribe` tests: benchmark files, text normalisation, error
//! rates and a real-time runner for one utterance.

#![allow(dead_code, reason = "each test binary uses a different part")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use prxvc_adapters::tts::qwentts::wav;

pub mod runner;

pub fn bench_root() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT").map_or_else(
        || PathBuf::from("/Users/nn/worker/voice/bench"),
        PathBuf::from,
    )
}

pub fn asr_model() -> PathBuf {
    bench_root().join("asr/models/hf/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf")
}

pub fn silero_model() -> PathBuf {
    bench_root().join("asr/models/hf/silero-vad-v6.2-gguf/silero-vad-v6.2-F32.gguf")
}

/// One row of the benchmark's manifest.
#[derive(Clone, Debug)]
pub struct Row {
    pub id: String,
    pub subset: String,
    pub lang: String,
    pub wav: String,
    pub reference: String,
}

pub fn manifest() -> Vec<Row> {
    let text =
        std::fs::read_to_string(bench_root().join("asr/data/manifest.tsv")).expect("manifest");
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5).then(|| Row {
                id: f[0].to_owned(),
                subset: f[1].to_owned(),
                lang: f[2].to_owned(),
                wav: f[3].to_owned(),
                reference: f[4].to_owned(),
            })
        })
        .collect()
}

pub fn rows(subset: &str) -> Vec<Row> {
    manifest()
        .into_iter()
        .filter(|r| r.subset == subset)
        .collect()
}

/// The 16 kHz mono samples of a row.
pub fn audio(row: &Row) -> Vec<f32> {
    let w = wav::read(&bench_root().join("asr").join(&row.wav)).expect("wav");
    assert_eq!(w.sample_rate, 16_000, "{}", row.id);
    w.samples
}

/// The oracle's `[first start, last end]` in seconds (energy based) of a row.
pub fn oracle(id: &str) -> Option<(f64, f64)> {
    let text = std::fs::read_to_string(bench_root().join("asr/data/oracle.json")).ok()?;
    let at = text.find(&format!("\"{id}\""))?;
    let rest = &text[at + id.len() + 2..];
    let open = rest.find('[')?;
    let close = rest.find(']')?;
    let mut it = rest[open + 1..close]
        .split(',')
        .map(|v| v.trim().parse::<f64>());
    Some((it.next()?.ok()?, it.next()?.ok()?))
}

fn is_cjk(c: char) -> bool {
    matches!(u32::from(c), 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

/// Scoring tokens: one per Chinese character and one per run of Latin letters and digits, case
/// folded, punctuation and spacing dropped (the benchmark's rule without its number and
/// traditional-character conversions).
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if is_cjk(c) {
            if !word.is_empty() {
                out.push(std::mem::take(&mut word));
            }
            out.push(c.to_string());
        } else if c.is_ascii_alphanumeric() || (c == '\'' && !word.is_empty()) {
            word.extend(c.to_lowercase());
        } else if !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
    }
    out
}

fn edit_distance(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(x != y);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// `(errors, reference length)` of `hyp` against `reference`.
pub fn errors(reference: &str, hyp: &str) -> (usize, usize) {
    let (r, h) = (tokens(reference), tokens(hyp));
    (edit_distance(&r, &h), r.len())
}

pub fn median(mut v: Vec<f64>) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

pub fn percentile(mut v: Vec<f64>, p: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[(((v.len() - 1) as f64) * p).round() as usize]
}

/// Sleep until `at`.
pub fn sleep_until(at: Instant) {
    let now = Instant::now();
    if at > now {
        std::thread::sleep(at - now);
    }
}

pub fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Append `id`, language, reference and hypothesis as one TSV line to the file named by the
/// environment variable `var`, when it is set: the lines can be scored with the benchmark's own
/// normaliser (bench/asr `common.error_counts`).
pub fn dump(var: &str, id: &str, lang: &str, reference: &str, hyp: &str) {
    use std::io::Write;
    if let Some(path) = std::env::var_os(var) {
        let clean = |s: &str| s.replace(['\t', '\n'], " ");
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("dump file");
        writeln!(f, "{id}\t{lang}\t{}\t{}", clean(reference), clean(hyp)).expect("dump");
    }
}
