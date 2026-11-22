//! The development options of re-translation (`--retranslate`, `--retranslate-agreement`, `--redecode-ms`), the line that says what is in force, and the units file of `--units-out`. The units file holds spoken text: it is bench material written where the operator asked, never an app log.

use std::path::Path;

use prxvc_config::PipelineConfig;
use prxvc_ports::TraceStage;
use prxvc_session::pipeline::PipelineParams;

use crate::report::{UtteranceRun, unit_cut};

/// The spelling of `--retranslate`; anything else is an error here, not a silent `auto`.
pub(crate) fn parse_mode(value: &str) -> Result<String, String> {
    match value {
        "auto" | "on" | "off" => Ok(value.to_owned()),
        other => Err(format!(
            "unknown retranslation mode {other} (auto, on or off)"
        )),
    }
}

/// Apply the options to the pipeline section of the run's configuration.
pub(crate) fn apply(
    p: &mut PipelineConfig,
    mode: Option<&str>,
    agreement: Option<u32>,
    redecode_ms: Option<u32>,
) {
    if let Some(mode) = mode {
        p.retranslation = mode.to_owned();
    }
    if let Some(n) = agreement {
        p.retranslate_agreement = n;
    }
    if let Some(ms) = redecode_ms {
        p.asr_redecode_interval_ms = ms;
    }
}

/// What the pipeline will do, next to the "early commit in force" line.
pub(crate) fn in_force_line(params: &PipelineParams) -> String {
    let on = |b: bool| if b { "on" } else { "off" };
    format!(
        "retranslation in force: {} (continuation {})",
        on(params.retranslate.enabled),
        on(params.retranslate.continuation)
    )
}

fn clean(text: &str) -> String {
    text.replace(['\t', '\n', '\r'], " ")
}

/// One line per unit: `id`, `segment.index`, the reason it was cut, whether its sentence goes on,
/// its source text and what was spoken for it; a unit whose translation diverged ends with the
/// trace text (the spoken translation and the new one).
pub(crate) fn lines(runs: &[UtteranceRun]) -> String {
    let mut out = String::new();
    for run in runs {
        for c in &run.clauses {
            let text = |stage: TraceStage| {
                c.texts
                    .iter()
                    .find(|(s, _)| *s == stage)
                    .map(|(_, t)| clean(t))
            };
            let (reason, continues) = unit_cut(c).unwrap_or(("-", false));
            let source = text(TraceStage::ClauseCommitted).unwrap_or_default();
            let spoken = text(TraceStage::TtsRequestSent)
                .or_else(|| text(TraceStage::MtDone))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}\t{}.{}\t{reason}\t{continues}\t{source}\t{spoken}",
                run.id,
                c.clause.segment().seq(),
                c.clause.index()
            ));
            if let Some(diverged) = text(TraceStage::ContinuationDiverged) {
                out.push_str(&format!("\tdiverged: {diverged}"));
            }
            out.push('\n');
        }
    }
    out
}

