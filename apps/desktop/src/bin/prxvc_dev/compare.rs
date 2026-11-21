//! `prxvc-dev translate-compare [--out FILE]`: the same sentences through system translation and
//! the language model, side by side, with the time each takes. Both engines are built by the
//! provider registry exactly as a call builds them; every sentence is a request of its own with no
//! context (the system engine has none, so the comparison gives the model none either). Needs the
//! language packs and a language-model server answering on the configured address (the one
//! the owner already runs); it starts nothing and stops nothing.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtRequest, Translator};

use crate::call_kit::config::load_current;
use crate::call_kit::providers::ProviderHost;
use crate::sentences::{EN, ZH};

/// One translation: the text and the times.
struct Done {
    text: String,
    first: Duration,
    total: Duration,
}

fn translate(mt: &dyn Translator, source: Lang, target: Lang, text: &str) -> Result<Done, String> {
    let started = Instant::now();
    let req = MtRequest {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
        source,
        target,
        text: text.to_owned(),
        context: Vec::new(),
        glossary: Vec::new(),
        strict_language: false,
    };
    let mut stream = mt.translate(req).map_err(|e| e.to_string())?;
    let cancel = CancellationToken::new();
    let (mut out, mut first) = (String::new(), None);
    while let Some(delta) = stream.next(&cancel) {
        match delta.map_err(|e| e.to_string())? {
            MtDelta::Text(t) => {
                first.get_or_insert(started.elapsed());
                out.push_str(&t);
            }
            MtDelta::Done { .. } => {}
        }
    }
    Ok(Done {
        text: out.trim().to_owned(),
        first: first.unwrap_or_else(|| started.elapsed()),
        total: started.elapsed(),
    })
}

fn build(engine: &str, source: Lang, target: Lang) -> Result<Box<dyn Translator>, String> {
    let mut config = load_current();
    config.call.translation_engine = engine.to_owned();
    let registry = ProviderHost::registry_for(&config, None).map_err(|f| f.detail)?;
    let resolved = registry.resolve_mt(source, target).map_err(|p| {
        p.iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("; ")
    })?;
    let mt = registry
        .build_mt(&resolved.primary)
        .map_err(|e| format!("{engine}: {e}"))?;
    // The registry must outlive the engine's handles: leak it for the life of this tool.
    std::mem::forget(registry);
    Ok(mt)
}

fn cell(text: &str) -> String {
    text.replace('|', "\\|").replace('\n', " ")
}

fn ms(d: Duration) -> u128 {
    d.as_millis()
}

fn percentile(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).round() as usize;
    sorted[idx]
}

/// One direction's section of the document.
fn section(
    title: &str,
    source: Lang,
    target: Lang,
    sentences: &[(&str, &str)],
) -> Result<String, String> {
    let system = build("system", source, target)?;
    let llm = build("llm", source, target)?;
    // The cold first call of each engine is not part of the sample.
    let warm = sentences[0].1;
    translate(system.as_ref(), source, target, warm)?;
    translate(llm.as_ref(), source, target, warm)?;
    let mut out = format!(
        "### {title}\n\n| # | kind | source | system translation | language model | system ms | model first / total ms |\n|---|---|---|---|---|---|---|\n"
    );
    let (mut sys_ms, mut llm_first, mut llm_total) = (Vec::new(), Vec::new(), Vec::new());
    for (i, (kind, text)) in sentences.iter().enumerate() {
        let s = translate(system.as_ref(), source, target, text)?;
        let l = translate(llm.as_ref(), source, target, text)?;
        sys_ms.push(ms(s.total));
        llm_first.push(ms(l.first));
        llm_total.push(ms(l.total));
        let _ = writeln!(
            out,
            "| {} | {} | {} | {} | {} | {} | {} / {} |",
            i + 1,
            kind,
            cell(text),
            cell(&s.text),
            cell(&l.text),
            ms(s.total),
            ms(l.first),
            ms(l.total)
        );
    }
    for v in [&mut sys_ms, &mut llm_first, &mut llm_total] {
        v.sort_unstable();
    }
    let _ = write!(
        out,
        "\nTranslation time per sentence, {} sentences, warm (median / p95 ms): system {} / {}, language model first token {} / {}, language model complete {} / {}.\n\n",
        sentences.len(),
        percentile(&sys_ms, 0.5),
        percentile(&sys_ms, 0.95),
        percentile(&llm_first, 0.5),
        percentile(&llm_first, 0.95),
        percentile(&llm_total, 0.5),
        percentile(&llm_total, 0.95),
    );
    Ok(out)
}

pub(crate) fn run(args: &[String]) -> Result<bool, String> {
    let out_path = match args {
        [] => None,
        [flag, path] if flag == "--out" => Some(path.clone()),
        _ => return Err("usage: prxvc-dev translate-compare [--out FILE]".into()),
    };
    let mut doc = String::new();
    doc.push_str(&section(
        "Chinese to English (the outgoing direction)",
        Lang::Zh,
        Lang::En,
        ZH,
    )?);
    doc.push_str(&section(
        "English to Chinese (the incoming direction)",
        Lang::En,
        Lang::Zh,
        EN,
    )?);
    match out_path {
        Some(path) => std::fs::write(&path, &doc).map_err(|e| format!("{path}: {e}"))?,
        None => println!("{doc}"),
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sets_have_48_sentences_in_seven_kinds_each() {
        for set in [ZH, EN] {
            assert_eq!(set.len(), 48);
            let mut kinds: Vec<&str> = set.iter().map(|(k, _)| *k).collect();
            kinds.dedup();
            assert_eq!(kinds.len(), 7, "{kinds:?}");
            assert!(set.iter().all(|(_, t)| !t.trim().is_empty()));
        }
    }

    #[test]
    fn table_cells_never_break_the_table() {
        assert_eq!(cell("a|b\nc"), "a\\|b c");
    }

    #[test]
    fn percentiles_pick_ranked_values() {
        let v = [10, 20, 30, 40, 50];
        assert_eq!(percentile(&v, 0.5), 30);
        assert_eq!(percentile(&v, 0.95), 50);
        assert_eq!(percentile(&[], 0.5), 0);
    }
}
