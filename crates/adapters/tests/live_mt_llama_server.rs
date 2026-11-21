//! Live check of `OpenAiChatMt` against a running OpenAI-compatible server
//! (pipeline-design CT-12 acceptance check). Ignored by default; the test never
//! starts or stops a server.
//!
//! Environment:
//!
//! * `PRXVC_MT_LIVE_URL`: base URL, default `http://127.0.0.1:8090`.
//! * `PRXVC_MT_LIVE_MODEL`: model name in the request, default `default`
//!   (`llama-server` ignores it).
//! * `PRXVC_MT_LIVE_TEMPLATE`: `gemma4` (default) or `generic`.
//!
//! Run: `cargo test -p prxvc-adapters --test live_mt_llama_server -- --ignored --nocapture`
//!
//! Each of two directions translates 20 conversational sentences with numbers,
//! dates, times, names and negations, once without context and once with the
//! two previous clauses as context. Prints source, translation, time to first
//! token and total time per item. Asserts structure only: non-empty, in the
//! target language, digits of the source preserved, no transport failure.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use prxvc_adapters::mt::{
    MtObserver, MtReport, OpenAiChatConfig, OpenAiChatMt, PromptTemplate, Timeouts,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtRequest, Translator};
use serde_json::json;

const ZH_EN: [&str; 20] = [
    "我们下午3:30在机场见面。",
    "会议改到20号上午10点了。",
    "我没有说过这句话。",
    "如果价格不降到350块，我们就不买。",
    "王明明天不来了，他的航班取消了。",
    "我的电话是138-0012-5567，别忘了。",
    "你能不能告诉我他在哪儿？",
    "不要把这份文件发给李华。",
    "这个项目的预算是2500美元，不是25000美元。",
    "我们已经等了45分钟了。",
    "我觉得可能是明天，但我不太确定。",
    "请问现在几点了？",
    "他说他从来没去过上海。",
    "第3号会议室在5楼，不在3楼。",
    "我想订两张周五晚上8点的票。",
    "谢谢你的帮助，我们下周再联系。",
    "他们公司去年增长了12%。",
    "我还没吃饭，因为一直在开会。",
    "不是我，是张伟把文件弄丢的。",
    "我们的航班是CA1234，15号起飞。",
];

const EN_ZH: [&str; 20] = [
    "Let's meet at the airport at 3:45 this afternoon.",
    "The meeting was moved to the 20th at 10 a.m.",
    "I never said that.",
    "If the price doesn't drop to 350 dollars, we won't buy it.",
    "Mr. Smith isn't coming tomorrow, his flight was cancelled.",
    "My phone number is 555-0134, don't forget it.",
    "Could you tell me where he is?",
    "Please don't send this file to Anna Lee.",
    "The budget for this project is 2,500 dollars, not 25,000.",
    "We have been waiting for 45 minutes.",
    "I think it might be tomorrow, but I'm not sure.",
    "What time is it now?",
    "He said he has never been to Shanghai.",
    "Meeting room 3 is on the 5th floor, not the 3rd.",
    "I'd like two tickets for 8 p.m. on Friday.",
    "Thanks for your help, we'll talk again next week.",
    "Their company grew by 12% last year.",
    "I haven't eaten yet because I've been in meetings all day.",
    "It wasn't me, it was David who lost the file.",
    "Our flight is CA1234 and departs on August 15.",
];

#[derive(Default)]
struct Reports(Mutex<Vec<MtReport>>);

impl MtObserver for Reports {
    fn on_report(&self, report: &MtReport) {
        self.0.lock().unwrap().push(report.clone());
    }
}

struct Item {
    translation: String,
    first_token: Duration,
    total: Duration,
    completion_tokens: u32,
}

fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}')
}

/// Whether every digit of `source` occurs in `output` at least as often
/// (order is free: "10 AM on the 20th" reorders "20号上午10点").
fn digits_preserved(source: &str, output: &str) -> bool {
    let count = |text: &str, d: char| text.chars().filter(|c| *c == d).count();
    ('0'..='9').all(|d| count(output, d) >= count(source, d))
}

fn translate_one(
    mt: &OpenAiChatMt,
    direction: Direction,
    index: u16,
    source: Lang,
    target: Lang,
    text: &str,
    context: Vec<(String, String)>,
) -> Item {
    let start = Instant::now();
    let mut stream = mt
        .translate(MtRequest {
            clause: ClauseId::new(SegmentId::first(direction), index),
            source,
            target,
            text: text.into(),
            context,
            glossary: Vec::new(),
            strict_language: false,
        })
        .expect("request accepted");
    let cancel = CancellationToken::new();
    let (mut translation, mut first_token, mut completion_tokens) = (String::new(), None, 0);
    while let Some(item) = stream.next(&cancel) {
        match item.expect("no transport failure") {
            MtDelta::Text(t) => {
                first_token.get_or_insert_with(|| start.elapsed());
                translation.push_str(&t);
            }
            MtDelta::Done {
                completion_tokens: n,
                ..
            } => completion_tokens = n,
        }
    }
    Item {
        translation,
        first_token: first_token.unwrap_or_default(),
        total: start.elapsed(),
        completion_tokens,
    }
}

