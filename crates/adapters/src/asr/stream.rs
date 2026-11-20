//! One recognition stream of `QwenAsr`: the caller's handle on a segment.
//!
//! `feed` never decodes: it appends audio, collects the result of the last finished partial pass,
//! and asks the worker for the next pass when the cadence says so. `finalize` queues the final
//! decode of the whole segment and waits for it.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;

use prxvc_media::{Lang, MonoTime, SampleRate};
use prxvc_ports::{AsrContext, AsrError, AsrHypothesis, AsrStream, CancellationToken, SegmentEnd};

use super::core::{Core, Job, SlotShared};
use super::stable::StablePrefix;

use std::sync::atomic::Ordering;

const RATE: SampleRate = SampleRate::HZ_16000;

/// A forced endpoint in progress: the final decode of the head of an over-long segment.
struct Cut {
    rx: mpsc::Receiver<Result<String, AsrError>>,
    abort: Arc<AtomicBool>,
    /// Samples of the segment before the cut.
    head_samples: usize,
}

/// The stream handed out by `QwenAsr::open`.
pub(crate) struct QwenStream {
    core: Arc<Core>,
    id: u64,
    shared: Arc<SlotShared>,
    lang: Lang,
    // Kept for models that can use them; the Qwen3-ASR runtime takes neither.
    _ctx: AsrContext,
    stable: StablePrefix,
    revision: u32,
    /// Capture time of the segment's first sample.
    start_ts: Option<MonoTime>,
    /// Samples of audio in the open segment.
    samples: usize,
    generation: u64,
    last_request_samples: usize,
    last_request_at: Option<MonoTime>,
    last_emitted: Option<(String, usize)>,
    cut: Option<Cut>,
    finalized: bool,
}

impl QwenStream {
    pub(crate) fn open(core: &Arc<Core>, lang: Lang, ctx: AsrContext) -> Self {
        let (id, shared) = core.register(lang);
        Self {
            core: Arc::clone(core),
            id,
            shared,
            lang,
            _ctx: ctx,
            stable: StablePrefix::new(core.params.stable_agreement, core.params.stable_tail_guard),
            revision: 0,
            start_ts: None,
            samples: 0,
            generation: 0,
            last_request_samples: 0,
            last_request_at: None,
            last_emitted: None,
            cut: None,
            finalized: false,
        }
    }

    fn audio_end(&self, samples: usize) -> MonoTime {
        self.start_ts
            .unwrap_or(MonoTime::ZERO)
            .saturating_add(RATE.duration_of(samples))
    }

    fn clear_segment(&mut self) {
        if let Some(cut) = self.cut.take() {
            cut.abort.store(true, Ordering::Release);
        }
        self.generation = self.core.reset_slot(self.id);
        self.shared.audio.lock().clear();
        self.stable.reset();
        self.revision = 0;
        self.start_ts = None;
        self.samples = 0;
        self.last_request_samples = 0;
        self.last_request_at = None;
        self.last_emitted = None;
        self.finalized = false;
    }

    fn check_alive(&self) -> Result<(), AsrError> {
        if self.core.is_dead() {
            return Err(AsrError::Unavailable {
                reason: "the recogniser engine has stopped".to_owned(),
            });
        }
        Ok(())
    }

    /// Take the result of the finished partial pass, if it is for the open segment, and turn it
    /// into a hypothesis when the text or the stable prefix changed.
    fn collect_partial(&mut self) -> Result<Option<AsrHypothesis>, AsrError> {
        let Some(result) = self.shared.result.lock().take() else {
            return Ok(None);
        };
        let pass = result?;
        if pass.generation != self.generation {
            return Ok(None);
        }
        let s = self.stable.push(&pass.text);
        if s.repaired {
            self.core
                .counters
                .stable_repairs
                .fetch_add(1, Ordering::Relaxed);
        }
        let key = (s.text.clone(), s.stable_len);
        if self.last_emitted.as_ref() == Some(&key) {
            return Ok(None);
        }
        self.last_emitted = Some(key);
        self.revision += 1;
        Ok(Some(AsrHypothesis {
            text: s.text,
            stable_prefix_len: s.stable_len,
            revision: self.revision,
            is_final: false,
            ended_by: None,
            contradicts_stable: false,
            audio_end: self.audio_end(pass.samples),
        }))
    }

