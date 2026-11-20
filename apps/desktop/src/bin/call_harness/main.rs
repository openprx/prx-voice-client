//! `call_harness`: one direction of the real pipeline with the real providers, a WAV file as the source and either a WAV file or the virtual microphone as the output (pipeline-design CT-15). No call application, no real microphone, no system default device is touched.
//!
//! ```text
//! call_harness --direction outgoing|incoming --set zh|en [--commit segment|early]
//!              [--output wav|vmic] [--count 10] [--numbers 5] [--out DIR] [--tag NAME]
//!              [--own zh --peer en] [--max-backlog-ms N] [--file X.wav --reference "text"] [--bench-asr DIR]
//! ```
//!
//! * `--set zh` takes the Mandarin utterances of `bench/asr/data` (read speech, then the
//!   synthetic sentences with numbers, dates, names and negation), `--set en` the English ones.
//!   The set's language must be the direction's source language (by default `zh` goes through
//!   `outgoing`, `en` through `incoming`).
//! * `--commit segment` is the pipeline's default (a clause is committed at a pause or the end of
//!   the segment); `early` also commits at a sentence-final mark of a partial that stopped changing.
//! * `--output wav` writes `<out>/<utterance>.wav`. `--output vmic` plays to the virtual microphone,
//!   records it (by UID, no default device changed) and recognises the recording again: the round trip.
//! * Every utterance runs at the pace of real time with 0.3 s of silence before and 2 s after.
//!   The report is a latency table in the terms of `bench/e2e/REPORT.md`, with the machine's load.

#[path = "../../call_kit/mod.rs"]
mod call_kit;

mod report;
mod run;
mod sinks;
mod source;
mod sysinfo;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use prxvc_media::{Direction, Lang, SystemClock};

use call_kit::config::load_current;
use call_kit::params::pipeline_params;
use call_kit::providers::{ProviderHost, build_directions};
use report::{UtteranceRun, table};
use run::{Output, Rig, run_utterance, speech_bounds};
use source::Recording;

const DEFAULT_BENCH_ASR: &str = "/Users/nn/worker/voice/bench/asr";
/// An utterance with its reference text.
struct Item {
    id: String,
    reference: String,
    path: PathBuf,
}

struct Options {
    direction: Direction,
    set: String,
    early: bool,
    vmic: bool,
    count: usize,
    numbers: usize,
    out: PathBuf,
    tag: String,
    file: Option<(PathBuf, String)>,
    bench_asr: PathBuf,
    max_backlog_ms: Option<u32>,
    own: String,
    peer: String,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut o = Options {
        direction: Direction::Outgoing,
        set: "zh".into(),
        early: false,
        vmic: false,
        count: 10,
        numbers: 5,
        out: PathBuf::from("target/call_harness"),
        tag: String::new(),
        file: None,
        bench_asr: PathBuf::from(DEFAULT_BENCH_ASR),
        max_backlog_ms: None,
        own: "zh".into(),
        peer: "en".into(),
    };
    let (mut file, mut reference) = (None, String::new());
    let mut it = args.iter();
    let value = |name: &str, it: &mut std::slice::Iter<'_, String>| {
        it.next().cloned().ok_or(format!("{name} needs a value"))
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--direction" => {
                o.direction = match value(arg, &mut it)?.as_str() {
                    "outgoing" => Direction::Outgoing,
                    "incoming" => Direction::Incoming,
                    other => return Err(format!("unknown direction {other}")),
                }
            }
            "--set" => o.set = value(arg, &mut it)?,
            "--commit" => {
                o.early = match value(arg, &mut it)?.as_str() {
                    "segment" => false,
                    "early" => true,
                    other => return Err(format!("unknown commit behaviour {other}")),
                }
            }
            "--output" => {
                o.vmic = match value(arg, &mut it)?.as_str() {
                    "wav" => false,
                    "vmic" => true,
                    other => return Err(format!("unknown output {other}")),
                }
            }
            "--count" => o.count = value(arg, &mut it)?.parse().map_err(|_| "bad --count")?,
            "--numbers" => o.numbers = value(arg, &mut it)?.parse().map_err(|_| "bad --numbers")?,
            "--out" => o.out = PathBuf::from(value(arg, &mut it)?),
            "--tag" => o.tag = value(arg, &mut it)?,
            "--max-backlog-ms" => {
                o.max_backlog_ms = Some(
                    value(arg, &mut it)?
                        .parse()
                        .map_err(|_| "bad --max-backlog-ms")?,
                )
            }
            "--own" => o.own = value(arg, &mut it)?,
            "--peer" => o.peer = value(arg, &mut it)?,
            "--bench-asr" => o.bench_asr = PathBuf::from(value(arg, &mut it)?),
            "--file" => file = Some(PathBuf::from(value(arg, &mut it)?)),
            "--reference" => reference = value(arg, &mut it)?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    o.file = file.map(|f| (f, reference));
    if o.tag.is_empty() {
        o.tag = format!(
            "{}-{}-{}-{}",
            format!("{:?}", o.direction).to_lowercase(),
            o.set,
            if o.early { "early" } else { "segment" },
            if o.vmic { "vmic" } else { "wav" }
        );
    }
    Ok(o)
}

