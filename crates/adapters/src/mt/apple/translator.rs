//! `AppleSystemMt`: the translation port over the operating system's own translation service.

use std::fmt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use prxvc_media::Lang;
use prxvc_ports::{
    Egress, MtError, MtRequest, MtStream, ProviderDescriptor, StreamingKind, Translator,
};

use super::api::{Strategy, SystemSession, SystemTranslationApi};
use super::fault::SystemFault;
use super::language::tag;
use super::stream::{SystemStream, spawn_request};

/// The provider id of the engine in `providers.active` and in the registry.
pub const PROVIDER_ID: &str = "apple-system-translation";
/// The model name shown for the engine.
pub const MODEL_NAME: &str = "Apple on-device translation";

/// How long each phase may take.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SystemTimeouts {
    /// Loading the pair's resources and the warm-up translation.
    pub prepare: Duration,
    /// One translation: tens of milliseconds when warm, so a generous bound that still ends a
    /// stuck call well inside a conversation.
    pub translate: Duration,
}

impl Default for SystemTimeouts {
    fn default() -> Self {
        Self {
            prepare: Duration::from_secs(15),
            translate: Duration::from_secs(5),
        }
    }
}

/// The configuration of one engine: one language pair, one direction of a call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SystemTranslationConfig {
    /// Language spoken.
    pub source: Lang,
    /// Language produced.
    pub target: Lang,
    /// Framework strategy.
    pub strategy: Strategy,
    /// Timeouts.
    pub timeouts: SystemTimeouts,
}

impl SystemTranslationConfig {
    /// A configuration with the default strategy and timeouts.
    pub fn new(source: Lang, target: Lang) -> Self {
        Self {
            source,
            target,
            strategy: Strategy::Default,
            timeouts: SystemTimeouts::default(),
        }
    }
}

/// What the request threads of one engine share.
pub(super) struct Shared {
    pub(super) session: Arc<dyn SystemSession>,
    /// Serialises the calls on the session (the framework session is one-at-a-time).
    pub(super) gate: Mutex<()>,
    /// The request whose call is inside the session right now (0: none), so that a cancel never
    /// interrupts another request's call.
    pub(super) active: AtomicU64,
    next_id: AtomicU64,
    pub(super) translate_timeout: Duration,
}

impl Shared {
    pub(super) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }
}

/// System translation of one language pair.
///
/// The engine is whole-text: the framework returns the translation of a text at once, not token
/// by token, so a request delivers exactly one `MtDelta::Text` and then `Done`. That is
/// acceptable on the live path because the translation of one committed clause takes tens of
/// milliseconds when warm (23 to 96 ms measured), shorter than the time the first token of a
/// language model takes; there is nothing to stream earlier. It is declared as
/// `StreamingKind::Segmented`, and the registry admits it on the live translation role through
/// `require_live`, not `require_native`.
///
/// It uses no context and follows no instructions: `MtRequest::context`, `glossary` and
/// `strict_language` are ignored, and the descriptor says so.
#[derive(Clone)]
pub struct AppleSystemMt {
    shared: Arc<Shared>,
    descriptor: ProviderDescriptor,
    cfg: SystemTranslationConfig,
}

impl fmt::Debug for AppleSystemMt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppleSystemMt")
            .field("source", &self.cfg.source)
            .field("target", &self.cfg.target)
            .finish()
    }
}

impl AppleSystemMt {
    /// Open a session for the configured pair. Blocks while the framework answers; checks that the
    /// system and the pack are there, so an engine exists only for a pair that can translate.
    ///
    /// # Errors
    /// `OsTooOld`, `PackNotInstalled`, `PairUnsupported` or a framework failure, each typed.
    pub fn open(
        api: &dyn SystemTranslationApi,
        cfg: SystemTranslationConfig,
    ) -> Result<Self, SystemFault> {
        api.os_support()?;
        let session = api.open(tag(cfg.source), tag(cfg.target), cfg.strategy)?;
        let descriptor = ProviderDescriptor::new(PROVIDER_ID, MODEL_NAME, StreamingKind::Segmented)
            .with_languages([cfg.source, cfg.target])
            .with_egress(Egress::Local)
            .with_text_guidance(false, false);
        Ok(Self {
            shared: Arc::new(Shared {
                session,
                gate: Mutex::new(()),
                active: AtomicU64::new(0),
                next_id: AtomicU64::new(1),
                translate_timeout: cfg.timeouts.translate,
            }),
            descriptor,
            cfg,
        })
    }

    /// Absorb the cold first call (about 630 ms on the reference machine): load the pair's
    /// resources and translate one short phrase, discarding it. Call it when the call starts,
    /// before the first clause arrives. Returns how long it took.
    ///
    /// # Errors
    /// Timeout or a framework failure.
    pub fn warm_up(&self) -> Result<Duration, SystemFault> {
        let started = Instant::now();
        self.shared.session.prepare(self.cfg.timeouts.prepare)?;
        let phrase = match self.cfg.source {
            Lang::Zh => "你好。",
            Lang::En => "Hello.",
        };
        self.shared
            .session
            .translate(phrase, self.cfg.timeouts.prepare)?;
        Ok(started.elapsed())
    }

    /// The language pair this engine translates.
    pub fn pair(&self) -> (Lang, Lang) {
        (self.cfg.source, self.cfg.target)
    }
}

impl Translator for AppleSystemMt {
    fn descriptor(&self) -> ProviderDescriptor {
        self.descriptor.clone()
    }

    fn translate(&self, req: MtRequest) -> Result<Box<dyn MtStream>, MtError> {
        if (req.source, req.target) != (self.cfg.source, self.cfg.target) {
            return Err(MtError::Unavailable {
                reason: format!(
                    "this engine translates {} to {}, not {} to {}",
                    self.cfg.source, self.cfg.target, req.source, req.target
                ),
            });
        }
        if req.text.trim().is_empty() {
            return Err(MtError::Internal("the source clause is empty".into()));
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let (id, rx) = spawn_request(&self.shared, &req.text, Arc::clone(&cancelled));
        Ok(Box::new(SystemStream::new(
            Arc::clone(&self.shared),
            req,
            id,
            rx,
            cancelled,
        )))
    }
}
