//! `assistant_harness`: the assistant's voice loop with the real providers (Silero VAD, Qwen3-ASR, the dialogue server, Qwen3-TTS), a WAV file as the microphone and a silent output (work package CT-20). No microphone is opened and nothing is played: the reply's audio is written to `<out>/<utterance>.reply.wav`.
//!
//! ```text
//! assistant_harness [--file X.wav [--reference "text"]] [--set zh|en [--subset NAME] [--count N]]
//!                   [--lang zh|en] [--preset NAME] [--out DIR] [--bench-asr DIR]
//! ```
//!
//! * Without `--file`, `--count` utterances of the manifest of `bench/asr/data` (default subset
//!   `short`) in the language of `--set` are used. `--lang` sets `assistant.language` and `--preset`
//!   `assistant.voice_preset` for the run.
//! * Every utterance runs at the pace of real time with 0.3 s of silence before and 2 s after, as
//!   a session of its own on providers that were loaded once.
//! * The report gives, per utterance and as the median over them, the time from the end of the
//!   speech (the last sample of the recording) to: the final recognised text, the first reply
//!   token, and the first reply audio. The first figure includes the silence that ends the user's
//!   turn (`assistant.end_silence_ms`); the table shows it with and without it.

#[path = "../../call_kit/mod.rs"]
mod call_kit;
#[allow(dead_code)] // the harness uses a different part of them than the application
#[path = "../../assistant/issues.rs"]
mod issues;
#[allow(dead_code)]
#[path = "../../assistant/kit.rs"]
mod kit;
#[path = "../../assistant/params.rs"]
mod params;
#[allow(dead_code)]
#[path = "../call_harness/sinks.rs"]
mod sinks;
#[allow(dead_code)]
#[path = "../call_harness/source.rs"]
mod source;

mod report;
mod run;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use prxvc_media::SystemClock;

use call_kit::config::load_current;
use report::{Row, table};
use source::Recording;

const DEFAULT_BENCH_ASR: &str = "/Users/nn/worker/voice/bench/asr";

struct Item {
    id: String,
    reference: String,
    path: PathBuf,
}

struct Options {
    file: Option<(PathBuf, String)>,
    set: String,
    subset: String,
    count: usize,
    lang: Option<String>,
    preset: Option<String>,
    out: PathBuf,
    bench_asr: PathBuf,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        file: None,
        set: "zh".into(),
        subset: "short".into(),
        count: 3,
        lang: None,
        preset: None,
        out: PathBuf::from("target/assistant_harness"),
        bench_asr: PathBuf::from(DEFAULT_BENCH_ASR),
    };
    let (mut file, mut reference) = (None, String::new());
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<'_, String>| {
        it.next().cloned().ok_or(format!("{name} needs a value"))
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--file" => file = Some(PathBuf::from(value(arg, &mut it)?)),
            "--reference" => reference = value(arg, &mut it)?,
            "--set" => o.set = value(arg, &mut it)?,
            "--subset" => o.subset = value(arg, &mut it)?,
            "--count" => o.count = value(arg, &mut it)?.parse().map_err(|_| "bad --count")?,
            "--lang" => o.lang = Some(value(arg, &mut it)?),
            "--preset" => o.preset = Some(value(arg, &mut it)?),
            "--out" => o.out = PathBuf::from(value(arg, &mut it)?),
            "--bench-asr" => o.bench_asr = PathBuf::from(value(arg, &mut it)?),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    o.file = file.map(|f| (f, reference));
    Ok(o)
}

/// The utterances: the named file, or the rows of the manifest of the set and subset.
fn load_items(o: &Options) -> Result<Vec<Item>, String> {
    if let Some((path, reference)) = &o.file {
        return Ok(vec![Item {
            id: path
                .file_stem()
                .map_or("file".into(), |s| s.to_string_lossy().into_owned()),
            reference: reference.clone(),
            path: path.clone(),
        }]);
    }
    let manifest = o.bench_asr.join("data/manifest.tsv");
    let text =
        std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let items: Vec<Item> = text
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f.len() >= 5 && f[2] == o.set && f[1] == o.subset).then(|| Item {
                id: f[0].replace('/', "-"),
                reference: f[4].to_owned(),
                path: o.bench_asr.join(f[3]),
            })
        })
        .take(o.count)
        .collect();
    if items.is_empty() {
        return Err(format!(
            "no utterance of set `{}` and subset `{}` in {}",
            o.set,
            o.subset,
            manifest.display()
        ));
    }
    Ok(items)
}

fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .try_init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    match go(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::from(1)
        }
    }
}

fn go(args: &[String]) -> Result<bool, String> {
    let o = parse(args)?;
    let items = load_items(&o)?;
    let mut config = load_current();
    if let Some(lang) = &o.lang {
        config.assistant.language = if lang == "zh" {
            "zh-CN".into()
        } else {
            lang.clone()
        };
    }
    if let Some(preset) = &o.preset {
        config.assistant.voice_preset = preset.clone();
    }
    let started = std::time::Instant::now();
    let prepared = kit::prepare(&config, None, None).map_err(|issues| {
        issues
            .iter()
            .map(|i| format!("{}: {}", i.key, i.detail))
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    eprintln!(
        "providers ready in {:.1} s",
        started.elapsed().as_secs_f64()
    );
    let clock = Arc::new(SystemClock::new());
    let mut rows: Vec<Row> = Vec::new();
    let mut ok = true;
    for item in &items {
        let rec = Recording::read(&item.path)?;
        let row = run::run_utterance(&config, &prepared, &clock, item, &rec, &o.out)?;
        ok &= row.failure.is_none();
        rows.push(row);
    }
    println!("{}", table(&rows, config.assistant.end_silence_ms));
    Ok(ok)
}
