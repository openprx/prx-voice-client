//! Fake speech synthesiser that streams scripted audio chunks on an injected clock.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use prxvc_media::{Clock, Lang, PcmChunk, SampleRate};
use tokio_util::sync::CancellationToken;

use super::log::EventLog;
use super::paced::{PacedStream, Step};
use crate::descriptor::{ProviderDescriptor, StreamingKind};
use crate::tts::{
    PreparedVoice, SpeechSynth, TtsError, TtsItem, TtsRequest, TtsStream, VoiceReference,
    VoiceSelection,
};

/// What the fake synthesiser streams for one text.
#[derive(Clone, Debug)]
pub struct TtsScript {
    /// Samples in each chunk. Chunk `i` is filled with the value `(i + 1) / 100`,
    /// so a test can tell chunks apart.
    pub chunk_samples: Vec<usize>,
    /// Delay before each chunk and before `Done`, on the injected clock.
    pub chunk_delay: Duration,
    /// Fail after this many chunks have been delivered, instead of finishing.
    pub fail_after: Option<(usize, TtsError)>,
    /// Fail `synthesize` itself.
    pub open_error: Option<TtsError>,
}

/// Sample rate of the fake's output.
pub const FAKE_TTS_RATE: SampleRate = SampleRate::HZ_24000;

impl TtsScript {
    /// A script of `chunks` chunks of `samples` samples each, then `Done`.
    pub fn chunks(chunks: usize, samples: usize) -> Self {
        Self {
            chunk_samples: vec![samples; chunks],
            chunk_delay: Duration::ZERO,
            fail_after: None,
            open_error: None,
        }
    }

    /// Wait `delay` on the injected clock before every chunk.
    #[must_use]
    pub fn with_chunk_delay(mut self, delay: Duration) -> Self {
        self.chunk_delay = delay;
        self
    }

    /// Deliver `delivered` chunks, then fail with `error` (for example `Runaway`).
    #[must_use]
    pub fn failing_after(mut self, delivered: usize, error: TtsError) -> Self {
        self.fail_after = Some((delivered, error));
        self
    }

    /// Make `synthesize` itself fail.
    #[must_use]
    pub fn failing_open(mut self, error: TtsError) -> Self {
        self.open_error = Some(error);
        self
    }

    fn steps(&self) -> Vec<Step<TtsItem, TtsError>> {
        let step = |outcome| Step {
            delay: self.chunk_delay,
            outcome,
        };
        let delivered = self
            .fail_after
            .as_ref()
            .map_or(self.chunk_samples.len(), |(n, _)| {
                (*n).min(self.chunk_samples.len())
            });
        let mut steps: Vec<_> = self.chunk_samples[..delivered]
            .iter()
            .enumerate()
            .map(|(i, &n)| {
                let value = (i + 1) as f32 / 100.0;
                step(Ok(TtsItem::Chunk(PcmChunk {
                    samples: vec![value; n],
                    rate: FAKE_TTS_RATE,
                })))
            })
            .collect();
        match &self.fail_after {
            Some((_, error)) => steps.push(step(Err(error.clone()))),
            None => {
                let total: usize = self.chunk_samples.iter().sum();
                let audio_secs = FAKE_TTS_RATE.duration_of(total).as_secs_f32();
                steps.push(step(Ok(TtsItem::Done { audio_secs })));
            }
        }
        steps
    }
}

struct TtsShared {
    clock: Arc<dyn Clock>,
    descriptor: Mutex<ProviderDescriptor>,
    by_text: Mutex<HashMap<String, TtsScript>>,
    default_script: Mutex<Option<TtsScript>>,
    presets: Vec<String>,
    prepared: Mutex<HashSet<String>>,
    requests: Mutex<Vec<TtsRequest>>,
    log: EventLog,
}

/// A [`SpeechSynth`] that answers from scripts keyed by the text.
///
/// Voices behave like the real contract: a preset must be one of the fake's
/// presets (`preset-a`, `preset-b`) and a cloned voice must have been
/// prepared with [`SpeechSynth::prepare_voice`] and not released since.
/// Requests are named by their clause id in the [`EventLog`]. A text without
/// a script and without a default script is rejected with `Unavailable`.
#[derive(Clone)]
pub struct FakeSpeechSynth {
    shared: Arc<TtsShared>,
}