/// The utterances of the set: read speech first, then the synthetic number sentences.
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
    let mut rows: Vec<(String, String, String, String)> = Vec::new(); // subset, id, wav, reference
    for line in text.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() >= 5 {
            rows.push((
                f[1].to_owned(),
                f[0].to_owned(),
                f[3].to_owned(),
                f[4].to_owned(),
            ));
        }
    }
    let take = |subset: &str, n: usize| -> Vec<Item> {
        rows.iter()
            .filter(|r| r.0 == subset)
            .take(n)
            .map(|r| Item {
                id: r.1.clone(),
                reference: r.3.clone(),
                path: o.bench_asr.join(&r.2),
            })
            .collect()
    };
    let (read, num) = match o.set.as_str() {
        "zh" => ("zh_read", "num_zh"),
        "en" => ("en_read", "num_en"),
        other => return Err(format!("unknown set {other} (zh or en)")),
    };
    let mut items = take(read, o.count);
    items.extend(take(num, o.numbers));
    if items.is_empty() {
        return Err(format!("no utterances found in {}", manifest.display()));
    }
    Ok(items)
}

fn lang_of(set: &str) -> Option<Lang> {
    match set {
        "zh" => Some(Lang::Zh),
        "en" => Some(Lang::En),
        _ => None,
    }
}

