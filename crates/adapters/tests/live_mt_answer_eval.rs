//! Live evaluation of the translation prompt against questions, requests and second-person
//! sentences: how often the model answers or refuses instead of translating, or replies in the
//! wrong language. Ignored by default; it never starts or stops a server.
//!
//! The sentences are `tests/data/mt_conversational_{zh,en}.txt`, one per line.
//!
//! Environment (as `live_mt_llama_server`): `PRXVC_MT_LIVE_URL` (default `http://127.0.0.1:8080`),
//! `PRXVC_MT_LIVE_MODEL`, `PRXVC_MT_LIVE_TEMPLATE` (`gemma4` or `generic`).
//!
//! Run: `cargo test -p prxvc-adapters --test live_mt_answer_eval -- --ignored --nocapture`
//!
//! With `PRXVC_MT_EVAL_RETRY=1` a request that came back flagged (answer-like, refusal, wrong
//! language) or that opens like an answer is asked once more with the strict instruction, as the
//! pipeline does, and the counts are of what is left after that.
//!
//! Every sentence is translated twice, without context and with the two previous sentences as
//! context. The counts are of the requests the adapter flagged (`answer_like`, `wrong_language`,
//! `refusal`) and, from this test's own check, of questions whose translation does not end in a
//! question mark. Each flagged output is printed so that it can be read.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use prxvc_adapters::mt::{
    MtObserver, MtReport, OpenAiChatConfig, OpenAiChatMt, PromptTemplate, Suspect, Timeouts,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtRequest, Translator};
use serde_json::json;

#[derive(Default)]
struct Reports(Mutex<Vec<MtReport>>);

fn retry_enabled() -> bool {
    std::env::var("PRXVC_MT_EVAL_RETRY").is_ok_and(|v| v == "1")
}

impl MtObserver for Reports {
    fn on_report(&self, report: &MtReport) {
        self.0.lock().unwrap().push(report.clone());
    }
}

fn sentences(file: &str) -> Vec<String> {
    let path = format!("{}/tests/data/{file}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{path}: {e}"))
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// One request; the output, the flags and the time to the first token in milliseconds.
fn translate(
    mt: &OpenAiChatMt,
    reports: &Reports,
    index: u16,
    (source, target): (Lang, Lang),
    text: &str,
    context: Vec<(String, String)>,
    strict_language: bool,
) -> (String, Vec<Suspect>, f64) {
    let before = reports.0.lock().unwrap().len();
    let mut stream = mt
        .translate(MtRequest {
            clause: ClauseId::new(SegmentId::first(Direction::Outgoing), index),
            source,
            target,
            text: text.into(),
            context,
            glossary: Vec::new(),
            strict_language,
        })
        .expect("request accepted");
    let cancel = CancellationToken::new();
    let mut out = String::new();
    while let Some(item) = stream.next(&cancel) {
        match item {
            Ok(MtDelta::Text(t)) => out.push_str(&t),
            Ok(MtDelta::Done { .. }) => {}
            Err(e) => panic!("transport failure: {e}"),
        }
    }
    let all = reports.0.lock().unwrap();
    let report = all.get(before);
    let suspects = report.map(|r| r.suspects.clone()).unwrap_or_default();
    let first = report
        .and_then(|r| r.first_token)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    (out, suspects, first)
}

/// What the pipeline asks again for: a flagged reply or one that opens like an answer or refusal.
fn needs_retry(source: &str, out: &str, suspects: &[Suspect]) -> bool {
    suspects.iter().any(|s| {
        matches!(s, Suspect::AnswerLike | Suspect::WrongLanguage | Suspect::Refusal)
    }) || prxvc_media::answer::opens_like_answer(source, out)
        || prxvc_media::answer::opens_like_refusal(source, out)
}

fn is_question(text: &str) -> bool {
    text.trim_end().ends_with(['?', '？'])
}