    /// Ask for the next partial pass when it is due.
    fn schedule(&mut self) {
        let params = &self.core.params;
        let first = RATE.samples_in(params.first_decode_after);
        if self.samples < first || self.samples <= self.last_request_samples {
            return;
        }
        let now = self.core.clock.now();
        let due = self
            .last_request_at
            .is_none_or(|at| now.saturating_since(at) >= params.redecode_interval);
        if !due {
            return;
        }
        // A pass that is still wanted or running is not queued behind: the request is skipped.
        // The cadence timer restarts either way, so a slow engine is asked once per interval.
        self.core.request_partial(self.id);
        self.last_request_at = Some(now);
        self.last_request_samples = self.samples;
    }

    /// At the maximum segment length: queue the final decode of the head of the segment and
    /// continue with the tail in a new one.
    fn start_cut(&mut self) -> Result<(), AsrError> {
        let (head, head_samples) = {
            let mut audio = self.shared.audio.lock();
            let at = quietest_point(&audio, self.core.params.cut_search);
            let tail = audio.split_off(at);
            (std::mem::replace(&mut *audio, tail), at)
        };
        self.samples -= head_samples;
        self.generation = self.core.reset_slot(self.id);
        self.last_request_samples = 0;
        self.last_request_at = None;
        let (reply, rx) = mpsc::channel();
        let abort = Arc::new(AtomicBool::new(false));
        self.core.submit(Job {
            audio: head,
            lang: self.lang,
            abort: Arc::clone(&abort),
            reply,
        })?;
        self.core
            .counters
            .forced_cuts
            .fetch_add(1, Ordering::Relaxed);
        self.cut = Some(Cut {
            rx,
            abort,
            head_samples,
        });
        Ok(())
    }

    fn finish_cut(&mut self, text: &str, head_samples: usize) -> AsrHypothesis {
        let s = self.stable.finish(text);
        if s.repaired {
            self.core
                .counters
                .final_rewrites
                .fetch_add(1, Ordering::Relaxed);
        }
        let audio_end = self.audio_end(head_samples);
        self.start_ts = Some(audio_end);
        self.last_emitted = None;
        let revision = self.revision + 1;
        // The audio after the cut is the next segment: its revisions start again.
        self.revision = 0;
        AsrHypothesis {
            text: s.text,
            stable_prefix_len: s.stable_len,
            revision,
            is_final: true,
            ended_by: Some(SegmentEnd::MaxLength),
            contradicts_stable: s.repaired,
            audio_end,
        }
    }
}

/// The sample index in the last `window` of `audio` where the signal is quietest (mean square over
/// 10 ms steps): the place to cut when no stable boundary is known.
fn quietest_point(audio: &[f32], window: std::time::Duration) -> usize {
    let step = RATE.samples_in(std::time::Duration::from_millis(10)).max(1);
    let from = audio.len().saturating_sub(RATE.samples_in(window));
    let mut best = (f32::MAX, audio.len());
    let mut at = from;
    while at + step <= audio.len() {
        let energy: f32 = audio[at..at + step].iter().map(|s| s * s).sum();
        if energy < best.0 {
            best = (energy, at + step / 2);
        }
        at += step;
    }
    best.1.min(audio.len())
}

/// Join the text before a forced cut with the text after it.
fn join_texts(head: &str, tail: &str) -> String {
    let ascii_edge = head.ends_with(|c: char| c.is_ascii_alphanumeric())
        && tail.starts_with(|c: char| c.is_ascii_alphanumeric());
    if head.is_empty() || tail.is_empty() {
        format!("{head}{tail}")
    } else if ascii_edge
        || head.ends_with(|c: char| c.is_ascii_punctuation())
            && tail.starts_with(|c: char| c.is_ascii_alphanumeric())
    {
        format!("{head} {tail}")
    } else {
        format!("{head}{tail}")
    }
}

