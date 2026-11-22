//! Per-direction provider handles over a shared engine.
//!
//! Each `build_*` call returns a new handle that implements the role's port by delegating to the
//! shared engine and keeps a [`Lease`] on it, so the engine lives as long as any handle does. A
//! handle that sees an engine-level failure retires the engine (see `engines`); a per-request
//! failure (a cancellation, a runaway guard, a rejected text) does not.

use std::sync::Arc;

use prxvc_adapters::dialogue::OpenAiChatDialogue;
use prxvc_adapters::mt::OpenAiChatMt;
use prxvc_adapters::mt::apple::AppleSystemMt;
use prxvc_adapters::tts::qwentts::QwenTts;
use prxvc_media::{Lang, MonoTime, SampleRate};
use prxvc_ports::{
    AsrContext, AsrError, AsrHypothesis, AsrStream, CancellationToken, ConversationId,
    DialogueBackend, DialogueError, DialogueRequest, DialogueStream, MtError, MtRequest, MtStream,
    PreparedVoice, ProviderDescriptor, RefineAsr, SpeechSynth, StreamingAsr, Translator, TtsError,
    TtsItem, TtsRequest, TtsStream, VadEngine, VadError, VadProvider, VoiceReference,
    VoiceSelection,
};

use crate::engines::Lease;

/// A translation handle.
pub struct SharedTranslator {
    pub(crate) lease: Lease<OpenAiChatMt>,
}

impl SharedTranslator {
    /// The lease, to compare engines or retire the engine.
    pub fn lease(&self) -> &Lease<OpenAiChatMt> {
        &self.lease
    }
}

impl Translator for SharedTranslator {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        // The client is stateless: no failure of a request is a reason to rebuild it.
        self.lease.engine().translate(req)
    }
}

/// A system-translation handle.
pub struct SharedSystemTranslator {
    pub(crate) lease: Lease<AppleSystemMt>,
}

impl SharedSystemTranslator {
    /// The lease, to compare engines or retire the engine.
    pub fn lease(&self) -> &Lease<AppleSystemMt> {
        &self.lease
    }
}

impl Translator for SharedSystemTranslator {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        let started = self.lease.engine().translate(req);
        // A system-level failure (not a cancellation or a slow answer) means the engine's session
        // is unusable; the next build creates a new one.
        if matches!(started, Err(MtError::Unavailable { .. })) {
            self.lease.report_failure();
        }
        started
    }
}

/// A dialogue handle.
pub struct SharedDialogue {
    pub(crate) lease: Lease<OpenAiChatDialogue>,
}

impl SharedDialogue {
    /// The lease, to compare engines or retire the engine.
    pub fn lease(&self) -> &Lease<OpenAiChatDialogue> {
        &self.lease
    }
}

impl DialogueBackend for SharedDialogue {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn respond(&self, req: DialogueRequest) -> Result<Box<dyn DialogueStream>, DialogueError> {
        // The client holds only the conversations: no failure of a request is a reason to
        // rebuild it (that would forget them).
        self.lease.engine().respond(req)
    }

    fn end_conversation(&self, conversation: &ConversationId) {
        self.lease.engine().end_conversation(conversation);
    }
}

/// Whether a synthesis error means the engine itself is unusable.
fn tts_engine_failure(e: &TtsError) -> bool {
    matches!(e, TtsError::Unavailable { .. } | TtsError::Internal(_))
}

/// A synthesis handle.
pub struct SharedSynth {
    pub(crate) lease: Lease<QwenTts>,
}

impl Drop for SharedSynth {
    /// The last handle on a synthesis engine says what it did for the call, just before it is freed.
    fn drop(&mut self) {
        if self.lease.is_last() {
            let s = self.lease.engine().stats();
            tracing::info!(
                requests = s.requests,
                completed = s.completed,
                cancelled = s.cancelled,
                failed = s.failed,
                runaways = s.runaways,
                retries = s.retries,
                held = s.held,
                streamed = s.streamed,
                overruns = s.overruns,
                warm_up_requests = s.warm_up_requests,
                "synthesis engine totals"
            );
        }
    }
}

impl SharedSynth {
    /// The lease, to compare engines or retire the engine.
    pub fn lease(&self) -> &Lease<QwenTts> {
        &self.lease
    }
}

impl SpeechSynth for SharedSynth {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn prepare_voice(&self, reference: &VoiceReference) -> Result<PreparedVoice, TtsError> {
        self.lease.engine().prepare_voice(reference)
    }

    fn release_voice(&self, handle: &str) -> Result<(), TtsError> {
        self.lease.engine().release_voice(handle)
    }