impl FakeSpeechSynth {
    /// A synthesiser with no scripts, timed on `clock`.
    pub fn new(clock: impl Clock + 'static) -> Self {
        let descriptor = ProviderDescriptor::new("fake-tts", "fake", StreamingKind::Native)
            .with_languages([Lang::Zh, Lang::En])
            .with_sample_rates([FAKE_TTS_RATE]);
        Self {
            shared: Arc::new(TtsShared {
                clock: Arc::new(clock),
                descriptor: Mutex::new(descriptor),
                by_text: Mutex::new(HashMap::new()),
                default_script: Mutex::new(None),
                presets: vec!["preset-a".to_owned(), "preset-b".to_owned()],
                prepared: Mutex::new(HashSet::new()),
                requests: Mutex::new(Vec::new()),
                log: EventLog::new(),
            }),
        }
    }

    /// Requests whose text is `text` play `script`.
    #[must_use]
    pub fn with_script(self, text: &str, script: TtsScript) -> Self {
        self.shared.by_text.lock().insert(text.to_owned(), script);
        self
    }

    /// Requests without a script of their own play `script`.
    #[must_use]
    pub fn with_default_script(self, script: TtsScript) -> Self {
        *self.shared.default_script.lock() = Some(script);
        self
    }

    /// Replace the descriptor, for example to make the provider non-streaming.
    #[must_use]
    pub fn with_descriptor(self, descriptor: ProviderDescriptor) -> Self {
        *self.shared.descriptor.lock() = descriptor;
        self
    }

    /// The log of every request's life.
    pub fn log(&self) -> EventLog {
        self.shared.log.clone()
    }

    /// Every request accepted so far, in order.
    pub fn requests(&self) -> Vec<TtsRequest> {
        self.shared.requests.lock().clone()
    }

    fn check_voice(&self, voice: &VoiceSelection) -> Result<(), TtsError> {
        match voice {
            VoiceSelection::Preset { name } if self.shared.presets.contains(name) => Ok(()),
            VoiceSelection::Preset { name } => Err(TtsError::UnknownVoice(name.clone())),
            VoiceSelection::Cloned { profile_id }
                if self.shared.prepared.lock().contains(profile_id) =>
            {
                Ok(())
            }
            VoiceSelection::Cloned { profile_id } => {
                Err(TtsError::UnknownVoice(profile_id.clone()))
            }
        }
    }
}

fn handle_of(profile_id: &str) -> String {
    format!("fake-voice-{profile_id}")
}

impl SpeechSynth for FakeSpeechSynth {
    fn descriptor(&self) -> ProviderDescriptor {
        self.shared.descriptor.lock().clone()
    }

    fn prepare_voice(&self, reference: &VoiceReference) -> Result<PreparedVoice, TtsError> {
        self.shared
            .descriptor
            .lock()
            .require_lang(reference.lang)
            .map_err(|_| TtsError::UnsupportedLanguage(reference.lang))?;
        if reference.samples.is_empty() {
            return Err(TtsError::Internal("empty reference recording".to_owned()));
        }
        self.shared
            .prepared
            .lock()
            .insert(reference.profile_id.clone());
        Ok(PreparedVoice {
            handle: handle_of(&reference.profile_id),
            engine: "fake".to_owned(),
            model_hash: "fake-model-hash".to_owned(),
            blobs: Vec::new(),
        })
    }

    fn release_voice(&self, handle: &str) -> Result<(), TtsError> {
        let mut prepared = self.shared.prepared.lock();
        let profile = prepared.iter().find(|p| handle_of(p) == handle).cloned();
        match profile {
            Some(p) => {
                prepared.remove(&p);
                Ok(())
            }
            None => Err(TtsError::UnknownVoice(handle.to_owned())),
        }
    }

    fn synthesize(&self, req: TtsRequest) -> Result<Box<dyn TtsStream>, TtsError> {
        self.shared
            .descriptor
            .lock()
            .require_lang(req.lang)
            .map_err(|_| TtsError::UnsupportedLanguage(req.lang))?;
        self.check_voice(&req.voice)?;
        let script = self
            .shared
            .by_text
            .lock()
            .get(&req.text)
            .cloned()
            .or_else(|| self.shared.default_script.lock().clone())
            .ok_or_else(|| TtsError::Unavailable {
                reason: format!("no script for `{}`", req.text),
            })?;
        if let Some(error) = &script.open_error {
            return Err(error.clone());
        }
        let voice = req.voice.clone();
        let request = req.clause.to_string();
        self.shared.requests.lock().push(req);
        let paced = PacedStream::open(
            Arc::clone(&self.shared.clock),
            script.steps(),
            request,
            self.shared.log.clone(),
            |i: &TtsItem| matches!(i, TtsItem::Done { .. }),
            || TtsError::Cancelled,
        );
        Ok(Box::new(FakeTtsStream { paced, voice }))
    }
}

