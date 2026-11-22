//! Parameters of one direction pipeline, shaped like the `[pipeline]` and `call.*` configuration keys.

use std::time::Duration;

use prxvc_media::{Direction, Lang};
use prxvc_ports::VoiceSelection;

use crate::backlog::BacklogParams;
use crate::commit_policy::{CommitParams, RetranslateParams};
use crate::interrupt::InterruptParams;
use crate::output::OutputPolicy;

/// `pipeline.recognition_mode` (pipeline-design 2.5.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecognitionMode {
    /// The streaming model's text goes to translation.
    Streaming,
    /// The refine model decodes the audio of each committed clause; the
    /// streaming text is the fallback.
    TwoPass,
    /// A refine-class model re-decodes a growing buffer behind the streaming
    /// port. For the pipeline this is `Streaming`: the adapter hides the
    /// re-decoding and exposes a stable prefix.
    Segmented,
}

/// Capacities of the queues of the module table. The overflow policy of each
/// queue is fixed by the table, not configurable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueCaps {
    /// Q1, capture to recognition: milliseconds of audio.
    pub audio_in: Duration,
    /// Q2, events into the control thread: items.
    pub inbox: usize,
    /// Q3, committed clauses waiting for the refine pass: items.
    pub refine_in: usize,
    /// Q4, committed clauses waiting for translation: items.
    pub mt_in: usize,
    /// Q5, translated pieces waiting for synthesis: items.
    pub tts_in: usize,
    /// Q6, synthesised audio held back for ordered release: milliseconds of audio.
    pub hold: Duration,
    /// Q7, events to the caller: items.
    pub events_out: usize,
    /// Q8, draft jobs of re-translation waiting for the draft worker: items.
    pub draft_in: usize,
    /// Q9, draft replies waiting for the recognition thread: items.
    pub draft_out: usize,
}

impl Default for QueueCaps {
    /// The capacities of pipeline-design 1.2.
    fn default() -> Self {
        Self {
            audio_in: Duration::from_secs(2),
            inbox: 256,
            refine_in: 8,
            mt_in: 8,
            tts_in: 8,
            hold: Duration::from_secs(20),
            events_out: 256,
            draft_in: 1,
            draft_out: 4,
        }
    }
}

/// When a failed direction tries to recover (pipeline-design 3.7, Failed to Starting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryParams {
    /// Wait before the first probe of the failed provider.
    pub first_probe_after: Duration,
    /// The wait doubles after each failed probe up to this.
    pub max_wait: Duration,
}

impl Default for RecoveryParams {
    fn default() -> Self {
        Self {
            first_probe_after: Duration::from_secs(2),
            max_wait: Duration::from_secs(30),
        }
    }
}

