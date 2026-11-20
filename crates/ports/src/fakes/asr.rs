//! Fakes of the streaming recogniser and the refine recogniser, driven by scripted hypotheses and an injected clock.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, Lang, MonoTime, SampleRate};
use tokio_util::sync::CancellationToken;

use super::log::{EventKind, EventLog};
use crate::asr::{AsrContext, AsrError, AsrHypothesis, AsrStream, RefineAsr, StreamingAsr};
use crate::descriptor::{ProviderDescriptor, StreamingKind};

/// What a fake recognition stream says.
#[derive(Clone, Debug)]
pub struct AsrScript {
    /// Partial texts: the n-th call to `feed` returns the n-th entry (with its
    /// stable prefix length in bytes); calls past the end return no partial.
    pub partials: Vec<(String, usize)>,
    /// The text of the final hypothesis.
    pub final_text: String,
    /// How long after the call `finalize` takes, on the injected clock.
    pub final_delay: Duration,
    /// Fail the `feed` call with this index.
    pub fail_feed_at: Option<(usize, AsrError)>,
    /// Fail `finalize`.
    pub fail_finalize: Option<AsrError>,
    /// What the stream says after the first, second, ... `reset`: the
    /// partials (with stable prefix lengths) and the final text of each round.
    /// A stream that was reset more often than there are rounds replays the
    /// last one. Empty means every round plays the fields above.
    pub later_rounds: Vec<(Vec<(String, usize)>, String)>,
}

impl AsrScript {
    /// A script whose partials have no stable prefix.
    pub fn new(partials: impl IntoIterator<Item = &'static str>, final_text: &str) -> Self {
        Self {
            partials: partials.into_iter().map(|t| (t.to_owned(), 0)).collect(),
            final_text: final_text.to_owned(),
            final_delay: Duration::ZERO,
            fail_feed_at: None,
            fail_finalize: None,
            later_rounds: Vec::new(),
        }
    }

    /// After the next `reset` (a new segment or clause) the stream plays these
    /// partials and this final text.
    #[must_use]
    pub fn then_round(
        mut self,
        partials: impl IntoIterator<Item = (&'static str, usize)>,
        final_text: &str,
    ) -> Self {
        self.later_rounds.push((
            partials
                .into_iter()
                .map(|(t, n)| (t.to_owned(), n))
                .collect(),
            final_text.to_owned(),
        ));
        self
    }

    /// The partials and final text of round `round` (the number of resets so far).
    fn round(&self, round: usize) -> (&[(String, usize)], &str) {
        match round.checked_sub(1) {
            None => (&self.partials, &self.final_text),
            Some(k) => self
                .later_rounds
                .get(k)
                .or(self.later_rounds.last())
                .map_or((&self.partials, &self.final_text), |(p, f)| {
                    (p.as_slice(), f.as_str())
                }),
        }
    }

    /// `finalize` takes `delay` on the injected clock.
    #[must_use]
    pub fn with_final_delay(mut self, delay: Duration) -> Self {
        self.final_delay = delay;
        self
    }

    /// Fail the `feed` call with index `index`.
    #[must_use]
    pub fn failing_feed_at(mut self, index: usize, error: AsrError) -> Self {
        self.fail_feed_at = Some((index, error));
        self
    }

    /// Fail `finalize`.
    #[must_use]
    pub fn failing_finalize(mut self, error: AsrError) -> Self {
        self.fail_finalize = Some(error);
        self
    }
}

struct AsrShared {
    clock: Arc<dyn Clock>,
    descriptor: Mutex<ProviderDescriptor>,
    default_script: AsrScript,
    by_lang: Mutex<HashMap<Lang, AsrScript>>,
    log: EventLog,
    opened: AtomicUsize,
}

/// A [`StreamingAsr`] whose streams play a script.
///
/// Streams are independent: each has its own counters and its own
/// cancellation, so cancelling the `finalize` of one leaves the others alone.
/// Requests are named `stream-1`, `stream-2`, ... in the [`EventLog`].
#[derive(Clone)]
pub struct FakeStreamingAsr {
    shared: Arc<AsrShared>,
}

impl FakeStreamingAsr {
    /// A provider whose streams play `script`, timed on `clock`.
    pub fn new(clock: impl Clock + 'static, script: AsrScript) -> Self {
        let descriptor = ProviderDescriptor::new("fake-asr", "fake", StreamingKind::Native)
            .with_stable_prefix(true)
            .with_languages([Lang::Zh, Lang::En])
            .with_sample_rates([SampleRate::HZ_16000]);
        Self {
            shared: Arc::new(AsrShared {
                clock: Arc::new(clock),
                descriptor: Mutex::new(descriptor),
                default_script: script,
                by_lang: Mutex::new(HashMap::new()),
                log: EventLog::new(),
                opened: AtomicUsize::new(0),
            }),
        }
    }

