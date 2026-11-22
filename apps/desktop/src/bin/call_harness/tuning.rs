//! Command-line overrides of the latency tuning keys (`--hold-max-words`, `--hold-max-chars`,
//! `--jitter-ms`) and the rows that tell held clauses from streamed ones.

use prxvc_adapters::tts::qwentts::HoldLimit;
use prxvc_adapters::tts::qwentts::cap::cap_frames;
use prxvc_config::PipelineConfig;
use prxvc_media::Lang;
use prxvc_ports::TraceStage as S;

use crate::report::{UtteranceRun, median_p95};

/// The overrides given on the command line.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Overrides {
    hold_words: Option<u32>,
    hold_chars: Option<u32>,
    jitter_ms: Option<u32>,
}

impl Overrides {
    /// Take the value of `flag` when it is one of the tuning flags. `None`: not a tuning flag.
    pub(crate) fn set(&mut self, flag: &str, value: Option<String>) -> Option<Result<(), String>> {
        let slot = match flag {
            "--hold-max-words" => &mut self.hold_words,
            "--hold-max-chars" => &mut self.hold_chars,
            "--jitter-ms" => &mut self.jitter_ms,
            _ => return None,
        };
        Some(match value.and_then(|v| v.parse().ok()) {
            Some(n) => {
                *slot = Some(n);
                Ok(())
            }
            None => Err(format!("{flag} needs a whole number")),
        })
    }

    /// Apply them to the pipeline section of the run's configuration.
    pub(crate) fn apply(&self, p: &mut PipelineConfig) {
        p.tts_hold_max_words = self.hold_words.unwrap_or(p.tts_hold_max_words);
        p.tts_hold_max_chars = self.hold_chars.unwrap_or(p.tts_hold_max_chars);
        p.jitter_buffer_ms = self.jitter_ms.unwrap_or(p.jitter_buffer_ms);
    }
}

/// The Chinese or English set a name stands for.
pub(crate) fn lang_of(set: &str) -> Option<Lang> {
    match set {
        "zh" => Some(Lang::Zh),
        "en" => Some(Lang::En),
        _ => None,
    }
}

/// Whether the clause was held back by the synthesiser: its text and cap against the rule in force.
fn is_held(text: &str, lang: Lang, hold: HoldLimit) -> bool {
    hold.holds(text, lang, cap_frames(text, lang))
}

/// Rows of held versus streamed clauses: counts, request to first chunk and commit to first
/// translated audio, with the median and the p95 of each group.
pub(crate) fn table(runs: &[UtteranceRun], hold: HoldLimit, lang: Lang) -> String {
    let mut out = format!(
        "| held versus streamed clauses (limit {} words, {} characters; 0 is the cap rule) | n | median | p95 |\n|---|---|---|---|\n",
        hold.max_words, hold.max_chars
    );
    for (name, want) in [("held", true), ("streamed", false)] {
        let (mut first_chunk, mut audio, mut count) = (Vec::new(), Vec::new(), 0usize);
        for c in runs.iter().flat_map(|r| &r.clauses) {
            let Some(text) = c
                .texts
                .iter()
                .find(|(s, _)| *s == S::TtsRequestSent)
                .map(|(_, t)| t.as_str())
            else {
                continue;
            };
            if is_held(text, lang, hold) != want {
                continue;
            }
            count += 1;
            first_chunk.extend(c.ms(S::TtsRequestSent, S::TtsFirstChunk));
            audio.extend(
                c.ms(S::ClauseCommitted, S::PlayoutStart)
                    .map(|v| v / 1000.0),
            );
        }
        out.push_str(&format!("| {name} clauses (count) | {count} | | |\n"));
        for (label, values, unit) in [
            ("TTS request -> first chunk", &first_chunk, "ms"),
            ("commit -> first translated audio", &audio, "s"),
        ] {
            if let Some((m, p)) = median_p95(values) {
                out.push_str(&format!(
                    "| {name}: {label} ({unit}) | {} | {m:.2} | {p:.2} |\n",
                    values.len()
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flags_override_the_keys_and_other_flags_are_not_taken() {
        let mut o = Overrides::default();
        assert!(o.set("--count", Some("3".into())).is_none());
        o.set("--hold-max-words", Some("0".into()))
            .unwrap()
            .unwrap();
        o.set("--jitter-ms", Some("80".into())).unwrap().unwrap();
        assert!(
            o.set("--hold-max-chars", Some("x".into()))
                .unwrap()
                .is_err()
        );
        assert!(o.set("--hold-max-chars", None).unwrap().is_err());
        let mut p = PipelineConfig::default();
        o.apply(&mut p);
        assert_eq!(
            (
                p.tts_hold_max_words,
                p.tts_hold_max_chars,
                p.jitter_buffer_ms
            ),
            (0, 4, 80)
        );
    }

    #[test]
    fn the_classification_follows_the_limit() {
        let rule = HoldLimit::clamped(3, 4);
        assert!(is_held("see you later", Lang::En, rule));
        assert!(!is_held("we should go there", Lang::En, rule));
        assert!(is_held("we should go there", Lang::En, HoldLimit::LEGACY));
        assert!(table(&[], rule, Lang::En).contains("held clauses (count) | 0"));
    }
}
