//! `call.*`, `voice.*` and `[pipeline]` as the parameters of one direction pipeline.

use std::time::Duration;

use prxvc_config::AppConfig;
use prxvc_media::{Direction, Lang};
use prxvc_ports::VoiceSelection;
use prxvc_providers::RecognitionMode as PlannedMode;
use prxvc_session::pipeline::{PipelineParams, RecognitionMode};
use prxvc_session::{BacklogParams, CommitParams, EarlyCommit, InterruptParams, OutputPolicy, Passthrough};

fn ms(n: u32) -> Duration {
    Duration::from_millis(u64::from(n))
}

fn secs(n: u32) -> Duration {
    Duration::from_secs(u64::from(n))
}

/// A configured passthrough choice (`"original"` or `"silence"`); anything else is reported and means `original`.
fn passthrough(setting: &str, key: &str) -> Passthrough {
    match setting {
        "original" => Passthrough::Original,
        "silence" => Passthrough::Silence,
        other => {
            tracing::warn!(
                key,
                value = other,
                "unknown passthrough setting; using \"original\""
            );
            Passthrough::Original
        }
    }
}

/// The configured early-commit behaviour; an unknown spelling is reported and means the default.
fn early_commit(setting: &str) -> EarlyCommit {
    EarlyCommit::parse(setting).unwrap_or_else(|| {
        tracing::warn!(
            key = "pipeline.early_commit",
            value = setting,
            "unknown early commit setting; using \"chunk\""
        );
        CommitParams::default().early
    })
}

/// What the listener of each direction hears while it is paused or failed (`call.*_when_paused`, `call.*_on_failure`).
pub(crate) fn output_policy(config: &AppConfig, direction: Direction) -> OutputPolicy {
    let call = &config.call;
    match direction {
        Direction::Outgoing => OutputPolicy {
            when_paused: passthrough(&call.outgoing_when_paused, "call.outgoing_when_paused"),
            on_failure: passthrough(&call.outgoing_on_failure, "call.outgoing_on_failure"),
        },
        Direction::Incoming => OutputPolicy {
            when_paused: passthrough(&call.incoming_when_paused, "call.incoming_when_paused"),
            on_failure: passthrough(&call.incoming_on_failure, "call.incoming_on_failure"),
        },
    }
}

fn recognition_mode(mode: PlannedMode) -> RecognitionMode {
    match mode {
        PlannedMode::Streaming => RecognitionMode::Streaming,
        PlannedMode::TwoPass => RecognitionMode::TwoPass,
        PlannedMode::Segmented => RecognitionMode::Segmented,
    }
}