    /// Streams opened for `lang` play `script` instead of the default.
    #[must_use]
    pub fn with_script_for(self, lang: Lang, script: AsrScript) -> Self {
        self.shared.by_lang.lock().insert(lang, script);
        self
    }

    /// Replace the descriptor, for example to make the provider non-streaming.
    #[must_use]
    pub fn with_descriptor(self, descriptor: ProviderDescriptor) -> Self {
        *self.shared.descriptor.lock() = descriptor;
        self
    }

    /// The log of every stream's life.
    pub fn log(&self) -> EventLog {
        self.shared.log.clone()
    }
}

impl StreamingAsr for FakeStreamingAsr {
    fn descriptor(&self) -> ProviderDescriptor {
        self.shared.descriptor.lock().clone()
    }

    fn open(&self, lang: Lang, _ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError> {
        if !self.shared.descriptor.lock().supports_lang(lang) {
            return Err(AsrError::UnsupportedLanguage(lang));
        }
        let n = self.shared.opened.fetch_add(1, Ordering::SeqCst) + 1;
        let script = self
            .shared
            .by_lang
            .lock()
            .get(&lang)
            .cloned()
            .unwrap_or_else(|| self.shared.default_script.clone());
        let request = format!("stream-{n}");
        self.shared.log.record(&request, EventKind::Opened);
        Ok(Box::new(FakeAsrStream {
            shared: Arc::clone(&self.shared),
            script,
            request,
            fed: 0,
            revision: 0,
            resets: 0,
            audio_end: MonoTime::ZERO,
        }))
    }
}

struct FakeAsrStream {
    shared: Arc<AsrShared>,
    script: AsrScript,
    request: String,
    fed: usize,
    revision: u32,
    resets: usize,
    audio_end: MonoTime,
}

impl AsrStream for FakeAsrStream {
    fn feed(&mut self, audio: &[f32], ts: MonoTime) -> Result<Option<AsrHypothesis>, AsrError> {
        let index = self.fed;
        self.fed += 1;
        let _ = audio;
        self.audio_end = ts;
        if let Some((at, error)) = &self.script.fail_feed_at
            && *at == index
        {
            self.shared.log.record(&self.request, EventKind::Failed);
            return Err(error.clone());
        }
        let (partials, _) = self.script.round(self.resets);
        let Some((text, stable)) = partials.get(index) else {
            return Ok(None);
        };
        self.revision += 1;
        self.shared.log.record(&self.request, EventKind::Item);
        Ok(Some(AsrHypothesis {
            text: text.clone(),
            stable_prefix_len: (*stable).min(text.len()),
            revision: self.revision,
            is_final: false,
            audio_end: ts,
        }))
    }

    fn finalize(&mut self, cancel: &CancellationToken) -> Result<AsrHypothesis, AsrError> {
        let due = self
            .shared
            .clock
            .now()
            .saturating_add(self.script.final_delay);
        self.shared.log.record(&self.request, EventKind::Started);
        if cancel.is_cancelled()
            || !self
                .shared
                .clock
                .sleep_until(due, &|| cancel.is_cancelled())
        {
            self.shared.log.record(&self.request, EventKind::Cancelled);
            return Err(AsrError::Cancelled);
        }
        if let Some(error) = &self.script.fail_finalize {
            self.shared.log.record(&self.request, EventKind::Failed);
            return Err(error.clone());
        }
        self.revision += 1;
        self.shared.log.record(&self.request, EventKind::Completed);
        let final_text = self.script.round(self.resets).1.to_owned();
        Ok(AsrHypothesis {
            stable_prefix_len: final_text.len(),
            text: final_text,
            revision: self.revision,
            is_final: true,
            audio_end: self.audio_end,
        })
    }

    fn reset(&mut self) {
        self.fed = 0;
        self.revision = 0;
        self.resets += 1;
    }
}

/// What the fake refine recogniser answers.
#[derive(Clone, Debug)]
pub struct RefineScript {
    /// The transcript returned.
    pub text: String,
    /// How long a call takes on the injected clock.
    pub delay: Duration,
    /// Fail every call with this error (after the delay).
    pub fail: Option<AsrError>,
}

impl RefineScript {
    /// A script that answers `text` after `delay`.
    pub fn new(text: &str, delay: Duration) -> Self {
        Self {
            text: text.to_owned(),
            delay,
            fail: None,
        }
    }