impl AsrStream for QwenStream {
    fn feed(&mut self, audio: &[f32], ts: MonoTime) -> Result<Option<AsrHypothesis>, AsrError> {
        if self.finalized {
            self.clear_segment();
        }
        self.check_alive()?;
        if audio.iter().any(|s| !s.is_finite()) {
            return Err(AsrError::Internal(
                "audio holds a non-finite sample".to_owned(),
            ));
        }
        self.start_ts.get_or_insert(ts);
        self.shared.audio.lock().extend_from_slice(audio);
        self.samples += audio.len();

        if let Some(cut) = &self.cut {
            return match cut.rx.try_recv() {
                Ok(Ok(text)) => {
                    let head_samples = cut.head_samples;
                    self.cut = None;
                    Ok(Some(self.finish_cut(&text, head_samples)))
                }
                Ok(Err(e)) => {
                    self.cut = None;
                    Err(e)
                }
                Err(mpsc::TryRecvError::Empty) => Ok(None),
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.cut = None;
                    self.check_alive()?;
                    Err(AsrError::Internal("the cut decode was lost".to_owned()))
                }
            };
        }

        let out = self.collect_partial()?;
        if self.samples >= RATE.samples_in(self.core.params.max_segment) {
            self.start_cut()?;
        } else {
            self.schedule();
        }
        Ok(out)
    }

    fn finalize(&mut self, cancel: &CancellationToken) -> Result<AsrHypothesis, AsrError> {
        if self.finalized {
            return Err(AsrError::Internal(
                "the stream is finalized; call reset for the next segment".to_owned(),
            ));
        }
        self.check_alive()?;
        let timeout = self.core.params.final_timeout;
        let cancelled = || cancel.is_cancelled();
        // A forced cut still in flight: its text is the start of this segment's text.
        let mut head: Option<(String, usize)> = None;
        if let Some(cut) = self.cut.take() {
            let text = self
                .core
                .wait_reply(&cut.rx, &cut.abort, &cancelled, timeout)?;
            head = Some((text, cut.head_samples));
        }
        // Drop the pending and running partial pass: the final decodes everything.
        self.generation = self.core.reset_slot(self.id);
        let audio = self.shared.audio.lock().clone();
        let tail_text = if audio.is_empty() {
            String::new()
        } else {
            let (reply, rx) = mpsc::channel();
            let abort = Arc::new(AtomicBool::new(false));
            self.core.submit(Job {
                audio,
                lang: self.lang,
                abort: Arc::clone(&abort),
                reply,
            })?;
            self.core.wait_reply(&rx, &abort, &cancelled, timeout)?
        };
        self.core.counters.finals.fetch_add(1, Ordering::Relaxed);
        let (text, covered) = match head {
            Some((head_text, head_samples)) => (
                join_texts(&head_text, &tail_text),
                head_samples + self.samples,
            ),
            None => (tail_text, self.samples),
        };
        let s = self.stable.finish(&text);
        if s.repaired {
            self.core
                .counters
                .final_rewrites
                .fetch_add(1, Ordering::Relaxed);
        }
        self.revision += 1;
        self.finalized = true;
        Ok(AsrHypothesis {
            stable_prefix_len: s.stable_len,
            text: s.text,
            revision: self.revision,
            is_final: true,
            ended_by: Some(SegmentEnd::Requested),
            contradicts_stable: s.repaired,
            audio_end: self.audio_end(covered),
        })
    }

    fn reset(&mut self) {
        self.clear_segment();
    }
}

impl Drop for QwenStream {
    fn drop(&mut self) {
        if let Some(cut) = self.cut.take() {
            cut.abort.store(true, Ordering::Release);
        }
        self.core.unregister(self.id);
    }
}
