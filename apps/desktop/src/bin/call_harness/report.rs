//! From the stage timestamps of the clauses of each utterance to the latency table of the prototype's report (`bench/e2e/REPORT.md`), and the text comparison of the round trip.

use std::collections::BTreeMap;

use prxvc_media::Lang;
use prxvc_ports::TraceStage as S;

use crate::call_kit::trace::ClauseTimeline;

/// The result of the round trip: the translated speech as recorded from the virtual microphone and recognised again.
#[derive(Clone, Debug)]
pub(crate) struct RoundTrip {
    pub recognised: String,
    /// Word error rate (English) or character error rate (Chinese) against the text the synthesiser was asked to speak.
    pub error_rate: f64,
    /// Loudest sample of the recording.
    pub peak: f32,
    pub seconds: f64,
}

/// Everything measured for one utterance.
#[derive(Clone, Debug)]
pub(crate) struct UtteranceRun {
    pub id: String,
    pub reference: String,
    /// Speech start and end in the file as the offline VAD pass found them, nanoseconds on the pipeline clock.
    pub speech_start_ns: f64,
    pub speech_end_ns: f64,
    pub clauses: Vec<ClauseTimeline>,
    pub source_text: String,
    pub translation: String,
    pub spoken: String,
    /// Seconds of translated audio produced, per clause in order.
    pub audio_seconds: Vec<f64>,
    pub underruns: u64,
    pub runaway_synth: u64,
    pub mt_suspect: u64,
    pub clauses_failed: u64,
    /// Clauses skipped to keep up (backlog or stale).
    pub clauses_skipped: u64,
    /// The pipeline's counters at the end, as text.
    pub counters: String,
    pub round_trip: Option<RoundTrip>,
}

fn ns(t: prxvc_media::MonoTime) -> f64 {
    t.as_nanos() as f64
}

/// The figures of one utterance in the prototype's terms, in seconds. Missing stages leave a figure out.
pub(crate) fn figures(run: &UtteranceRun) -> Vec<(&'static str, f64)> {
    let mut out = Vec::new();
    let secs = |a: f64, b: f64| (b - a) / 1e9;
    let first = run.clauses.first();
    let at = |c: &ClauseTimeline, s| c.at(s).map(ns);
    if let Some(p) = run
        .clauses
        .iter()
        .filter_map(|c| at(c, S::AsrPartial))
        .reduce(f64::min)
    {
        out.push((
            "speech start -> first partial (s)",
            secs(run.speech_start_ns, p),
        ));
    }
    if let Some(f) = run
        .clauses
        .iter()
        .filter_map(|c| at(c, S::AsrFinalReady))
        .reduce(f64::max)
    {
        out.push((
            "speech end -> last final ASR text (s)",
            secs(run.speech_end_ns, f),
        ));
    }
    if let Some(c) = first {
        if let Some(v) = c.ms(S::MtRequestSent, S::MtFirstToken) {
            out.push(("MT time to first token (s)", v / 1000.0));
        }
        if let Some(v) = c.ms(S::MtRequestSent, S::MtDone) {
            out.push(("MT time to first clause complete (s)", v / 1000.0));
        }
        if let Some(v) = c.ms(S::TtsRequestSent, S::TtsFirstChunk) {
            out.push(("TTS time to first audio (s)", v / 1000.0));
        }
        if let Some(v) = c.ms(S::ClauseCommitted, S::PlayoutStart) {
            out.push(("commit -> first translated audio (s)", v / 1000.0));
        }
        if let Some(p) = at(c, S::PlayoutStart) {
            out.push((
                "speech START -> first translated audio (s)",
                secs(run.speech_start_ns, p),
            ));
            out.push((
                "speech END -> first translated audio (s)",
                secs(run.speech_end_ns, p),
            ));
        }
    }
    if let Some(e) = run
        .clauses
        .iter()
        .filter_map(|c| at(c, S::PlayoutEnd))
        .reduce(f64::max)
    {
        out.push((
            "speech END -> last translated audio finished (s)",
            secs(run.speech_end_ns, e),
        ));
    }
    // Silence between clauses of one utterance.
    for pair in run.clauses.windows(2) {
        if let (Some(end), Some(start)) =
            (at(&pair[0], S::PlayoutEnd), at(&pair[1], S::PlayoutStart))
        {
            out.push(("silence gap between clauses (s)", secs(end, start).max(0.0)));
        }
    }
    // Synthesis real-time factor per clause.
    for (c, audio) in run.clauses.iter().zip(&run.audio_seconds) {
        if let (Some(ms), true) = (c.ms(S::TtsRequestSent, S::TtsDone), *audio > 0.0) {
            out.push((
                "TTS real-time factor (synth time / audio)",
                ms / 1000.0 / audio,
            ));
        }
    }
    out
}