    /// Fail every call with `error`.
    #[must_use]
    pub fn failing(mut self, error: AsrError) -> Self {
        self.fail = Some(error);
        self
    }
}

struct RefineShared {
    clock: Arc<dyn Clock>,
    descriptor: ProviderDescriptor,
    script: RefineScript,
    log: EventLog,
    calls: AtomicUsize,
    audio_lens: Mutex<Vec<usize>>,
}

/// A [`RefineAsr`] that answers a script. Calls are named `refine-1`, `refine-2`, ... in the log.
#[derive(Clone)]
pub struct FakeRefineAsr {
    shared: Arc<RefineShared>,
}

impl FakeRefineAsr {
    /// A refine recogniser playing `script`, timed on `clock`.
    pub fn new(clock: impl Clock + 'static, script: RefineScript) -> Self {
        let descriptor = ProviderDescriptor::new("fake-refine", "fake", StreamingKind::Segmented)
            .with_languages([Lang::Zh, Lang::En]);
        Self {
            shared: Arc::new(RefineShared {
                clock: Arc::new(clock),
                descriptor,
                script,
                log: EventLog::new(),
                calls: AtomicUsize::new(0),
                audio_lens: Mutex::new(Vec::new()),
            }),
        }
    }

    /// The log of every call's life.
    pub fn log(&self) -> EventLog {
        self.shared.log.clone()
    }

    /// The number of samples of each call's audio, in call order.
    pub fn audio_lens(&self) -> Vec<usize> {
        self.shared.audio_lens.lock().clone()
    }
}

impl RefineAsr for FakeRefineAsr {
    fn descriptor(&self) -> ProviderDescriptor {
        self.shared.descriptor.clone()
    }