/// The parameters of the pipeline of `direction`, translating `source` into `target` with `voice`.
pub(crate) fn pipeline_params(
    config: &AppConfig,
    direction: Direction,
    (source, target): (Lang, Lang),
    voice: VoiceSelection,
    mode: PlannedMode,
) -> PipelineParams {
    let p = &config.pipeline;
    let mut params = PipelineParams::new(direction, source, target, voice);
    params.commit = CommitParams {
        clause_pause: ms(p.clause_pause_ms),
        segment_end_pause: ms(p.segment_end_pause_ms),
        long_segment_after: secs(p.long_segment_after_s),
        long_segment_pause: ms(p.long_segment_pause_ms),
        max_clause: secs(p.max_clause_s),
        max_segment: secs(p.max_segment_s),
        early: early_commit(&p.early_commit),
        chunk_chars: p.early_chunk_chars as usize,
        chunk_words: p.early_chunk_words as usize,
        early_max_wait: ms(p.early_max_wait_ms),
        split_target_clauses: p.split_target_clauses,
        ..CommitParams::default()
    };
    params.interrupt = InterruptParams {
        barge_in: config.call.barge_in,
        max_lag: ms(config.call.barge_in_max_lag_ms),
    };
    params.output = output_policy(config, direction);
    params.recognition_mode = recognition_mode(mode);
    params.vad_threshold = p.vad_threshold;
    params.min_speech = ms(p.min_speech_ms);
    params.pad = ms(p.pad_ms);
    params.mt_context_clauses = p.mt_context_clauses as usize;
    params.backlog = BacklogParams {
        enabled: p.backlog_skipping,
        max_backlog: ms(p.max_backlog_ms),
        max_lag: ms(p.max_lag_ms),
    };
    params.jitter_buffer = ms(p.jitter_buffer_ms);
    params
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice() -> VoiceSelection {
        VoiceSelection::Preset { name: "p".into() }
    }

    #[test]
    fn the_default_configuration_gives_the_documented_defaults() {
        let params = pipeline_params(
            &AppConfig::default(),
            Direction::Outgoing,
            (Lang::Zh, Lang::En),
            voice(),
            PlannedMode::TwoPass,
        );
        assert_eq!(params.commit.clause_pause, Duration::from_millis(300));
        assert_eq!(params.commit.segment_end_pause, Duration::from_millis(600));
        assert_eq!(params.commit.max_clause, Duration::from_secs(8));
        assert_eq!(params.commit.early, EarlyCommit::Chunk);
        assert_eq!(params.commit.chunk_chars, 8);
        assert_eq!(params.commit.chunk_words, 6);
        assert_eq!(params.commit.early_max_wait, Duration::from_millis(2500));
        assert_eq!(params.recognition_mode, RecognitionMode::TwoPass);
        assert_eq!(params.output, OutputPolicy::default());
        assert!(params.interrupt.barge_in);
        assert_eq!(params.interrupt.max_lag, Duration::from_millis(3000));
        assert_eq!((params.source, params.target), (Lang::Zh, Lang::En));
        assert_eq!(params.jitter_buffer, Duration::from_millis(150));
        assert_eq!(params.backlog, BacklogParams::default());
    }

    #[test]
    fn the_backlog_keys_are_read() {
        let mut config = AppConfig::default();
        config.pipeline.backlog_skipping = false;
        config.pipeline.max_backlog_ms = 9_000;
        config.pipeline.max_lag_ms = 12_000;
        let params = pipeline_params(
            &config,
            Direction::Outgoing,
            (Lang::Zh, Lang::En),
            voice(),
            PlannedMode::Streaming,
        );
        assert!(!params.backlog.enabled);
        assert_eq!(params.backlog.max_backlog, Duration::from_millis(9_000));
        assert_eq!(params.backlog.max_lag, Duration::from_millis(12_000));
    }

    #[test]
    fn an_unknown_early_commit_setting_means_the_default() {
        let mut config = AppConfig::default();
        config.pipeline.early_commit = "sometimes".into();
        let params = pipeline_params(
            &config,
            Direction::Outgoing,
            (Lang::Zh, Lang::En),
            voice(),
            PlannedMode::Streaming,
        );
        assert_eq!(params.commit.early, CommitParams::default().early);
        for (spelling, want) in [
            ("segment_end", EarlyCommit::SegmentEnd),
            ("punctuation", EarlyCommit::Punctuation),
            ("chunk", EarlyCommit::Chunk),
        ] {
            config.pipeline.early_commit = spelling.into();
            let p = pipeline_params(
                &config,
                Direction::Outgoing,
                (Lang::Zh, Lang::En),
                voice(),
                PlannedMode::Streaming,
            );
            assert_eq!(p.commit.early, want);
        }
    }

    #[test]
    fn configured_values_and_the_direction_s_own_policy_are_read() {
        let mut config = AppConfig::default();
        config.pipeline.early_commit = "punctuation".into();
        config.pipeline.clause_pause_ms = 250;
        config.call.barge_in = false;
        config.call.incoming_on_failure = "silence".into();
        let outgoing = pipeline_params(
            &config,
            Direction::Outgoing,
            (Lang::Zh, Lang::En),
            voice(),
            PlannedMode::Streaming,
        );
        let incoming = pipeline_params(
            &config,
            Direction::Incoming,
            (Lang::En, Lang::Zh),
            voice(),
            PlannedMode::Streaming,
        );
        assert_eq!(outgoing.commit.early, EarlyCommit::Punctuation);
        assert_eq!(outgoing.commit.clause_pause, Duration::from_millis(250));
        assert!(!outgoing.interrupt.barge_in);
        assert_eq!(outgoing.output.on_failure, Passthrough::Original);
        assert_eq!(incoming.output.on_failure, Passthrough::Silence);
        assert_eq!(outgoing.recognition_mode, RecognitionMode::Streaming);
    }
}