/// Median and 95th percentile (nearest rank) of `values`.
pub(crate) fn median_p95(values: &[f64]) -> Option<(f64, f64)> {
    if values.is_empty() {
        return None;
    }
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = if v.len() % 2 == 1 {
        v[v.len() / 2]
    } else {
        (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
    };
    let rank = ((0.95 * v.len() as f64).ceil() as usize).clamp(1, v.len());
    Some((median, v[rank - 1]))
}

/// The table of one run: one row per figure with the number of values, the median and the p95, then the counts.
pub(crate) fn table(runs: &[UtteranceRun]) -> String {
    let mut by_name: BTreeMap<&'static str, Vec<f64>> = BTreeMap::new();
    let mut order: Vec<&'static str> = Vec::new();
    for run in runs {
        for (name, v) in figures(run) {
            if !by_name.contains_key(name) {
                order.push(name);
            }
            by_name.entry(name).or_default().push(v);
        }
    }
    let mut out = String::from("| metric | n | median | p95 |\n|---|---|---|---|\n");
    for name in order {
        let values = &by_name[name];
        if let Some((m, p)) = median_p95(values) {
            out.push_str(&format!(
                "| {name} | {} | {m:.2} | {p:.2} |\n",
                values.len()
            ));
        }
    }
    let n = runs.len();
    let with = |f: fn(&UtteranceRun) -> bool| runs.iter().filter(|r| f(r)).count();
    out.push_str(&format!(
        "| items with an output underrun | {n} | {} | |\n| items with a clause that hit the runaway guard | {n} | {} | |\n| items with a suspect translation | {n} | {} | |\n| items with a failed clause | {n} | {} | |\n| items with no translated audio | {n} | {} | |\n| items with a clause skipped to keep up | {n} | {} | |\n",
        with(|r| r.underruns > 0),
        with(|r| r.runaway_synth > 0),
        with(|r| r.mt_suspect > 0),
        with(|r| r.clauses_failed > 0),
        with(|r| r.audio_seconds.iter().sum::<f64>() <= 0.0),
        with(|r| r.clauses_skipped > 0),
    ));
    out
}

/// Lower-cased units for comparison: characters for Chinese, words for the rest.
fn units(text: &str, lang: Lang) -> Vec<String> {
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c.is_whitespace() {
                c.to_ascii_lowercase()
            } else {
                ' '
            }
        })
        .collect();
    if lang == Lang::Zh {
        clean
            .chars()
            .filter(|c| !c.is_whitespace())
            .map(|c| c.to_string())
            .collect()
    } else {
        clean.split_whitespace().map(str::to_owned).collect()
    }
}

/// Edit distance over units divided by the length of the reference.
pub(crate) fn error_rate(reference: &str, hypothesis: &str, lang: Lang) -> f64 {
    let (r, h) = (units(reference, lang), units(hypothesis, lang));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for (i, ru) in r.iter().enumerate() {
        let mut cur = vec![i + 1];
        for (j, hu) in h.iter().enumerate() {
            let sub = prev[j] + usize::from(ru != hu);
            cur.push(sub.min(prev[j + 1] + 1).min(cur[j] + 1));
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_and_p95_use_nearest_rank() {
        assert_eq!(median_p95(&[]), None);
        assert_eq!(median_p95(&[3.0]), Some((3.0, 3.0)));
        let v: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(median_p95(&v), Some((10.5, 19.0)));
    }

    #[test]
    fn error_rates_count_words_or_characters() {
        assert_eq!(
            error_rate("The weather is nice.", "the weather is nice", Lang::En),
            0.0
        );
        assert!((error_rate("a b c d", "a b x d", Lang::En) - 0.25).abs() < 1e-9);
        assert!((error_rate("今天天气很好", "今天天气很", Lang::Zh) - 1.0 / 6.0).abs() < 1e-9);
        assert_eq!(error_rate("", "", Lang::En), 0.0);
        assert_eq!(error_rate("", "x", Lang::En), 1.0);
    }
}