/// Write the units file.
pub(crate) fn write(path: &Path, runs: &[UtteranceRun]) -> Result<(), String> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(path, lines(runs)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use prxvc_media::{ClauseId, Direction, MonoTime, SegmentId};
    use prxvc_session::RetranslateParams;

    use super::*;
    use crate::call_kit::trace::ClauseTimeline;

    fn clause(index: u16, texts: &[(TraceStage, &str)]) -> ClauseTimeline {
        ClauseTimeline {
            clause: ClauseId::new(SegmentId::new(Direction::Incoming, 1).expect("seq"), index),
            stages: vec![(TraceStage::ClauseCommitted, MonoTime::from_millis(1))],
            texts: texts.iter().map(|(s, t)| (*s, (*t).to_owned())).collect(),
        }
    }

    fn run(clauses: Vec<ClauseTimeline>) -> UtteranceRun {
        UtteranceRun {
            id: "en000".into(),
            reference: String::new(),
            speech_start_ns: 0.0,
            speech_end_ns: 0.0,
            clauses,
            source_text: String::new(),
            recognised: String::new(),
            recognition_error: None,
            translation: String::new(),
            spoken: String::new(),
            audio_seconds: Vec::new(),
            underruns: 0,
            gap_ms: 0,
            worst_gap_ms: 0,
            sink_underruns: 0,
            runaway_synth: 0,
            mt_suspect: 0,
            clauses_failed: 0,
            clauses_skipped: 0,
            final_rewrites: 0,
            counters: String::new(),
            round_trip: None,
        }
    }

    #[test]
    fn the_options_of_the_run_are_parsed() {
        let args =
            |list: &[&str]| -> Vec<String> { list.iter().map(|s| (*s).to_owned()).collect() };
        let o = crate::parse(&args(&[
            "--retranslate",
            "off",
            "--retranslate-agreement",
            "1",
            "--redecode-ms",
            "300",
            "--units-out",
            "out/units.tsv",
        ]))
        .expect("parses");
        assert_eq!(o.retranslate.as_deref(), Some("off"));
        assert_eq!(
            (o.retranslate_agreement, o.redecode_ms),
            (Some(1), Some(300))
        );
        assert_eq!(o.units_out, Some(std::path::PathBuf::from("out/units.tsv")));
        let none = crate::parse(&args(&[])).expect("parses");
        assert!(none.retranslate.is_none() && none.units_out.is_none());
        assert!(crate::parse(&args(&["--retranslate", "maybe"])).is_err());
        assert!(crate::parse(&args(&["--retranslate-agreement", "x"])).is_err());
        assert!(crate::parse(&args(&["--redecode-ms"])).is_err());
    }

    #[test]
    fn the_modes_are_spelled_exactly() {
        assert_eq!(parse_mode("auto").as_deref(), Ok("auto"));
        assert_eq!(parse_mode("off").as_deref(), Ok("off"));
        assert!(parse_mode("On").is_err(), "no guessing at a spelling");
        assert!(parse_mode("").is_err());
    }

    #[test]
    fn the_options_reach_the_pipeline_section_and_leave_the_rest_alone() {
        let mut p = PipelineConfig::default();
        apply(&mut p, None, None, None);
        assert_eq!(p, PipelineConfig::default());
        apply(&mut p, Some("off"), Some(1), Some(300));
        assert_eq!(p.retranslation, "off");
        assert_eq!(p.retranslate_agreement, 1);
        assert_eq!(p.asr_redecode_interval_ms, 300);
        assert_eq!(
            p.retranslate_lookahead,
            PipelineConfig::default().retranslate_lookahead
        );
    }

    #[test]
    fn the_line_says_what_is_in_force() {
        let mut params = PipelineParams::new(
            Direction::Incoming,
            prxvc_media::Lang::En,
            prxvc_media::Lang::Zh,
            prxvc_ports::VoiceSelection::Preset { name: "p".into() },
        );
        assert_eq!(
            in_force_line(&params),
            "retranslation in force: off (continuation off)"
        );
        params.retranslate = RetranslateParams {
            enabled: true,
            continuation: true,
            ..RetranslateParams::disabled()
        };
        assert_eq!(
            in_force_line(&params),
            "retranslation in force: on (continuation on)"
        );
    }

    #[test]
    fn a_unit_is_one_line_and_a_divergence_adds_its_trace_text() {
        let a = clause(
            0,
            &[
                (TraceStage::UnitCut, "retranslation continues"),
                (TraceStage::ClauseCommitted, "we should\tmeet"),
                (TraceStage::TtsRequestSent, "我们应该见面"),
            ],
        );
        let b = clause(
            1,
            &[
                (TraceStage::UnitCut, "segment_end ends"),
                (TraceStage::ClauseCommitted, "at the office"),
                (TraceStage::MtDone, "在办公室"),
                (TraceStage::ContinuationDiverged, "我们应该 | 我们必须"),
            ],
        );
        let text = lines(&[run(vec![a, b])]);
        let rows: Vec<&str> = text.lines().collect();
        assert_eq!(
            rows[0],
            "en000\t1.0\tretranslation\ttrue\twe should meet\t我们应该见面"
        );
        assert_eq!(
            rows[1],
            "en000\t1.1\tsegment_end\tfalse\tat the office\t在办公室\tdiverged: 我们应该 | 我们必须"
        );
        assert_eq!(rows.len(), 2);
    }

    #[test]
    fn the_table_has_the_columns_of_the_rule() {
        let mut first = clause(0, &[(TraceStage::UnitCut, "retranslation continues")]);
        first
            .stages
            .push((TraceStage::RetranslationCut, MonoTime::from_millis(2)));
        let mut second = clause(1, &[(TraceStage::UnitCut, "segment_end ends")]);
        second.stages[0].1 = MonoTime::from_millis(900);
        second
            .stages
            .push((TraceStage::ContinuationDiverged, MonoTime::from_millis(901)));
        let r = run(vec![second, first]);
        assert_eq!(
            r.first_cut(),
            Some("retranslation"),
            "the earliest unit, not the first listed"
        );
        assert_eq!((r.retranslated(), r.diverged()), (1, 1));
        let table = crate::report::table(&[r]);
        for row in [
            "first_commit_s",
            "| retranslated (units cut by the rule per utterance) | 1 | 1.00 | 1.00 |",
            "| diverged (continuations translated alone per utterance) | 1 | 1.00 | 1.00 |",
            "first_cut (utterances by the reason of their first unit) | 1 | retranslation=1",
        ] {
            assert!(table.contains(row), "{row} missing from:\n{table}");
        }
    }

    #[test]
    fn a_unit_without_a_cut_record_has_dashes() {
        let text = lines(&[run(vec![clause(
            0,
            &[(TraceStage::ClauseCommitted, "x y")],
        )])]);
        assert_eq!(text, "en000\t1.0\t-\tfalse\tx y\t\n");
    }
}
