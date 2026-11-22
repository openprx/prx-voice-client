//! `call.*`, `voice.*` and `[pipeline]` as the parameters of one direction pipeline.

use std::time::Duration;

use prxvc_config::AppConfig;
use prxvc_media::{Direction, Lang};
use prxvc_ports::VoiceSelection;
use prxvc_providers::RecognitionMode as PlannedMode;
use prxvc_session::pipeline::{PipelineParams, RecognitionMode};
use prxvc_session::{
    BacklogParams, CommitParams, EarlyCommit, InterruptParams, OutputPolicy, Passthrough,
    RetranslateParams,
};

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

/// The early-commit behaviour in force: the configured one, held to the less eager of it and
/// `pipeline.early_commit_without_context` when the translation engine takes no conversation
/// context. An engine that translates every unit on its own gets the whole clause or nothing: a
/// fragment cut mid-clause is translated without its neighbours and comes back as a few odd words.
pub(crate) fn effective_early_commit(
    p: &prxvc_config::PipelineConfig,
    mt_uses_context: bool,
) -> EarlyCommit {
    let configured = early_commit(&p.early_commit);
    if mt_uses_context {
        return configured;
    }
    let limit = EarlyCommit::parse(&p.early_commit_without_context).unwrap_or_else(|| {
        tracing::warn!(
            key = "pipeline.early_commit_without_context",
            value = p.early_commit_without_context.as_str(),
            "unknown early commit setting; using \"punctuation\""
        );
        EarlyCommit::Punctuation
    });
    let eagerness = |m: EarlyCommit| match m {
        EarlyCommit::SegmentEnd => 0,
        EarlyCommit::Punctuation => 1,
        EarlyCommit::Chunk => 2,
    };
    if eagerness(limit) < eagerness(configured) {
        limit
    } else {
        configured
    }
}

/// Whether re-translation runs, and whether clause translations continue the sentence.
///
/// `pipeline.retranslation`: `"off"` never, `"on"` for every engine, `"auto"` (and any unknown
/// spelling, which is reported) for an engine that takes no conversation context. Never with the
/// early commit in force `segment_end`. Continuation (speaking only what follows the part of the
/// sentence already spoken) is for an engine without context only: context covers it otherwise.
pub(crate) fn effective_retranslation(
    p: &prxvc_config::PipelineConfig,
    mt_uses_context: bool,
    early: EarlyCommit,
) -> (bool, bool) {
    let wanted = match p.retranslation.as_str() {
        "off" => false,
        "on" => true,
        "auto" => !mt_uses_context,
        other => {
            tracing::warn!(
                key = "pipeline.retranslation",
                value = other,
                "unknown re-translation setting; using \"auto\""
            );
            !mt_uses_context
        }
    };
    let enabled = wanted && early != EarlyCommit::SegmentEnd;
    (enabled, enabled && !mt_uses_context)
}

/// `value` raised to `min`, reported when it had to be.
fn at_least(key: &'static str, value: u32, min: u32) -> u32 {
    if value < min {
        tracing::warn!(
            key,
            value,
            min,
            "value below its minimum; using the minimum"
        );
    }
    value.max(min)
}

/// `value` clamped to `0.0..=1.0`, reported when it had to be (not a number counts as 0.0).
fn ratio(key: &'static str, value: f32) -> f32 {
    let clamped = if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    };
    if clamped.to_bits() != value.to_bits() {
        tracing::warn!(
            key,
            value,
            clamped,
            "value outside 0.0 to 1.0; using the nearest"
        );
    }
    clamped
}