    fn transcribe(
        &self,
        audio: &[f32],
        _rate: SampleRate,
        lang: Lang,
        _ctx: &AsrContext,
        cancel: &CancellationToken,
    ) -> Result<AsrHypothesis, AsrError> {
        let shared = &self.shared;
        if !shared.descriptor.supports_lang(lang) {
            return Err(AsrError::UnsupportedLanguage(lang));
        }
        let request = format!("refine-{}", shared.calls.fetch_add(1, Ordering::SeqCst) + 1);
        shared.audio_lens.lock().push(audio.len());
        let due = shared.clock.now().saturating_add(shared.script.delay);
        shared.log.record(&request, EventKind::Opened);
        if cancel.is_cancelled() || !shared.clock.sleep_until(due, &|| cancel.is_cancelled()) {
            shared.log.record(&request, EventKind::Cancelled);
            return Err(AsrError::Cancelled);
        }
        if let Some(error) = &shared.script.fail {
            shared.log.record(&request, EventKind::Failed);
            return Err(error.clone());
        }
        shared.log.record(&request, EventKind::Completed);
        Ok(AsrHypothesis {
            text: shared.script.text.clone(),
            stable_prefix_len: shared.script.text.len(),
            revision: 1,
            is_final: true,
            audio_end: shared.clock.now(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use prxvc_media::ManualClock;

    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn stream(asr: &FakeStreamingAsr) -> Box<dyn AsrStream> {
        asr.open(Lang::Zh, AsrContext::default()).expect("open")
    }

    #[test]
    fn a_stream_yields_scripted_partials_with_rising_revisions_then_a_final() {
        let clock = ManualClock::new();
        let script = AsrScript::new(["he", "hello", "hello wor"], "hello world");
        let asr = FakeStreamingAsr::new(clock.clone(), script);
        let mut s = stream(&asr);
        let cancel = CancellationToken::new();

        let p1 = s
            .feed(&[0.0; 160], MonoTime::from_millis(10))
            .expect("feed")
            .expect("partial");
        let p2 = s
            .feed(&[0.0; 160], MonoTime::from_millis(20))
            .expect("feed")
            .expect("partial");
        assert_eq!(
            (p1.text.as_str(), p1.revision, p1.is_final),
            ("he", 1, false)
        );
        assert_eq!((p2.text.as_str(), p2.revision), ("hello", 2));
        assert_eq!(p2.audio_end, MonoTime::from_millis(20));
        s.feed(&[0.0; 160], MonoTime::from_millis(30))
            .expect("feed");
        assert_eq!(
            s.feed(&[0.0; 160], MonoTime::from_millis(40)),
            Ok(None),
            "script exhausted"
        );

        let fin = s.finalize(&cancel).expect("final");
        assert_eq!(
            (fin.text.as_str(), fin.is_final, fin.revision),
            ("hello world", true, 4)
        );
        assert_eq!(fin.stable_prefix_len, "hello world".len());
        assert_eq!(
            asr.log().kinds_for("stream-1").last(),
            Some(&EventKind::Completed)
        );
    }

    #[test]
    fn reset_starts_the_next_segment_from_revision_one() {
        let asr = FakeStreamingAsr::new(ManualClock::new(), AsrScript::new(["a"], "a b"));
        let mut s = stream(&asr);
        s.feed(&[0.0], MonoTime::ZERO).expect("feed");
        s.reset();
        let again = s
            .feed(&[0.0], MonoTime::ZERO)
            .expect("feed")
            .expect("partial");
        assert_eq!(again.revision, 1);
    }

    #[test]
    fn cancelling_finalize_stops_that_stream_only() {
        let clock = ManualClock::new();
        let script = AsrScript::new([], "done").with_final_delay(MS(100));
        let asr = FakeStreamingAsr::new(clock.clone(), script);
        let (mut a, mut b) = (stream(&asr), stream(&asr));
        let (cancel_a, cancel_b) = (CancellationToken::new(), CancellationToken::new());

        let ta = {
            let t = cancel_a.clone();
            thread::spawn(move || a.finalize(&t))
        };
        let tb = {
            let t = cancel_b.clone();
            thread::spawn(move || b.finalize(&t))
        };
        asr.log().wait_for(EventKind::Started, 2);
        cancel_a.cancel();
        assert_eq!(ta.join().expect("thread a"), Err(AsrError::Cancelled));

        clock.advance(MS(100));
        let fin = tb
            .join()
            .expect("thread b")
            .expect("stream b still finishes");
        assert_eq!(fin.text, "done");
        assert_eq!(
            asr.log().kinds_for("stream-1").last(),
            Some(&EventKind::Cancelled)
        );
        assert_eq!(
            asr.log().kinds_for("stream-2").last(),
            Some(&EventKind::Completed)
        );
        assert!(!cancel_b.is_cancelled());
    }

    #[test]
    fn a_token_cancelled_before_the_call_cancels_it_at_once() {
        let asr = FakeStreamingAsr::new(ManualClock::new(), AsrScript::new([], "x"));
        let cancel = CancellationToken::new();
        cancel.cancel();
        assert_eq!(stream(&asr).finalize(&cancel), Err(AsrError::Cancelled));
    }

    #[test]
    fn injected_failures_hit_the_chosen_call() {
        let clock = ManualClock::new();
        let failing_feed = AsrScript::new(["a", "b"], "ab").failing_feed_at(
            1,
            AsrError::Unavailable {
                reason: "engine died".to_owned(),
            },
        );
        let asr = FakeStreamingAsr::new(clock.clone(), failing_feed);
        let mut s = stream(&asr);
        assert!(
            s.feed(&[0.0], MonoTime::ZERO)
                .expect("first feed ok")
                .is_some()
        );
        assert!(matches!(
            s.feed(&[0.0], MonoTime::ZERO),
            Err(AsrError::Unavailable { .. })
        ));

        let failing_final = AsrScript::new([], "x").failing_finalize(AsrError::Timeout);
        let asr = FakeStreamingAsr::new(clock, failing_final);
        assert_eq!(
            stream(&asr).finalize(&CancellationToken::new()),
            Err(AsrError::Timeout)
        );
        assert_eq!(
            asr.log().kinds_for("stream-1").last(),
            Some(&EventKind::Failed)
        );
    }

    #[test]
    fn capabilities_refuse_what_the_provider_lacks() {
        let asr = FakeStreamingAsr::new(ManualClock::new(), AsrScript::new([], "x"));
        let d = asr.descriptor();
        assert!(d.require_native().is_ok());
        assert!(d.require_rate(SampleRate::HZ_48000).is_err());

        let zh_only = d.clone().with_languages([Lang::Zh]);
        let asr = asr.with_descriptor(zh_only);
        assert!(matches!(
            asr.open(Lang::En, AsrContext::default()).err(),
            Some(AsrError::UnsupportedLanguage(Lang::En))
        ));

        let batch = ProviderDescriptor::new("batch", "m", StreamingKind::None);
        assert!(
            asr.with_descriptor(batch)
                .descriptor()
                .require_live()
                .is_err()
        );
    }

    #[test]
    fn scripts_can_differ_per_language() {
        let asr = FakeStreamingAsr::new(ManualClock::new(), AsrScript::new([], "default"))
            .with_script_for(Lang::En, AsrScript::new([], "english"));
        let cancel = CancellationToken::new();
        let zh = asr
            .open(Lang::Zh, AsrContext::default())
            .expect("zh")
            .finalize(&cancel);
        let en = asr
            .open(Lang::En, AsrContext::default())
            .expect("en")
            .finalize(&cancel);
        assert_eq!(zh.expect("zh final").text, "default");
        assert_eq!(en.expect("en final").text, "english");
    }

    #[test]
    fn refine_returns_the_script_after_its_delay_and_records_the_audio() {
        let clock = ManualClock::new();
        let refine = FakeRefineAsr::new(clock.clone(), RefineScript::new("refined text", MS(50)));
        let call = {
            let refine = refine.clone();
            thread::spawn(move || {
                refine.transcribe(
                    &[0.0; 3200],
                    SampleRate::HZ_16000,
                    Lang::En,
                    &AsrContext::default(),
                    &CancellationToken::new(),
                )
            })
        };
        refine.log().wait_for(EventKind::Opened, 1);
        clock.advance(MS(50));
        let result = call.join().expect("thread").expect("transcript");
        assert_eq!(
            (result.text.as_str(), result.is_final),
            ("refined text", true)
        );
        assert_eq!(refine.audio_lens(), vec![3200]);
        assert!(
            refine.descriptor().require_native().is_err(),
            "refine is segmented, not a live stream"
        );
    }

    #[test]
    fn cancelling_one_refine_call_leaves_a_concurrent_call_running() {
        let clock = ManualClock::new();
        let refine = FakeRefineAsr::new(clock.clone(), RefineScript::new("ok", MS(80)));
        let spawn_call = |token: &CancellationToken| {
            let (refine, token) = (refine.clone(), token.clone());
            thread::spawn(move || {
                refine.transcribe(
                    &[0.0; 16],
                    SampleRate::HZ_16000,
                    Lang::Zh,
                    &AsrContext::default(),
                    &token,
                )
            })
        };
        let (cancel_a, cancel_b) = (CancellationToken::new(), CancellationToken::new());
        let (ta, tb) = (spawn_call(&cancel_a), spawn_call(&cancel_b));
        refine.log().wait_for(EventKind::Opened, 2);
        cancel_a.cancel();
        assert_eq!(ta.join().expect("thread a"), Err(AsrError::Cancelled));
        clock.advance(MS(80));
        assert_eq!(
            tb.join().expect("thread b").expect("b completes").text,
            "ok"
        );
        assert_eq!(refine.log().count(EventKind::Cancelled), 1);
        assert_eq!(refine.log().count(EventKind::Completed), 1);
    }

    #[test]
    fn refine_failure_is_typed() {
        let script =
            RefineScript::new("x", Duration::ZERO).failing(AsrError::Internal("decode".to_owned()));
        let refine = FakeRefineAsr::new(ManualClock::new(), script);
        let r = refine.transcribe(
            &[0.0],
            SampleRate::HZ_16000,
            Lang::Zh,
            &AsrContext::default(),
            &CancellationToken::new(),
        );
        assert_eq!(r, Err(AsrError::Internal("decode".to_owned())));
    }

    #[test]
    fn a_reset_moves_the_stream_to_the_next_scripted_round() {
        let asr = FakeStreamingAsr::new(
            ManualClock::new(),
            AsrScript::new(["one"], "one final").then_round([("two", 3)], "two final"),
        );
        let mut s = asr.open(Lang::En, AsrContext::default()).expect("stream");
        let cancel = CancellationToken::new();
        let first = s
            .feed(&[0.0], MonoTime::ZERO)
            .expect("feed")
            .expect("partial");
        assert_eq!(first.text, "one");
        assert_eq!(s.finalize(&cancel).expect("final").text, "one final");
        s.reset();
        let second = s
            .feed(&[0.0], MonoTime::ZERO)
            .expect("feed")
            .expect("partial");
        assert_eq!((second.text.as_str(), second.stable_prefix_len), ("two", 3));
        assert_eq!(s.finalize(&cancel).expect("final").text, "two final");
        s.reset();
        assert_eq!(s.finalize(&cancel).expect("final").text, "two final");
    }
}
