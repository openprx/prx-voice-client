//! The latency table of the assistant harness.

use std::time::Duration;

/// What one utterance measured, from the end of the speech.
pub(crate) struct Row {
    pub id: String,
    pub reference: String,
    pub recognised: String,
    pub reply: String,
    pub speech_s: f64,
    pub to_final: Option<Duration>,
    pub to_first_token: Option<Duration>,
    pub to_first_audio: Option<Duration>,
    pub reply_audio_s: f64,
    pub failure: Option<String>,
    /// What happened instead of a reply (filler makes no turn); not a failure.
    pub note: Option<String>,
}

fn ms(d: Option<Duration>) -> String {
    d.map_or("-".to_owned(), |d| {
        format!("{:.0}", d.as_secs_f64() * 1000.0)
    })
}

fn median(mut v: Vec<f64>) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    v.sort_by(f64::total_cmp);
    Some(v[v.len() / 2])
}

/// The table, one line per utterance, then the medians; `end_silence_ms` is the silence that ends
/// a turn, shown separately because it is the user's pause, not a processing time.
pub(crate) fn table(rows: &[Row], end_silence_ms: u32) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "\n{:<18} {:>7} {:>11} {:>11} {:>11} {:>11} {:>7}  text\n",
        "utterance", "speech", "final (ms)", "w/o pause", "token (ms)", "audio (ms)", "reply"
    ));
    for r in rows {
        let without = r
            .to_final
            .map(|d| d.saturating_sub(Duration::from_millis(u64::from(end_silence_ms))));
        out.push_str(&format!(
            "{:<18} {:>6.1}s {:>11} {:>11} {:>11} {:>11} {:>6.1}s  {}\n",
            r.id,
            r.speech_s,
            ms(r.to_final),
            ms(without),
            ms(r.to_first_token),
            ms(r.to_first_audio),
            r.reply_audio_s,
            match (&r.failure, &r.note) {
                (Some(f), _) => format!("FAILED: {f}"),
                (None, Some(n)) => n.clone(),
                (None, None) => format!("heard: {} | reply: {}", r.recognised, r.reply),
            }
        ));
        if !r.reference.is_empty() {
            out.push_str(&format!("{:<18} reference: {}\n", "", r.reference));
        }
    }
    let col = |f: fn(&Row) -> Option<Duration>| {
        median(
            rows.iter()
                .filter_map(|r| f(r).map(|d| d.as_secs_f64() * 1000.0))
                .collect(),
        )
    };
    let m = |v: Option<f64>| v.map_or("-".to_owned(), |v| format!("{v:.0}"));
    let final_ms = col(|r| r.to_final);
    out.push_str(&format!(
        "\nmedian over {} utterance(s), from the end of the speech:\n  final text            {} ms (of which the end-of-turn pause {} ms: {} ms of processing)\n  first reply token     {} ms\n  first reply audio     {} ms\n",
        rows.len(),
        m(final_ms),
        end_silence_ms,
        m(final_ms.map(|v| (v - f64::from(end_silence_ms)).max(0.0))),
        m(col(|r| r.to_first_token)),
        m(col(|r| r.to_first_audio)),
    ));
    out
}