struct FakeTtsStream {
    paced: PacedStream<TtsItem, TtsError>,
    voice: VoiceSelection,
}

impl TtsStream for FakeTtsStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<TtsItem, TtsError>> {
        self.paced.next(cancel)
    }

    fn voice_used(&self) -> &VoiceSelection {
        &self.voice
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use prxvc_media::{ClauseId, Direction, ManualClock, SegmentId};

    use super::*;
    use crate::fakes::log::EventKind;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    fn preset(name: &str) -> VoiceSelection {
        VoiceSelection::Preset {
            name: name.to_owned(),
        }
    }

    fn request(direction: Direction, text: &str, voice: VoiceSelection) -> TtsRequest {
        TtsRequest {
            clause: ClauseId::new(SegmentId::first(direction), 0),
            text: text.to_owned(),
            lang: Lang::En,
            voice,
        }
    }

    fn drain(
        stream: &mut dyn TtsStream,
        cancel: &CancellationToken,
    ) -> Vec<Result<TtsItem, TtsError>> {
        std::iter::from_fn(|| stream.next(cancel)).collect()
    }

    fn reference(profile: &str) -> VoiceReference {
        VoiceReference {
            profile_id: profile.to_owned(),
            samples: vec![0.1; 48_000],
            rate: SampleRate::HZ_48000,
            transcript: Some("a reference sentence".to_owned()),
            lang: Lang::En,
        }
    }

    #[test]
    fn chunks_stream_in_order_and_done_reports_the_total() {
        let tts = FakeSpeechSynth::new(ManualClock::new())
            .with_default_script(TtsScript::chunks(2, 2400));
        let mut s = tts
            .synthesize(request(Direction::Incoming, "hello", preset("preset-a")))
            .expect("stream");
        let items = drain(s.as_mut(), &CancellationToken::new());
        assert_eq!(items.len(), 3);
        let Ok(TtsItem::Chunk(first)) = &items[0] else {
            panic!("first item is a chunk: {items:?}")
        };
        let Ok(TtsItem::Chunk(second)) = &items[1] else {
            panic!("second item is a chunk: {items:?}")
        };
        assert_eq!(first.rate, FAKE_TTS_RATE);
        assert_eq!(first.duration(), MS(100));
        assert_eq!(
            (first.samples[0], second.samples[0]),
            (0.01, 0.02),
            "chunks are distinguishable"
        );
        assert_eq!(items[2], Ok(TtsItem::Done { audio_secs: 0.2 }));
        assert_eq!(s.voice_used(), &preset("preset-a"));
    }

    #[test]
    fn cancelling_mid_stream_stops_that_request_while_another_direction_keeps_speaking() {
        let clock = ManualClock::new();
        let script = TtsScript::chunks(3, 240).with_chunk_delay(MS(10));
        let tts = FakeSpeechSynth::new(clock.clone()).with_default_script(script);
        let mut outgoing = tts
            .synthesize(request(Direction::Outgoing, "a", preset("preset-a")))
            .expect("out");
        let mut incoming = tts
            .synthesize(request(Direction::Incoming, "b", preset("preset-b")))
            .expect("in");
        let (cancel_out, cancel_in) = (CancellationToken::new(), CancellationToken::new());

        let out_thread = {
            let token = cancel_out.clone();
            thread::spawn(move || {
                (
                    outgoing.next(&token),
                    outgoing.next(&token),
                    outgoing.next(&token),
                )
            })
        };
        let in_thread = {
            let token = cancel_in.clone();
            thread::spawn(move || drain(incoming.as_mut(), &token))
        };

        clock.advance(MS(10));
        while !tts.log().kinds_for("out#1.0").contains(&EventKind::Item) {
            thread::yield_now();
        }
        cancel_out.cancel();
        let (first, second, after) = out_thread.join().expect("outgoing worker");
        assert!(matches!(first, Some(Ok(TtsItem::Chunk(_)))));
        assert_eq!(second, Some(Err(TtsError::Cancelled)));
        assert_eq!(after, None, "no audio is produced for a cancelled request");

        clock.advance(MS(30));
        let incoming_items = in_thread.join().expect("incoming worker");
        assert_eq!(incoming_items.len(), 4);
        assert!(matches!(
            incoming_items.last(),
            Some(Ok(TtsItem::Done { .. }))
        ));
        assert_eq!(
            tts.log().kinds_for("out#1.0").last(),
            Some(&EventKind::Cancelled)
        );
        assert_eq!(
            tts.log().kinds_for("in#1.0").last(),
            Some(&EventKind::Completed)
        );
    }

    #[test]
    fn a_runaway_is_an_injected_failure_after_the_chosen_chunk() {
        let script = TtsScript::chunks(5, 240).failing_after(
            2,
            TtsError::Runaway {
                produced_secs: 30.0,
            },
        );
        let tts = FakeSpeechSynth::new(ManualClock::new()).with_default_script(script);
        let mut s = tts
            .synthesize(request(Direction::Outgoing, "x", preset("preset-a")))
            .expect("stream");
        let items = drain(s.as_mut(), &CancellationToken::new());
        assert_eq!(items.len(), 3);
        assert_eq!(
            items[2],
            Err(TtsError::Runaway {
                produced_secs: 30.0
            })
        );
        assert_eq!(
            tts.log().kinds_for("out#1.0").last(),
            Some(&EventKind::Failed)
        );
    }

    #[test]
    fn a_cloned_voice_must_be_prepared_first_and_stops_working_once_released() {
        let tts =
            FakeSpeechSynth::new(ManualClock::new()).with_default_script(TtsScript::chunks(1, 240));
        let cloned = VoiceSelection::Cloned {
            profile_id: "me".to_owned(),
        };
        assert_eq!(
            tts.synthesize(request(Direction::Outgoing, "x", cloned.clone()))
                .err(),
            Some(TtsError::UnknownVoice("me".to_owned()))
        );

        let prepared = tts.prepare_voice(&reference("me")).expect("prepared");
        assert_eq!(
            (prepared.engine.as_str(), prepared.handle.as_str()),
            ("fake", "fake-voice-me")
        );
        let s = tts
            .synthesize(request(Direction::Outgoing, "x", cloned.clone()))
            .expect("stream");
        assert_eq!(s.voice_used(), &cloned);

        tts.release_voice(&prepared.handle).expect("released");
        assert!(
            tts.synthesize(request(Direction::Outgoing, "x", cloned))
                .is_err()
        );
        assert_eq!(
            tts.release_voice(&prepared.handle),
            Err(TtsError::UnknownVoice(prepared.handle.clone()))
        );
    }

    #[test]
    fn an_unknown_preset_and_an_unsupported_language_are_refused() {
        let tts =
            FakeSpeechSynth::new(ManualClock::new()).with_default_script(TtsScript::chunks(1, 1));
        assert_eq!(
            tts.synthesize(request(Direction::Outgoing, "x", preset("nobody")))
                .err(),
            Some(TtsError::UnknownVoice("nobody".to_owned()))
        );
        let en_only = tts.descriptor().with_languages([Lang::En]);
        let tts = tts.with_descriptor(en_only);
        let mut zh = request(Direction::Outgoing, "x", preset("preset-a"));
        zh.lang = Lang::Zh;
        assert_eq!(
            tts.synthesize(zh).err(),
            Some(TtsError::UnsupportedLanguage(Lang::Zh))
        );
    }

    #[test]
    fn open_failure_and_a_missing_script_are_typed_errors() {
        let tts = FakeSpeechSynth::new(ManualClock::new()).with_script(
            "bad",
            TtsScript::chunks(1, 1).failing_open(TtsError::Rejected { status: 500 }),
        );
        assert_eq!(
            tts.synthesize(request(Direction::Outgoing, "bad", preset("preset-a")))
                .err(),
            Some(TtsError::Rejected { status: 500 })
        );
        assert!(matches!(
            tts.synthesize(request(Direction::Outgoing, "other", preset("preset-a")))
                .err(),
            Some(TtsError::Unavailable { .. })
        ));
    }

    #[test]
    fn capabilities_declare_the_output_rate_and_refuse_a_non_streaming_provider() {
        let tts = FakeSpeechSynth::new(ManualClock::new());
        let d = tts.descriptor();
        assert!(d.require_native().is_ok());
        assert!(d.require_rate(SampleRate::HZ_24000).is_ok());
        assert!(d.require_rate(SampleRate::HZ_16000).is_err());
        let batch = ProviderDescriptor::new("batch-tts", "m", StreamingKind::None);
        assert!(
            tts.with_descriptor(batch)
                .descriptor()
                .require_live()
                .is_err()
        );
    }

    #[test]
    fn preparing_a_voice_checks_the_recording() {
        let tts = FakeSpeechSynth::new(ManualClock::new());
        let mut empty = reference("me");
        empty.samples.clear();
        assert!(matches!(
            tts.prepare_voice(&empty),
            Err(TtsError::Internal(_))
        ));
    }
}
