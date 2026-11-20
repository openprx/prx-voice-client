//! Where the stage timestamps of a call go: a recorder that groups them per clause, keeps the last clause's figures of each direction for the debug view, and, only when switched on, writes one JSON line per clause to a size-capped file.
//!
//! * Off by default. Switched on by `debug.call_trace` or the environment variable
//!   `PRXVC_CALL_TRACE=1`; the texts of the clause (recognised, translated, spoken) are added only
//!   with a second switch, `debug.call_trace_text` or `PRXVC_CALL_TRACE_TEXT=1`. Without it the
//!   recorder never asks for text, so call content cannot reach a file or a log.
//! * The file is `<data dir>/traces/call-<unix seconds>.jsonl`, at most [`TraceOptions::cap_bytes`]
//!   long (a last line says it was cut), and only the newest [`KEEP_FILES`] files are kept.
//! * Each line is one clause: identity, outcome, the stage timestamps in nanoseconds on the
//!   pipeline clock (`t_ns`), the intervals derived from them in milliseconds (`ms`) and, with the
//!   text switch, the texts. The first line of a file describes the session.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::Mutex;
use prxvc_config::AppConfig;
use prxvc_media::{ClauseId, Direction, MonoTime};
use prxvc_ports::{TraceEvent, TraceSink, TraceStage};

/// How many trace files stay in the directory.
pub(crate) const KEEP_FILES: usize = 5;

/// What to record and where.
#[derive(Clone, Debug)]
pub(crate) struct TraceOptions {
    /// Write a file at all.
    pub enabled: bool,
    /// Add the texts to each line.
    pub text: bool,
    /// Directory of the files.
    pub dir: PathBuf,
    /// Most bytes of one file.
    pub cap_bytes: u64,
}

impl TraceOptions {
    /// The options of `config` and the environment (`PRXVC_CALL_TRACE`, `PRXVC_CALL_TRACE_TEXT`; "1" or "true").
    pub(crate) fn from_config(config: &AppConfig, data_dir: &Path) -> Self {
        let on = |var: &str| {
            std::env::var(var).is_ok_and(|v| matches!(v.trim(), "1" | "true" | "TRUE" | "yes"))
        };
        let enabled = config.debug.call_trace || on("PRXVC_CALL_TRACE");
        Self {
            enabled,
            text: enabled && (config.debug.call_trace_text || on("PRXVC_CALL_TRACE_TEXT")),
            dir: data_dir.join("traces"),
            cap_bytes: 20 * 1024 * 1024,
        }
    }

    /// Nothing is written.
    pub(crate) fn off() -> Self {
        Self {
            enabled: false,
            text: false,
            dir: PathBuf::new(),
            cap_bytes: 0,
        }
    }
}

/// The name of a stage in the files and the figures.
pub(crate) const fn stage_name(stage: TraceStage) -> &'static str {
    match stage {
        TraceStage::SpeechStart => "speech_start",
        TraceStage::AsrPartial => "first_partial",
        TraceStage::ClausePause => "clause_pause",
        TraceStage::AsrFinalReady => "asr_final_ready",
        TraceStage::ClauseCommitted => "clause_committed",
        TraceStage::FinalDisagrees => "final_disagrees",
        TraceStage::SegmentCut => "segment_cut",
        TraceStage::RefineDone => "refine_done",
        TraceStage::MtRequestSent => "mt_request_sent",
        TraceStage::MtFirstToken => "mt_first_token",
        TraceStage::MtDone => "mt_done",
        TraceStage::TtsRequestSent => "tts_request_sent",
        TraceStage::TtsFirstChunk => "tts_first_chunk",
        TraceStage::TtsDone => "tts_done",
        TraceStage::PlayoutStart => "playout_start",
        TraceStage::PlayoutEnd => "playout_end",
        TraceStage::Cancelled => "cancelled",
        TraceStage::Skipped => "skipped",
        TraceStage::Failed => "failed",
        _ => "other",
    }
}

/// The stages of one clause, first occurrence of each, with the texts the sink asked for.
#[derive(Clone, Debug)]
pub(crate) struct ClauseTimeline {
    pub clause: ClauseId,
    pub stages: Vec<(TraceStage, MonoTime)>,
    pub texts: Vec<(TraceStage, String)>,
}

impl ClauseTimeline {
    fn new(clause: ClauseId) -> Self {
        Self {
            clause,
            stages: Vec::new(),
            texts: Vec::new(),
        }
    }