/// Everything one direction pipeline reads from configuration. The
/// composition root fills it from `prxvc-config`; this crate does not depend
/// on that crate (CS-1.3).
#[derive(Clone, Debug)]
pub struct PipelineParams {
    /// Which direction this pipeline runs.
    pub direction: Direction,
    /// Language spoken in this direction's audio.
    pub source: Lang,
    /// Language the listener hears.
    pub target: Lang,
    /// `pipeline.clause_pause_ms`, `segment_end_pause_ms`, `long_segment_*`,
    /// `max_clause_s`, `max_segment_s`, `early_commit`, `early_chunk_chars`,
    /// `early_chunk_words`, `early_max_wait_ms`, `split_target_clauses`.
    pub commit: CommitParams,
    /// `pipeline.retranslation` and `retranslate_*`: commit a source prefix while the speaker goes
    /// on, once its translation is confirmed. Off unless the composition root turns it on.
    pub retranslate: RetranslateParams,
    /// `call.barge_in` and `call.barge_in_max_lag_ms`.
    pub interrupt: InterruptParams,
    /// `call.<direction>_when_paused` and `call.<direction>_on_failure`.
    pub output: OutputPolicy,
    /// `pipeline.recognition_mode`.
    pub recognition_mode: RecognitionMode,
    /// `pipeline.vad_threshold`: probability that starts speech.
    pub vad_threshold: f32,
    /// Probability below which a frame inside speech counts as silence (the hysteresis).
    pub vad_release_threshold: f32,
    /// `pipeline.min_speech_ms`.
    pub min_speech: Duration,
    /// `pipeline.min_speech_dbfs`: the least mean level, in dBFS, of the frames that start a
    /// segment; -100 or less is off.
    pub min_speech_dbfs: f32,
    /// `pipeline.pad_ms`: audio kept before the detected onset.
    pub pad: Duration,
    /// `pipeline.drop_filler`: a recognised unit that is only filler is not translated or spoken.
    pub drop_filler: bool,
    /// `pipeline.skip_source_in_target`: a recognised clause that is already in the language the
    /// listener hears is not translated or spoken; the original voice is passed through.
    pub skip_source_in_target: bool,
    /// `pipeline.retry_wrong_language`: a translation that starts in the wrong language is
    /// requested once more with a stricter instruction before the clause fails.
    pub retry_wrong_language: bool,
    /// `pipeline.mt_context_clauses`: previous source/target pairs sent as context.
    pub mt_context_clauses: usize,
    /// `pipeline.backlog_skipping`, `max_backlog_ms` and `max_lag_ms`.
    pub backlog: BacklogParams,
    /// `pipeline.catch_up`, `catch_up_after_ms` and `catch_up_max_speed`: how a queue of
    /// translated speech is caught up without dropping anything.
    pub catch_up: crate::pace::CatchUpParams,
    /// `pipeline.jitter_buffer_ms`. The pipeline does not read it: the
    /// playout side that consumes the audio events does. Carried so that one
    /// struct mirrors the section.
    pub jitter_buffer: Duration,
    /// The voice the direction speaks with (`voice.*`).
    pub voice: VoiceSelection,
    /// The session glossary: required term translations.
    pub glossary: Vec<(String, String)>,
    /// Words the recogniser should favour.
    pub hotwords: Vec<String>,
    /// Queue capacities.
    pub queues: QueueCaps,
    /// Recovery probing after a failure.
    pub recovery: RecoveryParams,
    /// Consecutive refine failures after which the direction is Degraded
    /// (`DegradeReason::RefineFailing`).
    pub refine_failures_to_degrade: u32,
    /// Frames of audio with no overrun after which a capture overrun is over.
    pub overrun_clear_frames: u32,
    /// Clauses kept in the snapshot, newest last.
    pub snapshot_clauses: usize,
    /// Text sent to the translator and the synthesiser as a health probe
    /// while recovering from a failure.
    pub probe_text: String,
}

impl PipelineParams {
    /// Parameters with the configuration defaults of pipeline-design 7.1 for
    /// a direction translating `source` into `target`, speaking with `voice`.
    pub fn new(direction: Direction, source: Lang, target: Lang, voice: VoiceSelection) -> Self {
        Self {
            direction,
            source,
            target,
            commit: CommitParams::default(),
            retranslate: RetranslateParams::disabled(),
            interrupt: InterruptParams::default(),
            output: OutputPolicy::default(),
            recognition_mode: RecognitionMode::Streaming,
            vad_threshold: 0.4,
            vad_release_threshold: 0.25,
            min_speech: Duration::from_millis(160),
            min_speech_dbfs: -75.0,
            pad: Duration::from_millis(200),
            drop_filler: true,
            skip_source_in_target: true,
            retry_wrong_language: true,
            mt_context_clauses: 4,
            backlog: BacklogParams::default(),
            catch_up: crate::pace::CatchUpParams::default(),
            jitter_buffer: Duration::from_millis(150),
            voice,
            glossary: Vec::new(),
            hotwords: Vec::new(),
            queues: QueueCaps::default(),
            recovery: RecoveryParams::default(),
            refine_failures_to_degrade: 3,
            overrun_clear_frames: 50,
            snapshot_clauses: 12,
            probe_text: "Hello.".to_owned(),
        }
    }

    /// Whether a refine pass runs between commit and translation.
    pub fn refines(&self) -> bool {
        self.recognition_mode == RecognitionMode::TwoPass
    }
}
