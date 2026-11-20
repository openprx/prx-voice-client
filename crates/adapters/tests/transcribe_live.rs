//! Live checks of the VAD and the recogniser with the real `transcribe.cpp` runtime (work packages
//! CT-11a and CT-11b). Every test is `#[ignore]`d: it needs the models; nothing is played or recorded.
//!
//! ```text
//! cargo test -p prxvc-adapters --features transcribe --release --test transcribe_live \
//!     -- --ignored --nocapture --test-threads=1
//! ```
//!
//! `PRXVC_BENCH_ROOT` is the workspace `bench` directory (default `/Users/nn/worker/voice/bench`);
//! models and audio are read in place.
#![cfg(feature = "transcribe")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod transcribe_support;

use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_adapters::asr::{AsrParams, QwenAsr, QwenAsrConfig};
use prxvc_adapters::vad::{GateParams, SileroVadProvider, VadEvent, VadGate};
use prxvc_media::{Lang, MonoTime, SampleRate};
use prxvc_ports::{VadEngine, VadError, VadProvider};
use transcribe_support::runner::{Utterance, run};
use transcribe_support::{
    asr_model, audio, errors, median, oracle, percentile, rows, silero_model,
};

/// Wraps an engine and records how long each frame took.
struct Timed {
    inner: Box<dyn VadEngine>,
    spent: Arc<Mutex<Vec<f64>>>,
}