    /// When `stage` happened.
    pub(crate) fn at(&self, stage: TraceStage) -> Option<MonoTime> {
        self.stages
            .iter()
            .find(|(s, _)| *s == stage)
            .map(|(_, t)| *t)
    }

    /// Milliseconds from `from` to `to`; `None` when either is missing.
    pub(crate) fn ms(&self, from: TraceStage, to: TraceStage) -> Option<f64> {
        let (a, b) = (self.at(from)?, self.at(to)?);
        Some((b.as_nanos() as f64 - a.as_nanos() as f64) / 1e6)
    }

    /// Why the clause ended, when it did.
    pub(crate) fn outcome(&self) -> Option<&'static str> {
        [
            (TraceStage::PlayoutEnd, "played"),
            (TraceStage::Failed, "failed"),
            (TraceStage::Cancelled, "cancelled"),
            (TraceStage::Skipped, "skipped"),
        ]
        .into_iter()
        .find(|(s, _)| self.at(*s).is_some())
        .map(|(_, name)| name)
    }

    /// The intervals the files and the debug view show, in milliseconds.
    pub(crate) fn intervals(&self) -> Vec<(&'static str, f64)> {
        use TraceStage as S;
        let pairs = [
            (
                "speech_start_to_first_partial",
                S::SpeechStart,
                S::AsrPartial,
            ),
            (
                "speech_start_to_first_audio",
                S::SpeechStart,
                S::PlayoutStart,
            ),
            (
                "clause_pause_to_first_audio",
                S::ClausePause,
                S::PlayoutStart,
            ),
            (
                "clause_pause_to_asr_final",
                S::ClausePause,
                S::AsrFinalReady,
            ),
            (
                "asr_final_to_mt_request",
                S::AsrFinalReady,
                S::MtRequestSent,
            ),
            ("mt_first_token", S::MtRequestSent, S::MtFirstToken),
            ("mt_total", S::MtRequestSent, S::MtDone),
            ("mt_done_to_tts_request", S::MtDone, S::TtsRequestSent),
            ("tts_first_chunk", S::TtsRequestSent, S::TtsFirstChunk),
            ("tts_total", S::TtsRequestSent, S::TtsDone),
            (
                "tts_first_chunk_to_playout",
                S::TtsFirstChunk,
                S::PlayoutStart,
            ),
            (
                "committed_to_first_audio",
                S::ClauseCommitted,
                S::PlayoutStart,
            ),
            ("playout_duration", S::PlayoutStart, S::PlayoutEnd),
        ];
        pairs
            .into_iter()
            .filter_map(|(name, a, b)| self.ms(a, b).map(|ms| (name, ms)))
            .collect()
    }
}

/// The figures of the last finished clause of one direction, for the debug view.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LastClause {
    /// Speech detected until the first translated sample plays.
    pub speech_start_to_first_audio_ms: Option<f64>,
    /// The pause that committed the clause detected until the first translated sample plays.
    pub clause_pause_to_first_audio_ms: Option<f64>,
    /// Every interval of the clause, in the order of the pipeline.
    pub stages: Vec<(String, f64)>,
}

fn direction_slot(d: Direction) -> usize {
    match d {
        Direction::Outgoing => 0,
        Direction::Incoming => 1,
    }
}

struct Writer {
    file: File,
    written: u64,
    cap: u64,
    cut: bool,
}

impl Writer {
    fn line(&mut self, value: &serde_json::Value) {
        if self.cut {
            return;
        }
        let mut text = value.to_string();
        text.push('\n');
        if self.written + text.len() as u64 > self.cap {
            self.cut = true;
            let _ = writeln!(
                self.file,
                "{{\"type\":\"truncated\",\"cap_bytes\":{}}}",
                self.cap
            );
            tracing::warn!(
                cap = self.cap,
                "the call trace file reached its size cap; it is cut here"
            );
            return;
        }
        if self.file.write_all(text.as_bytes()).is_ok() {
            self.written += text.len() as u64;
        }
    }
}

#[derive(Default)]
struct State {
    open: HashMap<ClauseId, ClauseTimeline>,
    done: Vec<ClauseTimeline>,
    last: [Option<LastClause>; 2],
    writer: Option<Writer>,
}

/// The sink the call uses. Cheap to share; `record` never blocks on anything but its own lock.
pub(crate) struct CallTrace {
    state: Mutex<State>,
    text: bool,
    session: String,
    keep_done: bool,
}

