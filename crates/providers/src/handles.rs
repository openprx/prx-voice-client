//! Per-direction provider handles over a shared engine.
//!
//! Each `build_*` call returns a new handle that implements the role's port by delegating to the
//! shared engine and keeps a [`Lease`] on it, so the engine lives as long as any handle does. A
//! handle that sees an engine-level failure retires the engine (see `engines`); a per-request
//! failure (a cancellation, a runaway guard, a rejected text) does not.

use std::sync::Arc;

use prxvc_adapters::mt::OpenAiChatMt;
use prxvc_adapters::tts::qwentts::QwenTts;
use prxvc_media::{Lang, SampleRate};
use prxvc_ports::{
    AsrContext, AsrError, AsrHypothesis, AsrStream, CancellationToken, MtError, MtRequest,
    MtStream, PreparedVoice, ProviderDescriptor, RefineAsr, SpeechSynth, StreamingAsr, Translator,
    TtsError, TtsItem, TtsRequest, TtsStream, VadEngine, VadError, VadProvider, VoiceReference,
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

/// Whether a synthesis error means the engine itself is unusable.
fn tts_engine_failure(e: &TtsError) -> bool {
    matches!(e, TtsError::Unavailable { .. } | TtsError::Internal(_))
}

/// A synthesis handle.
pub struct SharedSynth {
    pub(crate) lease: Lease<QwenTts>,
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
        let opened = self.lease.engine().open();
        if matches!(
            opened,
            Err(VadError::Unavailable { .. } | VadError::Internal(_))
        ) {
            self.lease.report_failure();
        }
        opened
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
        let opened = self.lease.engine().open(lang, ctx);
        if let Err(e) = &opened
            && asr_engine_failure(e)
        {
            self.lease.report_failure();
        }
        opened
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