impl VadEngine for Timed {
    fn frame_len(&self) -> usize {
        self.inner.frame_len()
    }
    fn rate(&self) -> SampleRate {
        self.inner.rate()
    }
    fn probability(&mut self, frame: &[f32]) -> Result<f32, VadError> {
        let t = Instant::now();
        let p = self.inner.probability(frame);
        self.spent.lock().push(t.elapsed().as_secs_f64() * 1e6);
        p
    }
    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// The energy reference of the benchmark: 20 ms RMS above 1 % of the file's peak RMS.
fn energy_ref(a: &[f32]) -> Option<(f64, f64)> {
    let w = 320;
    let rms: Vec<f32> = a
        .chunks_exact(w)
        .map(|c| (c.iter().map(|s| s * s).sum::<f32>() / w as f32).sqrt())
        .collect();
    let peak = rms.iter().copied().fold(0.0, f32::max);
    let idx: Vec<usize> = (0..rms.len()).filter(|&i| rms[i] > 0.01 * peak).collect();
    Some((
        *idx.first()? as f64 * 0.02,
        (*idx.last()? + 1) as f64 * 0.02,
    ))
}

fn segments(
    provider: &SileroVadProvider,
    params: GateParams,
    a: &[f32],
    spent: &Arc<Mutex<Vec<f64>>>,
) -> Vec<(f64, f64)> {
    let engine = Timed {
        inner: provider.open().unwrap(),
        spent: Arc::clone(spent),
    };
    let mut gate = VadGate::new(Box::new(engine), params);
    let mut out = Vec::new();
    let mut start = 0.0;
    let mut handle = |e: VadEvent| match e {
        VadEvent::SpeechStart { at, .. } => start = at.as_nanos() as f64 / 1e9,
        VadEvent::SpeechEnd { end, .. } => out.push((start, end.as_nanos() as f64 / 1e9)),
    };
    for (k, chunk) in a.chunks(320).enumerate() {
        for e in gate
            .feed(chunk, MonoTime::from_nanos((k * 320) as u64 * 62_500))
            .unwrap()
        {
            handle(e);
        }
    }
    if let Some(e) = gate.flush() {
        handle(e);
    }
    out
}

#[test]
#[ignore = "needs the Silero model"]
fn live_vad_matches_the_benchmark_and_costs_little() {
    let provider = SileroVadProvider::load(&silero_model()).unwrap();
    let spent = Arc::new(Mutex::new(Vec::new()));
    // The benchmark's offline setting: no padding, 300 ms silence ends a segment.
    let bench = GateParams {
        pad: Duration::ZERO,
        min_silence: Duration::from_millis(300),
        ..GateParams::default()
    };
    let t_all = Instant::now();
    let mut audio_s = 0.0;
    for subset in ["silence", "nonspeech"] {
        let (mut fa, mut total, mut n) = (0.0, 0.0, 0);
        for r in rows(subset) {
            let a = audio(&r);
            total += a.len() as f64 / 16_000.0;
            audio_s += a.len() as f64 / 16_000.0;
            let segs = segments(&provider, bench.clone(), &a, &spent);
            n += 1;
            fa += segs.iter().map(|(s, e)| e - s).sum::<f64>();
            let production = segments(&provider, GateParams::default(), &a, &spent);
            assert!(production.is_empty(), "{}: {production:?}", r.id);
        }
        println!(
            "VAD {subset}: {n} files, {total:.0} s of audio, false alarm {fa:.2} s (benchmark: 0)"
        );
        assert!(fa < 0.01);
    }
    let short = rows("short");
    let missed = short
        .iter()
        .filter(|r| {
            let a = audio(r);
            audio_s += a.len() as f64 / 16_000.0;
            segments(&provider, bench.clone(), &a, &spent).is_empty()
        })
        .count();
    println!(
        "VAD short: {} files, missed {missed} (benchmark: 0)",
        short.len()
    );
    assert_eq!(missed, 0);
    let (mut starts, mut ends) = (Vec::new(), Vec::new());
    for subset in ["short", "num_zh", "num_en", "codeswitch"] {
        for r in rows(subset) {
            let a = audio(&r);
            audio_s += a.len() as f64 / 16_000.0;
            let segs = segments(&provider, bench.clone(), &a, &spent);
            if let (Some(first), Some(last), Some(reference)) =
                (segs.first(), segs.last(), energy_ref(&a))
            {
                starts.push(first.0 - reference.0);
                ends.push(last.1 - reference.1);
            }
        }
    }
    println!(
        "VAD boundaries vs energy reference on {} clean files: start median {:+.3} s (benchmark +0.032), end median {:+.3} s (benchmark +0.070); end within 40 ms of the benchmark median: {}",
        starts.len(),
        median(starts.clone()),
        median(ends.clone()),
        (median(ends.clone()) - 0.07).abs() <= 0.04
    );
    assert!((median(starts) - 0.032).abs() <= 0.04);
    assert!((median(ends) - 0.07).abs() <= 0.04);
    // Natural pauses: the 0.9 s pause inside the file ends a segment under the production setting.
    let (mut two, mut files) = (0, 0);
    for subset in ["pause_zh", "pause_en"] {
        for r in rows(subset) {
            let a = audio(&r);
            audio_s += a.len() as f64 / 16_000.0;
            let segs = segments(&provider, GateParams::default(), &a, &spent);
            files += 1;
            two += usize::from(segs.len() == 2);
            if let Some(o) = oracle(&r.id) {
                println!(
                    "VAD {}: {} segments {:?}, oracle {:.2}..{:.2}",
                    r.id,
                    segs.len(),
                    segs.iter()
                        .map(|(s, e)| format!("{s:.2}-{e:.2}"))
                        .collect::<Vec<_>>(),
                    o.0,
                    o.1
                );
            }
        }
    }
    println!("VAD pause files: {two} of {files} split into two segments by the 0.9 s pause");
    let frames = spent.lock().clone();
    println!(
        "VAD cost: {} frames, per frame median {:.0} us, p99 {:.0} us, max {:.0} us; inside the engine {:.4} s per second of audio ({:.2}% of one core); whole test {:.1} s for {audio_s:.0} s of audio",
        frames.len(),
        median(frames.clone()),
        percentile(frames.clone(), 0.99),
        percentile(frames.clone(), 1.0),
        frames.iter().sum::<f64>() / 1e6 / (frames.len() as f64 * 0.032),
        frames.iter().sum::<f64>() / 1e6 / (frames.len() as f64 * 0.032) * 100.0,
        t_all.elapsed().as_secs_f64()
    );
    assert!(
        median(frames) < 1000.0,
        "a frame must cost well under a millisecond"
    );
}

fn print_header() {
    println!(
        "{:<16} {:>5} {:>6} {:>5} {:>4} {:>4} {:>4} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>7} {:>3}",
        "id",
        "dur",
        "err%",
        "parts",
        "rew",
        "retr",
        "swr",
        "1st-onset",
        "1st-vad",
        "fin-end",
        "fin-speech",
        "s50-early",
        "s90-early",
        "gap-med",
        "seg"
    );
}

fn print_row(u: &Utterance) {
    let f = |v: Option<f64>| v.map_or("-".to_owned(), |x| format!("{x:.2}"));
    println!(
        "{:<16} {:>5.1} {:>6.1} {:>5} {:>4} {:>4} {:>4} {:>8} {:>8} {:>8.0} {:>8.0} {:>8} {:>8} {:>7.0} {:>3}",
        u.id,
        u.audio_s,
        100.0 * u.errors as f64 / u.ref_len.max(1) as f64,
        u.partials,
        u.rewrites,
        u.retractions,
        u.stable_wrong,
        f(u.first_partial_after_onset_ms.map(|x| x / 1000.0)),
        f(u.first_partial_after_vad_ms.map(|x| x / 1000.0)),
        u.final_after_endpoint_ms,
        u.final_after_speech_end_ms,
        f(u.stable_50_early_s),
        f(u.stable_90_early_s),
        median(u.partial_gaps_ms.clone()),
        u.segments,
    );
}

fn summarize(label: &str, us: &[Utterance]) {
    let (e, n): (usize, usize) = us
        .iter()
        .fold((0, 0), |a, u| (a.0 + u.errors, a.1 + u.ref_len));
    let col = |f: &dyn Fn(&Utterance) -> Option<f64>| us.iter().filter_map(f).collect::<Vec<_>>();
    println!(
        "{label}: {} utterances, error rate {:.2}% ({e}/{n}); partials {} (rewrote earlier text {}, retractions of the declared prefix {}, declared prefix wrong against the final {}); first partial after onset median {:.2} s; final after endpoint median {:.0} ms p95 {:.0} ms (after true speech end median {:.0} ms); stable 50% at median {:.2} s before the end of speech, 90% at median {:.2} s",
        us.len(),
        100.0 * e as f64 / n.max(1) as f64,
        us.iter().map(|u| u.partials).sum::<usize>(),
        us.iter().map(|u| u.rewrites).sum::<usize>(),
        us.iter().map(|u| u.retractions).sum::<usize>(),
        us.iter().map(|u| u.stable_wrong).sum::<usize>(),
        median(col(&|u| u.first_partial_after_onset_ms)) / 1000.0,
        median(us.iter().map(|u| u.final_after_endpoint_ms).collect()),
        percentile(us.iter().map(|u| u.final_after_endpoint_ms).collect(), 0.95),
        median(us.iter().map(|u| u.final_after_speech_end_ms).collect()),
        median(col(&|u| u.stable_50_early_s)),
        median(col(&|u| u.stable_90_early_s)),
    );
    assert_eq!(
        us.iter().map(|u| u.retractions).sum::<usize>(),
        0,
        "a declared prefix was retracted"
    );
}

fn load_asr(params: AsrParams) -> QwenAsr {
    let t = Instant::now();
    let asr = QwenAsr::load_native(
        &asr_model(),
        QwenAsrConfig {
            params,
            ..QwenAsrConfig::default()
        },
    )
    .expect("load the recogniser");
    println!(
        "recogniser loaded in {:.0} ms, warm-up {:.0} ms",
        t.elapsed().as_secs_f64() * 1000.0,
        asr.warm_up_duration().as_secs_f64() * 1000.0
    );
    asr
}

fn pick(subset: &str, n: usize) -> Vec<transcribe_support::Row> {
    rows(subset).into_iter().take(n).collect()
}

fn lang_of(r: &transcribe_support::Row) -> Lang {
    if r.lang == "zh" { Lang::Zh } else { Lang::En }
}

fn run_rows(
    asr: &QwenAsr,
    vad: &SileroVadProvider,
    list: &[transcribe_support::Row],
) -> Vec<Utterance> {
    list.iter()
        .map(|r| {
            let a = audio(r);
            let speech = oracle(&r.id).unwrap_or((0.5, a.len() as f64 / 16_000.0));
            run(asr, vad, lang_of(r), &r.id, &a, speech, &r.reference)
        })
        .collect()
}

#[test]
#[ignore = "needs the models"]
fn live_ten_mandarin_and_ten_english_utterances_at_real_time_pace() {
    let vad = SileroVadProvider::load(&silero_model()).unwrap();
    let asr = load_asr(AsrParams::default());
    print_header();
    let mut all = Vec::new();
    for (label, subset) in [("Mandarin", "zh_read"), ("English", "en_read")] {
        let us = run_rows(&asr, &vad, &pick(subset, 10));
        us.iter().for_each(print_row);
        summarize(label, &us);
        all.extend(us);
    }
    println!("recogniser counters: {:?}", asr.stats());
    assert!(all.iter().all(|u| u.partials > 0));
    assert!(all.iter().all(|u| !u.final_text.is_empty()));
    let _ = errors("", "");
}

#[test]
#[ignore = "needs the models"]
fn live_two_streams_at_once_share_the_engine() {
    let vad = SileroVadProvider::load(&silero_model()).unwrap();
    let asr = load_asr(AsrParams::default());
    let (zh_rows, en_rows) = (pick("zh_read", 10), pick("en_read", 10));
    let t = Instant::now();
    let (zh, en) = std::thread::scope(|s| {
        let zh = s.spawn(|| run_rows(&asr, &vad, &zh_rows));
        let en = s.spawn(|| run_rows(&asr, &vad, &en_rows));
        (zh.join().unwrap(), en.join().unwrap())
    });
    println!(
        "both directions finished in {:.1} s",
        t.elapsed().as_secs_f64()
    );
    print_header();
    zh.iter().for_each(print_row);
    en.iter().for_each(print_row);
    summarize("Mandarin (concurrent)", &zh);
    summarize("English (concurrent)", &en);
    let stats = asr.stats();
    println!("recogniser counters: {stats:?}");
    println!(
        "decode queue: partial passes {}, skipped because the previous pass was still wanted or running {}, preempted by finals {}, longest wait of a requested pass {:.0} ms, mean wait {:.0} ms, longest decode {:.0} ms",
        stats.partial_passes,
        stats.skipped_busy,
        stats.aborted_passes,
        stats.max_pass_wait.as_secs_f64() * 1000.0,
        stats.mean_pass_wait.as_secs_f64() * 1000.0,
        stats.max_decode.as_secs_f64() * 1000.0
    );
    assert!(
        zh.iter()
            .chain(&en)
            .all(|u| u.partials > 0 && !u.final_text.is_empty())
    );
}

#[test]
#[ignore = "needs the models"]
fn live_a_long_monologue_is_cut_at_the_maximum_segment_length() {
    let asr = load_asr(AsrParams::default());
    for (subset, lang) in [("long_zh", Lang::Zh), ("long_en", Lang::En)] {
        let r = &rows(subset)[0];
        let a = audio(r);
        let mut stream =
            prxvc_ports::StreamingAsr::open(&asr, lang, prxvc_ports::AsrContext::default())
                .unwrap();
        let t0 = Instant::now();
        let mut finals: Vec<(f64, f64, String)> = Vec::new();
        let (mut parts, mut gaps, mut last) = (0, Vec::new(), None::<Instant>);
        for (k, chunk) in a.chunks(320).enumerate() {
            transcribe_support::sleep_until(
                t0 + Duration::from_secs_f64(((k + 1) * 320) as f64 / 16_000.0),
            );
            if let Some(h) = prxvc_ports::AsrStream::feed(
                &mut *stream,
                chunk,
                MonoTime::from_nanos((k * 320) as u64 * 62_500),
            )
            .unwrap()
            {
                let now = Instant::now();
                if h.is_final {
                    finals.push((
                        h.audio_end.as_nanos() as f64 / 1e9,
                        t0.elapsed().as_secs_f64(),
                        h.text.clone(),
                    ));
                } else {
                    parts += 1;
                    if let Some(l) = last {
                        gaps.push(now.duration_since(l).as_secs_f64() * 1000.0);
                    }
                    last = Some(now);
                }
            }
        }
        let fin =
            prxvc_ports::AsrStream::finalize(&mut *stream, &prxvc_ports::CancellationToken::new())
                .unwrap();
        let mut text = String::new();
        for (_, _, t) in &finals {
            text.push_str(t);
            text.push(' ');
        }
        text.push_str(&fin.text);
        let (e, n) = errors(&r.reference, &text);
        println!(
            "{}: {:.1} s of audio, {} forced cuts at audio positions {:?} (final text arrived at wall {:?}), {parts} partials, partial gap median {:.0} ms p95 {:.0} ms max {:.0} ms, error rate {:.2}% ({e}/{n})",
            r.id,
            a.len() as f64 / 16_000.0,
            finals.len(),
            finals
                .iter()
                .map(|f| format!("{:.2}", f.0))
                .collect::<Vec<_>>(),
            finals
                .iter()
                .map(|f| format!("{:.2}", f.1))
                .collect::<Vec<_>>(),
            median(gaps.clone()),
            percentile(gaps.clone(), 0.95),
            percentile(gaps, 1.0),
            100.0 * e as f64 / n as f64
        );
        println!("counters: {:?}", asr.stats());
        assert_eq!(
            finals.len(),
            1,
            "a 45 to 53 s monologue is cut once at 25 s"
        );
        assert!(
            finals[0].0 <= 25.2 && finals[0].0 > 22.0,
            "{:?}",
            finals[0].0
        );
    }
}

#[test]
#[ignore = "needs the models"]
fn live_refine_decode_accuracy_on_the_benchmark_files() {
    use prxvc_ports::{AsrContext, CancellationToken, RefineAsr};
    let asr = load_asr(AsrParams::default());
    for (subset, label, benchmark) in [
        ("zh_read", "Mandarin CER", 2.8),
        ("en_read", "English WER", 6.8),
    ] {
        let (mut e, mut n, mut decode_ms) = (0, 0, Vec::new());
        let (mut e10, mut n10) = (0, 0);
        for (i, r) in rows(subset).into_iter().enumerate() {
            let a = audio(&r);
            let t = Instant::now();
            let h = asr
                .transcribe(
                    &a,
                    SampleRate::HZ_16000,
                    lang_of(&r),
                    &AsrContext::default(),
                    &CancellationToken::new(),
                )
                .unwrap();
            decode_ms.push(t.elapsed().as_secs_f64() * 1000.0 / (a.len() as f64 / 16_000.0));
            transcribe_support::dump("PRXVC_DUMP_REFINE", &r.id, &r.lang, &r.reference, &h.text);
            let (ee, nn) = errors(&r.reference, &h.text);
            e += ee;
            n += nn;
            if i < 10 {
                println!("  {} plain {ee}/{nn}: {}", r.id, h.text);
                e10 += ee;
                n10 += nn;
            }
        }
        println!(
            "{label} on the first ten files (the ones of the real-time runs) through the refine port: {:.2}% ({e10}/{n10})",
            100.0 * e10 as f64 / n10 as f64
        );
        println!(
            "{label} over {} files through the refine port: {:.2}% ({e}/{n}); benchmark {benchmark}% (its normaliser also converts numbers and traditional characters); decode time per second of audio median {:.0} ms",
            rows(subset).len(),
            100.0 * e as f64 / n as f64,
            median(decode_ms)
        );
    }
}