impl CallTrace {
    /// A recorder that writes nothing (the product default).
    pub(crate) fn off() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            text: false,
            session: String::new(),
            keep_done: false,
        })
    }

    /// A recorder that follows `options`: a file when enabled.
    pub(crate) fn open(options: &TraceOptions) -> Arc<Self> {
        let session = uuid::Uuid::new_v4().to_string();
        let writer = options
            .enabled
            .then(|| open_file(options, &session))
            .flatten();
        Arc::new(Self {
            state: Mutex::new(State {
                writer,
                ..State::default()
            }),
            text: options.enabled && options.text,
            session,
            keep_done: false,
        })
    }

    /// A recorder that keeps every finished clause in memory and asks for text (the harness).
    pub(crate) fn collecting() -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::default()),
            text: true,
            session: String::new(),
            keep_done: true,
        })
    }

    /// Whether a file is being written.
    pub(crate) fn writes_file(&self) -> bool {
        self.state.lock().writer.is_some()
    }

    /// The figures of the last finished clause of `d`.
    pub(crate) fn last(&self, d: Direction) -> Option<LastClause> {
        self.state.lock().last[direction_slot(d)].clone()
    }

    /// Every finished clause so far (only for a collecting recorder), and the unfinished ones.
    pub(crate) fn timelines(&self) -> Vec<ClauseTimeline> {
        let s = self.state.lock();
        let mut all = s.done.clone();
        all.extend(s.open.values().cloned());
        all
    }

    fn finish(&self, s: &mut State, timeline: ClauseTimeline) {
        let intervals = timeline.intervals();
        let get = |name: &str| intervals.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        s.last[direction_slot(timeline.clause.segment().direction())] = Some(LastClause {
            speech_start_to_first_audio_ms: get("speech_start_to_first_audio"),
            clause_pause_to_first_audio_ms: get("clause_pause_to_first_audio"),
            stages: intervals
                .iter()
                .map(|(n, v)| ((*n).to_owned(), *v))
                .collect(),
        });
        if let Some(writer) = s.writer.as_mut() {
            writer.line(&self.json_of(&timeline, &intervals));
        }
        if self.keep_done {
            s.done.push(timeline);
        }
    }

    fn json_of(&self, t: &ClauseTimeline, intervals: &[(&'static str, f64)]) -> serde_json::Value {
        let c = t.clause;
        let mut stages = serde_json::Map::new();
        for (stage, at) in &t.stages {
            stages.insert(stage_name(*stage).to_owned(), at.as_nanos().into());
        }
        let ms: serde_json::Map<_, _> = intervals
            .iter()
            .map(|(n, v)| {
                (
                    (*n).to_owned(),
                    serde_json::json!((v * 1000.0).round() / 1000.0),
                )
            })
            .collect();
        let mut line = serde_json::json!({
            "v": 1,
            "session": self.session,
            "direction": format!("{:?}", c.segment().direction()).to_lowercase(),
            "segment": c.segment().seq(),
            "clause": c.index(),
            "outcome": t.outcome().unwrap_or("open"),
            "t_ns": stages,
            "ms": ms,
        });
        if self.text && !t.texts.is_empty() {
            let texts: serde_json::Map<_, _> = t
                .texts
                .iter()
                .map(|(s, text)| (stage_name(*s).to_owned(), text.clone().into()))
                .collect();
            line["texts"] = texts.into();
        }
        line
    }
}

impl Drop for CallTrace {
    fn drop(&mut self) {
        // Clauses that never ended (the call stopped) are written as they are.
        let mut s = self.state.lock();
        let open: Vec<ClauseTimeline> = s.open.drain().map(|(_, t)| t).collect();
        for t in open {
            let intervals = t.intervals();
            if let Some(writer) = s.writer.as_mut() {
                writer.line(&self.json_of(&t, &intervals));
            }
        }
    }
}

impl TraceSink for CallTrace {
    fn wants_text(&self) -> bool {
        self.text
    }

    fn record(&self, ev: TraceEvent) {
        let mut s = self.state.lock();
        let timeline = s
            .open
            .entry(ev.clause)
            .or_insert_with(|| ClauseTimeline::new(ev.clause));
        if timeline.at(ev.stage).is_none() {
            timeline.stages.push((ev.stage, ev.at));
        }
        if let Some(text) = ev.text {
            timeline.texts.push((ev.stage, text));
        }
        let terminal = matches!(
            ev.stage,
            TraceStage::PlayoutEnd
                | TraceStage::Cancelled
                | TraceStage::Skipped
                | TraceStage::Failed
        );
        if terminal && let Some(done) = s.open.remove(&ev.clause) {
            self.finish(&mut s, done);
        }
    }
}

/// Create the trace file (and remove the oldest ones). A failure is logged and means no file.
fn open_file(options: &TraceOptions, session: &str) -> Option<Writer> {
    if let Err(e) = fs::create_dir_all(&options.dir) {
        tracing::warn!(dir = %options.dir.display(), "cannot create the trace directory: {e}");
        return None;
    }
    prune(&options.dir);
    let stamp = chrono::Utc::now().timestamp();
    let path = options.dir.join(format!("call-{stamp}.jsonl"));
    let file = match File::create(&path) {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(path = %path.display(), "cannot create the trace file: {e}");
            return None;
        }
    };
    tracing::info!(path = %path.display(), text = options.text, "call trace is on");
    let mut writer = Writer {
        file,
        written: 0,
        cap: options.cap_bytes,
        cut: false,
    };
    writer.line(&serde_json::json!({
        "v": 1,
        "type": "session",
        "session": session,
        "started_at": chrono::Utc::now().to_rfc3339(),
        "text": options.text,
        "clock": "t_ns are nanoseconds on the pipeline clock, which starts when the call is prepared",
    }));
    Some(writer)
}