/// The re-translation parameters of a direction: `[pipeline]` keys with their minimums applied,
/// switched on or off by [`effective_retranslation`].
pub(crate) fn retranslate_params(
    p: &prxvc_config::PipelineConfig,
    mt_uses_context: bool,
    early: EarlyCommit,
) -> RetranslateParams {
    let (enabled, continuation) = effective_retranslation(p, mt_uses_context, early);
    RetranslateParams {
        enabled,
        continuation,
        agreement: at_least("pipeline.retranslate_agreement", p.retranslate_agreement, 1),
        lookahead: at_least("pipeline.retranslate_lookahead", p.retranslate_lookahead, 1) as usize,
        source_agreement: at_least(
            "pipeline.retranslate_source_agreement",
            p.retranslate_source_agreement,
            2,
        ) as usize,
        source_guard: p.retranslate_source_guard as usize,
        min_chars: at_least("pipeline.retranslate_min_chars", p.retranslate_min_chars, 1) as usize,
        min_words: at_least("pipeline.retranslate_min_words", p.retranslate_min_words, 1) as usize,
        interval: ms(p.retranslate_interval_ms),
        align_max_ratio: ratio(
            "pipeline.retranslate_align_max_ratio",
            p.retranslate_align_max_ratio,
        ),
    }
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

/// The parameters of the pipeline of `direction`, translating `source` into `target` with `voice`,
/// for a translation engine that uses the conversation context.
#[cfg(test)]
pub(crate) fn pipeline_params(
    config: &AppConfig,
    direction: Direction,
    langs: (Lang, Lang),
    voice: VoiceSelection,
    mode: PlannedMode,
) -> PipelineParams {
    pipeline_params_for_engine(config, direction, langs, voice, mode, true)
}

/// The parameters of the pipeline of `direction`, translating `source` into `target` with `voice`;
/// `mt_uses_context` is the translation provider's declared `supports_context`.
pub(crate) fn pipeline_params_for_engine(
    config: &AppConfig,
    direction: Direction,
    (source, target): (Lang, Lang),
    voice: VoiceSelection,
    mode: PlannedMode,
    mt_uses_context: bool,
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
        early: effective_early_commit(p, mt_uses_context),
        chunk_chars: p.early_chunk_chars as usize,
        chunk_words: p.early_chunk_words as usize,
        early_max_wait: ms(p.early_max_wait_ms),
        split_target_clauses: p.split_target_clauses,
        ..CommitParams::default()
    };
    params.retranslate = retranslate_params(p, mt_uses_context, params.commit.early);
    params.interrupt = InterruptParams {
        barge_in: config.call.barge_in,
        max_lag: ms(config.call.barge_in_max_lag_ms),
    };
    params.output = output_policy(config, direction);
    params.recognition_mode = recognition_mode(mode);
    params.vad_threshold = p.vad_threshold;
    params.min_speech = ms(p.min_speech_ms);
    params.min_speech_dbfs = p.min_speech_dbfs;
    params.pad = ms(p.pad_ms);
    params.drop_filler = p.drop_filler;
    params.skip_source_in_target = p.skip_source_in_target;
    params.retry_wrong_language = p.retry_wrong_language;
    params.mt_context_clauses = p.mt_context_clauses as usize;
    params.backlog = BacklogParams {
        enabled: p.backlog_skipping,
        max_backlog: ms(p.max_backlog_ms),
        max_lag: ms(p.max_lag_ms),
    };
    params.jitter_buffer = ms(p.jitter_buffer_ms);
    params.catch_up = prxvc_session::CatchUpParams {
        enabled: p.catch_up,
        after: ms(p.catch_up_after_ms),
        max_speed: p.catch_up_max_speed,
    };
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
        assert_eq!(params.jitter_buffer, Duration::from_millis(100));
        assert_eq!(params.backlog, BacklogParams::default());
        assert_eq!(params.catch_up, prxvc_session::CatchUpParams::default());
    }

    #[test]
    fn the_catch_up_keys_are_read() {
        let mut config = AppConfig::default();
        config.pipeline.catch_up = false;
        config.pipeline.catch_up_after_ms = 3_500;
        config.pipeline.catch_up_max_speed = 1.1;
        let params = pipeline_params(
            &config,
            Direction::Incoming,
            (Lang::En, Lang::Zh),
            voice(),
            PlannedMode::TwoPass,
        );
        assert!(!params.catch_up.enabled);
        assert_eq!(params.catch_up.after, Duration::from_millis(3_500));
        assert_eq!(params.catch_up.max_speed, 1.1);
    }

    #[test]
    fn the_filler_and_level_gates_are_on_by_default_and_read_from_the_keys() {
        let params = pipeline_params(
            &AppConfig::default(),
            Direction::Outgoing,
            (Lang::Zh, Lang::En),
            voice(),
            PlannedMode::TwoPass,
        );
        assert!(params.drop_filler);
        assert!(params.skip_source_in_target && params.retry_wrong_language);
        assert_eq!(params.min_speech_dbfs, -75.0);
        let mut config = AppConfig::default();
        config.pipeline.drop_filler = false;
        config.pipeline.min_speech_dbfs = -100.0;
        let params = pipeline_params(
            &config,
            Direction::Incoming,
            (Lang::En, Lang::Zh),
            voice(),
            PlannedMode::TwoPass,
        );
        assert!(!params.drop_filler);
        assert_eq!(params.min_speech_dbfs, -100.0);
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
    fn an_engine_without_context_never_gets_fragments_cut_mid_clause() {
        let mut config = AppConfig::default();
        let early = |config: &AppConfig, context: bool| {
            pipeline_params_for_engine(
                config,
                Direction::Outgoing,
                (Lang::Zh, Lang::En),
                voice(),
                PlannedMode::Streaming,
                context,
            )
            .commit
            .early
        };
        // The defaults: chunk for an engine with context, punctuation without.
        assert_eq!(early(&config, true), EarlyCommit::Chunk);
        assert_eq!(early(&config, false), EarlyCommit::Punctuation);
        // An explicit override restores chunk for every engine.
        config.pipeline.early_commit_without_context = "chunk".into();
        assert_eq!(early(&config, false), EarlyCommit::Chunk);
        // The less eager of the two settings wins.
        config.pipeline.early_commit = "segment_end".into();
        assert_eq!(early(&config, false), EarlyCommit::SegmentEnd);
        assert_eq!(early(&config, true), EarlyCommit::SegmentEnd);
        config.pipeline.early_commit = "chunk".into();
        config.pipeline.early_commit_without_context = "segment_end".into();
        assert_eq!(early(&config, false), EarlyCommit::SegmentEnd);
        assert_eq!(early(&config, true), EarlyCommit::Chunk);
        // An unknown spelling is reported and means punctuation.
        config.pipeline.early_commit_without_context = "often".into();
        assert_eq!(early(&config, false), EarlyCommit::Punctuation);
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

    fn retranslation(setting: &str, mt_uses_context: bool, early: EarlyCommit) -> (bool, bool) {
        let p = prxvc_config::PipelineConfig {
            retranslation: setting.into(),
            ..Default::default()
        };
        effective_retranslation(&p, mt_uses_context, early)
    }

    #[test]
    fn auto_runs_for_an_engine_without_context_and_continues_the_sentence() {
        assert_eq!(
            retranslation("auto", false, EarlyCommit::Punctuation),
            (true, true)
        );
        assert_eq!(
            retranslation("auto", true, EarlyCommit::Chunk),
            (false, false)
        );
    }

    #[test]
    fn on_runs_for_every_engine_and_continues_only_without_context() {
        assert_eq!(retranslation("on", false, EarlyCommit::Chunk), (true, true));
        assert_eq!(retranslation("on", true, EarlyCommit::Chunk), (true, false));
    }

    #[test]
    fn off_never_runs() {
        for context in [false, true] {
            assert_eq!(
                retranslation("off", context, EarlyCommit::Chunk),
                (false, false)
            );
        }
    }

    #[test]
    fn nothing_runs_while_units_are_committed_only_at_the_segment_end() {
        for setting in ["auto", "on"] {
            assert_eq!(
                retranslation(setting, false, EarlyCommit::SegmentEnd),
                (false, false)
            );
        }
    }

    #[test]
    fn an_unknown_spelling_means_auto() {
        assert_eq!(
            retranslation("sometimes", false, EarlyCommit::Punctuation),
            (true, true)
        );
        assert_eq!(
            retranslation("sometimes", true, EarlyCommit::Chunk),
            (false, false)
        );
    }

    #[test]
    fn the_default_configuration_turns_re_translation_on_for_the_system_engine_only() {
        let config = AppConfig::default();
        let params = |context| {
            pipeline_params_for_engine(
                &config,
                Direction::Outgoing,
                (Lang::Zh, Lang::En),
                voice(),
                PlannedMode::Streaming,
                context,
            )
            .retranslate
        };
        let without_context = params(false);
        assert!(without_context.enabled && without_context.continuation);
        assert_eq!(
            RetranslateParams {
                enabled: false,
                continuation: false,
                ..without_context
            },
            RetranslateParams::disabled(),
            "the numbers are the documented defaults"
        );
        assert!(!params(true).enabled);
    }

    #[test]
    fn the_re_translation_keys_are_read_and_held_to_their_minimums() {
        let mut p = prxvc_config::PipelineConfig {
            retranslate_agreement: 1,
            retranslate_lookahead: 3,
            retranslate_source_agreement: 3,
            retranslate_source_guard: 0,
            retranslate_min_chars: 5,
            retranslate_min_words: 4,
            retranslate_interval_ms: 250,
            retranslate_align_max_ratio: 0.3,
            ..Default::default()
        };
        let r = retranslate_params(&p, false, EarlyCommit::Punctuation);
        assert_eq!(
            (r.agreement, r.lookahead, r.source_agreement, r.source_guard),
            (1, 3, 3, 0)
        );
        assert_eq!((r.min_chars, r.min_words), (5, 4));
        assert_eq!(r.interval, Duration::from_millis(250));
        assert_eq!(r.align_max_ratio, 0.3);

        p.retranslate_agreement = 0;
        p.retranslate_lookahead = 0;
        p.retranslate_source_agreement = 1;
        p.retranslate_min_chars = 0;
        p.retranslate_min_words = 0;
        let r = retranslate_params(&p, false, EarlyCommit::Punctuation);
        assert_eq!(
            (r.agreement, r.lookahead, r.source_agreement),
            (1, 1, 2),
            "clamped"
        );
        assert_eq!((r.min_chars, r.min_words), (1, 1));

        for (given, held) in [
            (-0.2, 0.0),
            (1.7, 1.0),
            (f32::NAN, 0.0),
            (0.0, 0.0),
            (1.0, 1.0),
        ] {
            p.retranslate_align_max_ratio = given;
            let r = retranslate_params(&p, false, EarlyCommit::Punctuation);
            assert_eq!(r.align_max_ratio, held, "{given}");
        }
    }
}