fn execute(o: &Options) -> Result<bool, String> {
    let items = load_items(o)?;
    let mut config = load_current();
    config.pipeline.commit_on_stable_partial = o.early;
    // The languages of the run, whatever the interface language derives them to.
    config.call.self_language.clone_from(&o.own);
    config.call.peer_language.clone_from(&o.peer);
    if let Some(ms) = o.max_backlog_ms {
        config.pipeline.max_backlog_ms = ms;
    }
    let registry = ProviderHost::registry_for(&config, None).map_err(|f| f.detail)?;
    let plan = registry.resolve_directions(&[o.direction]).map_err(|p| {
        p.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let direction_plan = plan
        .directions
        .first()
        .ok_or("no direction planned")?
        .clone();
    if let Some(want) = lang_of(&o.set).filter(|_| o.file.is_none())
        && want != direction_plan.source
    {
        return Err(format!(
            "the {:?} direction listens to {}, not {}: use the other direction for this set",
            o.direction, direction_plan.source, want
        ));
    }
    println!("== {} ==", o.tag);
    println!(
        "direction {:?}: {} to {}, recognition {:?}, commit {}, output {}",
        o.direction,
        direction_plan.source,
        direction_plan.target,
        plan.recognition,
        if o.early {
            "early (stable sentence end)"
        } else {
            "at the pause or segment end"
        },
        if o.vmic {
            "virtual microphone with round trip"
        } else {
            "wav file"
        }
    );
    println!("max backlog {} ms", config.pipeline.max_backlog_ms);
    println!("{}", sysinfo::snapshot("machine before"));
    let mut built = build_directions(&registry, &config, &plan).map_err(|f| {
        f.faults
            .iter()
            .map(|x| x.detail.clone())
            .collect::<Vec<_>>()
            .join("\n")
    })?;
    let build = built.pop().ok_or("nothing was built")?;
    let mut params = pipeline_params(
        &config,
        o.direction,
        (direction_plan.source, direction_plan.target),
        direction_plan.tts.primary.voice.selection(),
        plan.recognition,
    );
    params.commit.commit_on_stable_partial = o.early;
    let out_dir = o.out.join(&o.tag);
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let output = if o.vmic {
        let spec = registry
            .resolve_asr_refine(direction_plan.target)
            .map_err(|p| {
                p.iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            })?;
        let recogniser: Arc<dyn prxvc_ports::RefineAsr> = Arc::from(
            registry
                .build_asr_refine(&spec.primary)
                .map_err(|e| e.to_string())?,
        );
        Output::VirtualMic {
            dir: out_dir.clone(),
            recogniser,
        }
    } else {
        Output::Wav(out_dir.clone())
    };
    let rig = Rig {
        clock: Arc::new(SystemClock::new()),
        params,
        providers: build.providers.clone(),
        output,
        jitter: Duration::from_millis(u64::from(config.pipeline.jitter_buffer_ms)),
    };
    let mut runs: Vec<UtteranceRun> = Vec::new();
    for item in &items {
        let rec = Recording::read(&item.path)?;
        let bounds = speech_bounds(build.providers.vad.as_ref(), &rec)?;
        eprintln!(
            "{}: {:.1} s, speech {:.2}..{:.2} s",
            item.id,
            rec.seconds(),
            bounds.0,
            bounds.1
        );
        match run_utterance(&rig, &item.id, &item.reference, &rec, bounds) {
            Ok(r) => {
                println!(
                    "- {}: {} => {} [{:.1} s audio]",
                    r.id,
                    r.source_text,
                    r.translation,
                    r.audio_seconds.iter().sum::<f64>()
                );
                println!("    reference: {}", r.reference);
                println!("    counters: {}", r.counters);
                for c in &r.clauses {
                    let stages: Vec<String> = c
                        .stages
                        .iter()
                        .map(|(st, at)| {
                            format!(
                                "{} {:+.2}",
                                call_kit::trace::stage_name(*st),
                                (at.as_nanos() as f64 - r.speech_start_ns) / 1e9
                            )
                        })
                        .collect();
                    println!(
                        "    clause {}.{} [{}] (seconds from speech start): {}",
                        c.clause.segment().seq(),
                        c.clause.index(),
                        c.outcome().unwrap_or("open"),
                        stages.join(", ")
                    );
                }
                if let Some(rt) = &r.round_trip {
                    println!(
                        "    round trip: spoke \"{}\", heard \"{}\" (error rate {:.2}, peak {:.2}, {:.1} s)",
                        r.spoken, rt.recognised, rt.error_rate, rt.peak, rt.seconds
                    );
                }
                runs.push(r);
            }
            Err(e) => println!("- {}: FAILED: {e}", item.id),
        }
    }
    println!("{}", sysinfo::snapshot("machine after"));
    println!("\n{}", table(&runs));
    summary(&runs, o);
    drop(rig);
    drop(build);
    drop(registry);
    Ok(!runs.is_empty())
}

/// The round trip's verdict and the free-engine check.
fn summary(runs: &[UtteranceRun], o: &Options) {
    println!("utterances measured: {} of the set", runs.len());
    if o.vmic {
        let trips: Vec<_> = runs.iter().filter_map(|r| r.round_trip.as_ref()).collect();
        let audible = trips
            .iter()
            .filter(|t| t.peak > 0.01 && t.seconds > 0.3)
            .count();
        let good = trips.iter().filter(|t| t.error_rate <= 0.35).count();
        let mean = trips.iter().map(|t| t.error_rate).sum::<f64>() / trips.len().max(1) as f64;
        println!(
            "round trip: {} of {} recordings carry speech, {} of {} are recognised within 0.35 error rate (mean {:.2})",
            audible,
            trips.len(),
            good,
            trips.len(),
            mean
        );
    }
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
    let options = match parse(&args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!(
                "error: {e}\nsee the header of apps/desktop/src/bin/call_harness/main.rs for the options"
            );
            return ExitCode::from(2);
        }
    };
    match execute(&options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn the_options_name_the_run() {
        let o = parse(&args(&[
            "--direction",
            "incoming",
            "--set",
            "en",
            "--commit",
            "early",
            "--output",
            "vmic",
        ]))
        .unwrap();
        assert_eq!(o.direction, Direction::Incoming);
        assert!(o.early && o.vmic);
        assert_eq!(o.tag, "incoming-en-early-vmic");
        assert!(parse(&args(&["--commit", "late"])).is_err());
        assert!(parse(&args(&["--nope"])).is_err());
    }

    #[test]
    fn the_sets_are_read_from_the_manifest_in_order() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        let rows = [
            "zh_read/zh000\tzh_read\tzh\tdata/wav/a.wav\t你好\t0\t1\tx",
            "zh_read/zh001\tzh_read\tzh\tdata/wav/b.wav\t再见\t0\t1\tx",
            "num_zh/n000\tnum_zh\tzh\tdata/wav/c.wav\t三月三日\t1\t1\tx",
            "en_read/e000\ten_read\ten\tdata/wav/d.wav\thello\t0\t1\tx",
        ];
        std::fs::write(
            dir.path().join("data/manifest.tsv"),
            format!(
                "id\tsubset\tlang\twav\tref\tsynthetic\tdur\tsrc\n{}\n",
                rows.join("\n")
            ),
        )
        .unwrap();
        let mut o = parse(&args(&["--count", "1", "--numbers", "1"])).unwrap();
        o.bench_asr = dir.path().to_path_buf();
        let items = load_items(&o).unwrap();
        assert_eq!(
            items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
            ["zh_read/zh000", "num_zh/n000"]
        );
        assert_eq!(items[0].path, dir.path().join("data/wav/a.wav"));
        o.set = "klingon".into();
        assert!(load_items(&o).is_err());
    }
}