/// Keep the newest `KEEP_FILES - 1` trace files so that the new one makes `KEEP_FILES`.
fn prune(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "jsonl")
                && p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("call-"))
        })
        .collect();
    files.sort();
    while files.len() >= KEEP_FILES {
        let oldest = files.remove(0);
        let _ = fs::remove_file(oldest);
    }
}

/// The recorder of the call in progress, shared between the host (which makes a new one for each
/// call) and the debug view (which reads the last clause's figures from it).
#[derive(Clone, Default)]
pub(crate) struct TraceSlot(Arc<Mutex<Option<Arc<CallTrace>>>>);

impl TraceSlot {
    /// Make `trace` the current recorder.
    pub(crate) fn set(&self, trace: Arc<CallTrace>) {
        *self.0.lock() = Some(trace);
    }

    /// The figures of the last finished clause of `d` in the current call.
    pub(crate) fn last(&self, d: Direction) -> Option<LastClause> {
        self.0.lock().as_ref().and_then(|t| t.last(d))
    }
}

/// Drops every event and never asks for text.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct NullTrace;

impl TraceSink for NullTrace {
    fn wants_text(&self) -> bool {
        false
    }

    fn record(&self, _event: TraceEvent) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_media::SegmentId;

    fn clause(d: Direction, seg: u64, i: u16) -> ClauseId {
        ClauseId::new(SegmentId::new(d, seg).expect("seq >= 1"), i)
    }

    fn ev(c: ClauseId, stage: TraceStage, ms: u64, text: Option<&str>) -> TraceEvent {
        TraceEvent {
            clause: c,
            at: MonoTime::from_millis(ms),
            stage,
            text: text.map(str::to_owned),
        }
    }

    fn play_one(trace: &CallTrace, c: ClauseId) {
        for (stage, ms, text) in [
            (TraceStage::SpeechStart, 1000, None),
            (TraceStage::AsrPartial, 1900, None),
            (TraceStage::ClausePause, 3000, None),
            (TraceStage::AsrFinalReady, 3150, Some("今天天气很好")),
            (TraceStage::ClauseCommitted, 3150, Some("今天天气很好")),
            (TraceStage::MtRequestSent, 3160, None),
            (TraceStage::MtFirstToken, 3250, None),
            (TraceStage::MtDone, 3400, Some("The weather is nice.")),
            (
                TraceStage::TtsRequestSent,
                3410,
                Some("The weather is nice."),
            ),
            (TraceStage::TtsFirstChunk, 3450, None),
            (TraceStage::TtsDone, 4200, None),
            (TraceStage::PlayoutStart, 3600, None),
            (TraceStage::PlayoutEnd, 5000, None),
        ] {
            trace.record(ev(c, stage, ms, text));
        }
    }

    fn options(dir: &Path, text: bool, cap: u64) -> TraceOptions {
        TraceOptions {
            enabled: true,
            text,
            dir: dir.to_path_buf(),
            cap_bytes: cap,
        }
    }