fn millis(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

#[test]
#[ignore = "needs an OpenAI-compatible server; see the module documentation"]
fn live_mt_llama_server() {
    let url = std::env::var("PRXVC_MT_LIVE_URL").unwrap_or_else(|_| "http://127.0.0.1:8090".into());
    let model = std::env::var("PRXVC_MT_LIVE_MODEL").unwrap_or_else(|_| "default".into());
    let template: PromptTemplate = std::env::var("PRXVC_MT_LIVE_TEMPLATE")
        .unwrap_or_else(|_| "gemma4".into())
        .parse()
        .expect("PRXVC_MT_LIVE_TEMPLATE is gemma4 or generic");

    let mut params = serde_json::Map::new();
    params.insert("temperature".into(), json!(0));
    params.insert("cache_prompt".into(), json!(true));
    // Gemma 4 and Qwen3.5 think unless told not to; per request, so the server needs no flag.
    params.insert(
        "chat_template_kwargs".into(),
        json!({"enable_thinking": false}),
    );
    let cfg = OpenAiChatConfig::new(&url, &model)
        .with_provider_id("live-llama-server")
        .with_template(template)
        .with_params(params)
        .with_timeouts(Timeouts {
            connect: Duration::from_secs(2),
            first_token: Duration::from_secs(30),
            idle: Duration::from_secs(15),
        });
    let reports = Arc::new(Reports::default());
    let mt = OpenAiChatMt::with_observer(cfg, reports.clone()).expect("configuration");
    println!("server {url}, model {model}, template {template}");

    let mut first_tokens = Vec::new();
    let mut rates = Vec::new();
    for (context_pairs, label) in [(0usize, "no context"), (2, "2 clauses of context")] {
        for (direction, source, target, set) in [
            (Direction::Outgoing, Lang::Zh, Lang::En, &ZH_EN),
            (Direction::Incoming, Lang::En, Lang::Zh, &EN_ZH),
        ] {
            println!("\n== {source} -> {target}, {label} ==");
            let mut history: Vec<(String, String)> = Vec::new();
            for (i, text) in set.iter().enumerate() {
                let context = history
                    .iter()
                    .rev()
                    .take(context_pairs)
                    .rev()
                    .cloned()
                    .collect();
                let item = translate_one(
                    &mt,
                    direction,
                    u16::try_from(i).unwrap_or(0),
                    source,
                    target,
                    text,
                    context,
                );
                println!(
                    "[{:>2}] {text}\n     -> {}\n     first token {:>5.0} ms, total {:>5.0} ms, {} tokens",
                    i + 1,
                    item.translation,
                    millis(item.first_token),
                    millis(item.total),
                    item.completion_tokens
                );
                assert!(
                    !item.translation.trim().is_empty(),
                    "empty translation of {text}"
                );
                let has_cjk = item.translation.chars().any(is_cjk);
                match target {
                    Lang::Zh => assert!(has_cjk, "not Chinese: {}", item.translation),
                    Lang::En => assert!(!has_cjk, "not English: {}", item.translation),
                }
                assert!(
                    digits_preserved(text, &item.translation),
                    "digits of {text:?} lost in {:?}",
                    item.translation
                );
                first_tokens.push(millis(item.first_token));
                let decode = item.total.saturating_sub(item.first_token).as_secs_f64();
                if item.completion_tokens > 1 && decode > 0.0 {
                    rates.push(f64::from(item.completion_tokens - 1) / decode);
                }
                history.push((text.to_string(), item.translation));
            }
        }
    }

    first_tokens.sort_by(f64::total_cmp);
    rates.sort_by(f64::total_cmp);
    println!(
        "\nfirst token ms: p50 {:.0}, p95 {:.0}, max {:.0}; decode tokens/s: p50 {:.1}, min {:.1}",
        percentile(&first_tokens, 0.5),
        percentile(&first_tokens, 0.95),
        first_tokens.last().copied().unwrap_or_default(),
        percentile(&rates, 0.5),
        rates.first().copied().unwrap_or_default()
    );
    let all = reports.0.lock().unwrap();
    let suspect: Vec<_> = all.iter().filter(|r| r.is_suspect()).collect();
    println!(
        "{} requests, {} flagged mt_suspect",
        all.len(),
        suspect.len()
    );
    for r in &suspect {
        println!(
            "  suspect {:?}: {:?} / stripped {:?}",
            r.suspects, r.text, r.stripped
        );
    }
}