#[test]
#[ignore = "needs an OpenAI-compatible server; see the module documentation"]
fn live_mt_answer_eval() {
    let url = std::env::var("PRXVC_MT_LIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:8080".into());
    let model = std::env::var("PRXVC_MT_LIVE_MODEL").unwrap_or_else(|_| "default".into());
    let template: PromptTemplate = std::env::var("PRXVC_MT_LIVE_TEMPLATE")
        .unwrap_or_else(|_| "gemma4".into())
        .parse()
        .expect("PRXVC_MT_LIVE_TEMPLATE is gemma4 or generic");
    let mut params = serde_json::Map::new();
    params.insert("temperature".into(), json!(0));
    params.insert("cache_prompt".into(), json!(true));
    params.insert("chat_template_kwargs".into(), json!({"enable_thinking": false}));
    let cfg = OpenAiChatConfig::new(&url, &model)
        .with_provider_id("live-eval")
        .with_template(template)
        .with_params(params)
        .with_timeouts(Timeouts {
            connect: Duration::from_secs(2),
            first_token: Duration::from_secs(60),
            idle: Duration::from_secs(30),
        });
    let reports = Arc::new(Reports::default());
    let mt = OpenAiChatMt::with_observer(cfg, reports.clone()).expect("configuration");
    println!("server {url}, model {model}, template {template}");

    let mut totals = [0usize; 6];
    let (mut requests, mut retried) = (0usize, 0usize);
    let mut first_tokens: Vec<f64> = Vec::new();
    for (langs, file) in [
        ((Lang::Zh, Lang::En), "mt_conversational_zh.txt"),
        ((Lang::En, Lang::Zh), "mt_conversational_en.txt"),
    ] {
        let set = sentences(file);
        assert!(set.len() >= 40, "{file} has {} sentences", set.len());
        for context_pairs in [0usize, 2] {
            println!("\n== {} -> {}, {context_pairs} context pairs, {} sentences ==", langs.0, langs.1, set.len());
            let mut history: Vec<(String, String)> = Vec::new();
            let mut counts = [0usize; 6];
            for (i, text) in set.iter().enumerate() {
                let context: Vec<(String, String)> = history.iter().rev().take(context_pairs).rev().cloned().collect();
                let index = u16::try_from(i).unwrap_or(0);
                let (mut out, mut suspects, first) =
                    translate(&mt, &reports, index, langs, text, context.clone(), false);
                first_tokens.push(first);
                if retry_enabled() && needs_retry(text, &out, &suspects) {
                    retried += 1;
                    println!("  retry [{:>2}] {text}\n       -> {out}   {suspects:?}", i + 1);
                    (out, suspects, _) = translate(&mt, &reports, index, langs, text, context, true);
                }
                let answer = suspects.contains(&Suspect::AnswerLike);
                let wrong = suspects.contains(&Suspect::WrongLanguage);
                let refusal = suspects.contains(&Suspect::Refusal);
                let unmarked = is_question(text) && !is_question(&out);
                let other = suspects
                    .iter()
                    .any(|s| !matches!(s, Suspect::AnswerLike | Suspect::WrongLanguage | Suspect::Refusal));
                let tags = out.contains("source>") || out.contains("<source");
                for (slot, hit) in counts
                    .iter_mut()
                    .zip([answer, wrong, refusal, unmarked, other, tags])
                {
                    *slot += usize::from(hit);
                }
                if answer || wrong || refusal || unmarked || other || tags {
                    println!("  [{:>2}] {text}\n       -> {out}   {suspects:?}{}", i + 1, if unmarked { " (question lost its mark)" } else { "" });
                }
                history.push((text.clone(), out));
                requests += 1;
            }
            println!(
                "  answer_like {}, wrong_language {}, refusal {}, question without question mark {}, other flags {}, tags echoed {}",
                counts[0], counts[1], counts[2], counts[3], counts[4], counts[5]
            );
            for (t, c) in totals.iter_mut().zip(counts) {
                *t += c;
            }
        }
    }
    println!(
        "\nTOTAL over {requests} requests ({retried} asked again): answer_like {}, wrong_language {}, refusal {}, question without question mark {}, other flags {}, tags echoed {}",
        totals[0], totals[1], totals[2], totals[3], totals[4], totals[5]
    );
    first_tokens.sort_by(f64::total_cmp);
    let at = |p: f64| -> f64 { first_tokens[((first_tokens.len() as f64 - 1.0) * p).round() as usize] };
    println!(
        "time to first token ms (first attempts): p50 {:.0}, p95 {:.0}, max {:.0}",
        at(0.5),
        at(0.95),
        at(1.0)
    );
}