    fn lines(dir: &Path) -> Vec<serde_json::Value> {
        let file = fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .next()
            .expect("a trace file");
        fs::read_to_string(file)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn nothing_is_written_and_no_text_is_asked_for_by_default() {
        let trace = CallTrace::off();
        assert!(!trace.wants_text());
        assert!(!trace.writes_file());
        let off = TraceOptions::off();
        assert!(!off.enabled && !off.text);
        let config = AppConfig::default();
        let from_config = TraceOptions::from_config(&config, Path::new("/data"));
        assert!(!from_config.enabled || std::env::var("PRXVC_CALL_TRACE").is_ok());
        assert_eq!(from_config.dir, Path::new("/data/traces"));
    }

    #[test]
    fn one_line_per_finished_clause_with_stamps_and_intervals_and_no_text_without_the_second_switch()
     {
        let dir = tempfile::tempdir().unwrap();
        let trace = CallTrace::open(&options(dir.path(), false, 1 << 20));
        assert!(!trace.wants_text(), "the text switch is separate");
        play_one(&trace, clause(Direction::Outgoing, 1, 0));
        let lines = lines(dir.path());
        assert_eq!(lines[0]["type"], "session");
        let line = &lines[1];
        assert_eq!(line["direction"], "outgoing");
        assert_eq!(line["segment"], 1);
        assert_eq!(line["clause"], 0);
        assert_eq!(line["outcome"], "played");
        assert_eq!(line["t_ns"]["playout_start"], 3_600_000_000_u64);
        assert_eq!(line["ms"]["speech_start_to_first_audio"], 2600.0);
        assert_eq!(line["ms"]["mt_first_token"], 90.0);
        assert!(line.get("texts").is_none());
    }

    #[test]
    fn with_the_text_switch_the_texts_are_added() {
        let dir = tempfile::tempdir().unwrap();
        let trace = CallTrace::open(&options(dir.path(), true, 1 << 20));
        assert!(trace.wants_text());
        play_one(&trace, clause(Direction::Incoming, 2, 1));
        let lines = lines(dir.path());
        let texts = &lines[1]["texts"];
        assert_eq!(texts["asr_final_ready"], "今天天气很好");
        assert_eq!(texts["mt_done"], "The weather is nice.");
        assert_eq!(texts["tts_request_sent"], "The weather is nice.");
        assert_eq!(lines[1]["direction"], "incoming");
    }

    #[test]
    fn the_file_is_cut_at_its_cap_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let trace = CallTrace::open(&options(dir.path(), false, 900));
        for seg in 1..=10 {
            play_one(&trace, clause(Direction::Outgoing, seg, 0));
        }
        let lines = lines(dir.path());
        assert!(lines.len() < 12, "{}", lines.len());
        assert_eq!(lines.last().unwrap()["type"], "truncated");
        let file = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .next()
            .unwrap()
            .path();
        assert!(fs::metadata(file).unwrap().len() < 1100);
    }

    #[test]
    fn unfinished_clauses_are_written_when_the_recorder_ends() {
        let dir = tempfile::tempdir().unwrap();
        {
            let trace = CallTrace::open(&options(dir.path(), false, 1 << 20));
            trace.record(ev(
                clause(Direction::Outgoing, 1, 0),
                TraceStage::SpeechStart,
                10,
                None,
            ));
        }
        let lines = lines(dir.path());
        assert_eq!(lines[1]["outcome"], "open");
    }

    #[test]
    fn only_the_newest_files_are_kept() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..8 {
            fs::write(dir.path().join(format!("call-{i:010}.jsonl")), "x").unwrap();
        }
        fs::write(dir.path().join("notes.txt"), "keep").unwrap();
        prune(dir.path());
        let mut left: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left.len(), KEEP_FILES - 1 + 1, "{left:?}");
        assert!(left.contains(&"notes.txt".to_owned()));
        assert!(left.contains(&"call-0000000007.jsonl".to_owned()));
        assert!(!left.contains(&"call-0000000000.jsonl".to_owned()));
    }

    #[test]
    fn the_last_clause_of_each_direction_is_kept_for_the_debug_view() {
        let trace = CallTrace::off();
        play_one(&trace, clause(Direction::Outgoing, 1, 0));
        let last = trace.last(Direction::Outgoing).expect("a finished clause");
        assert_eq!(last.speech_start_to_first_audio_ms, Some(2600.0));
        assert_eq!(last.clause_pause_to_first_audio_ms, Some(600.0));
        assert!(last.stages.iter().any(|(n, _)| n == "tts_first_chunk"));
        assert!(trace.last(Direction::Incoming).is_none());
    }

    #[test]
    fn a_collecting_recorder_keeps_every_clause_and_asks_for_text() {
        let trace = CallTrace::collecting();
        assert!(trace.wants_text());
        play_one(&trace, clause(Direction::Outgoing, 1, 0));
        play_one(&trace, clause(Direction::Outgoing, 2, 0));
        assert_eq!(trace.timelines().len(), 2);
    }
}
