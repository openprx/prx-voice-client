//! Offline sweep of the stable-prefix parameters on real hypotheses (ignored, feature `transcribe`).
//!
//! Re-decodes growing prefixes of benchmark files at the 400 ms cadence (no real-time pacing, the
//! decodes are what a live run would produce) and replays the same hypothesis sequences through the
//! stable-prefix rule with different agreement counts and guards, counting repairs and the damage
//! they do to the final text.
//!
//! ```text
//! cargo test -p prxvc-adapters --features transcribe --release --lib asr::sweep_live \
//!     -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use prxvc_media::Lang;

use super::native::NativeEngine;
use super::seam::RecognizeEngine;
use super::stable::StablePrefix;
use crate::tts::qwentts::wav;
use prxvc_media::text_units::units;

fn bench() -> PathBuf {
    std::env::var_os("PRXVC_BENCH_ROOT").map_or_else(
        || PathBuf::from("/Users/nn/worker/voice/bench"),
        PathBuf::from,
    )
}

fn keys(text: &str) -> Vec<String> {
    units(text).into_iter().map(|u| u.key).collect()
}

fn distance(a: &[String], b: &[String]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for (i, x) in a.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, y) in b.iter().enumerate() {
            cur.push(
                (prev[j] + usize::from(x != y))
                    .min(prev[j + 1] + 1)
                    .min(cur[j] + 1),
            );
        }
        prev = cur;
    }
    prev[b.len()]
}

struct Case {
    lang: Lang,
    reference: String,
    /// Hypotheses at 400 ms steps (from 480 ms), with the audio seconds of each.
    partials: Vec<(f64, String)>,
    fin: String,
    secs: f64,
}

#[test]
#[ignore = "needs the models"]
fn sweep_the_stable_prefix_parameters() {
    let mut engine = NativeEngine::load(
        &bench().join("asr/models/hf/Qwen3-ASR-0.6B-gguf/Qwen3-ASR-0.6B-Q8_0.gguf"),
    )
    .expect("load");
    let manifest =
        std::fs::read_to_string(bench().join("asr/data/manifest.tsv")).expect("manifest");
    let mut cases = Vec::new();
    for (subset, lang) in [("zh_read", Lang::Zh), ("en_read", Lang::En)] {
        for line in manifest.lines().skip(1) {
            let f: Vec<&str> = line.split('\t').collect();
            if f.len() < 5
                || f[1] != subset
                || cases.iter().filter(|c: &&Case| c.lang == lang).count() >= 30
            {
                continue;
            }
            let audio = wav::read(&bench().join("asr").join(f[3]))
                .expect("wav")
                .samples;
            let mut partials = Vec::new();
            let mut at = 7680;
            while at < audio.len() {
                let text = engine
                    .decode(&audio[..at], lang, &|| false)
                    .expect("decode");
                partials.push((at as f64 / 16000.0, text));
                at += 6400;
            }
            let fin = engine.decode(&audio, lang, &|| false).expect("decode");
            cases.push(Case {
                lang,
                reference: f[4].to_owned(),
                partials,
                fin,
                secs: audio.len() as f64 / 16000.0,
            });
        }
    }
    println!("{} utterances decoded", cases.len());
    println!(
        "lang  N guard | repaired-partials/total | final repaired | error plain -> after rule | stable50 at (s before end, median)"
    );
    for lang in [Lang::Zh, Lang::En] {
        for n in [2usize, 3, 4, 5] {
            for guard in [0usize, 2, 4, 6, 8] {
                let (mut rep, mut total, mut frep) = (0, 0, 0);
                let (mut plain, mut after, mut refn) = (0, 0, 0);
                let mut early = Vec::new();
                for c in cases.iter().filter(|c| c.lang == lang) {
                    let mut s = StablePrefix::new(n, guard);
                    let fin_keys = keys(&c.fin);
                    let mut reached = None;
                    for (t, h) in &c.partials {
                        let r = s.push(h);
                        total += 1;
                        rep += usize::from(r.repaired);
                        let st = keys(&r.text[..r.stable_len]);
                        if reached.is_none()
                            && st.len() * 2 >= fin_keys.len().max(1)
                            && fin_keys.starts_with(&st)
                        {
                            reached = Some(c.secs - t);
                        }
                    }
                    let f = s.finish(&c.fin);
                    frep += usize::from(f.repaired);
                    let r = keys(&c.reference);
                    refn += r.len();
                    plain += distance(&r, &fin_keys);
                    after += distance(&r, &keys(&f.text));
                    early.extend(reached);
                }
                early.sort_by(|a, b| a.partial_cmp(b).unwrap());
                println!(
                    "{lang:?} {n} {guard:>2} | {rep:>3}/{total} | {frep:>2} | {:.2}% -> {:.2}% | {:.1}",
                    100.0 * plain as f64 / refn as f64,
                    100.0 * after as f64 / refn as f64,
                    early.get(early.len() / 2).copied().unwrap_or(f64::NAN)
                );
            }
        }
    }
}