    fn synthesize(&self, req: TtsRequest) -> Result<Box<dyn TtsStream>, TtsError> {
        match self.lease.engine().synthesize(req) {
            Ok(inner) => Ok(Box::new(SharedTtsStream {
                inner,
                lease: self.lease.clone(),
            })),
            Err(e) => {
                if tts_engine_failure(&e) {
                    self.lease.report_failure();
                }
                Err(e)
            }
        }
    }
}

struct SharedTtsStream {
    inner: Box<dyn TtsStream>,
    lease: Lease<QwenTts>,
}

impl TtsStream for SharedTtsStream {
    fn next(&mut self, cancel: &CancellationToken) -> Option<Result<TtsItem, TtsError>> {
        let item = self.inner.next(cancel);
        if let Some(Err(e)) = &item
            && tts_engine_failure(e)
        {
            self.lease.report_failure();
        }
        item
    }

    fn voice_used(&self) -> &VoiceSelection {
        self.inner.voice_used()
    }
}

/// A voice activity detection handle.
pub struct SharedVad {
    pub(crate) lease: Lease<Arc<dyn VadProvider>>,
}

impl VadProvider for SharedVad {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn open(&self) -> Result<Box<dyn VadEngine>, VadError> {
        match self.lease.engine().open() {
            Ok(inner) => Ok(Box::new(SharedVadEngine {
                inner,
                lease: self.lease.clone(),
            })),
            Err(e) => {
                if vad_engine_failure(&e) {
                    self.lease.report_failure();
                }
                Err(e)
            }
        }
    }
}

fn vad_engine_failure(e: &VadError) -> bool {
    matches!(e, VadError::Unavailable { .. } | VadError::Internal(_))
}

/// An open VAD session. It keeps the engine alive for as long as it is used.
struct SharedVadEngine {
    inner: Box<dyn VadEngine>,
    lease: Lease<Arc<dyn VadProvider>>,
}

impl VadEngine for SharedVadEngine {
    fn frame_len(&self) -> usize {
        self.inner.frame_len()
    }

    fn rate(&self) -> SampleRate {
        self.inner.rate()
    }

    fn probability(&mut self, frame: &[f32]) -> Result<f32, VadError> {
        let result = self.inner.probability(frame);
        if let Err(e) = &result
            && vad_engine_failure(e)
        {
            self.lease.report_failure();
        }
        result
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

fn asr_engine_failure(e: &AsrError) -> bool {
    matches!(e, AsrError::Unavailable { .. } | AsrError::Internal(_))
}

/// A streaming recognition handle.
pub struct SharedAsr {
    pub(crate) lease: Lease<Arc<dyn StreamingAsr>>,
}

impl StreamingAsr for SharedAsr {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn open(&self, lang: Lang, ctx: AsrContext) -> Result<Box<dyn AsrStream>, AsrError> {
        match self.lease.engine().open(lang, ctx) {
            Ok(inner) => Ok(Box::new(SharedAsrStream {
                inner,
                lease: self.lease.clone(),
            })),
            Err(e) => {
                if asr_engine_failure(&e) {
                    self.lease.report_failure();
                }
                Err(e)
            }
        }
    }
}

/// An open recognition stream. It keeps the engine alive while the direction uses it, and an
/// engine-level failure of any call retires the engine.
struct SharedAsrStream {
    inner: Box<dyn AsrStream>,
    lease: Lease<Arc<dyn StreamingAsr>>,
}

impl SharedAsrStream {
    fn watch<T>(&self, result: Result<T, AsrError>) -> Result<T, AsrError> {
        if let Err(e) = &result
            && asr_engine_failure(e)
        {
            self.lease.report_failure();
        }
        result
    }
}

impl AsrStream for SharedAsrStream {
    fn feed(&mut self, audio: &[f32], ts: MonoTime) -> Result<Option<AsrHypothesis>, AsrError> {
        let result = self.inner.feed(audio, ts);
        self.watch(result)
    }

    fn finalize(&mut self, cancel: &CancellationToken) -> Result<AsrHypothesis, AsrError> {
        let result = self.inner.finalize(cancel);
        self.watch(result)
    }

    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// A refine-pass recognition handle.
pub struct SharedRefine {
    pub(crate) lease: Lease<Arc<dyn RefineAsr>>,
}

impl RefineAsr for SharedRefine {
    fn descriptor(&self) -> ProviderDescriptor {
        self.lease.engine().descriptor()
    }

    fn transcribe(
        &self,
        audio: &[f32],
        rate: SampleRate,
        lang: Lang,
        ctx: &AsrContext,
        cancel: &CancellationToken,
    ) -> Result<AsrHypothesis, AsrError> {
        let result = self
            .lease
            .engine()
            .transcribe(audio, rate, lang, ctx, cancel);
        if let Err(e) = &result
            && asr_engine_failure(e)
        {
            self.lease.report_failure();
        }
        result
    }
}
