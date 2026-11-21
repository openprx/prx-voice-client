//! Starting a real session: providers from the registry, the microphone, the output and the gate on this machine.

use std::sync::{Arc, LazyLock};
use std::time::Duration;

use prxvc_audio::call_pump::EventQueue;
use prxvc_config::AppConfig;
use prxvc_desktop_ui::AssistantIssue;
use prxvc_media::{Clock, SystemClock};
use prxvc_ports::ConversationId;
use prxvc_session::assistant::{AssistantDeps, AssistantHandle, start};

use super::audio::{PlayerOutput, echo_gate, microphone};
use super::issues;
use super::kit::{self, Prepared};
use super::warm::WarmCache;
use super::params::session_params;
use crate::call_kit::trace::NullTrace;

/// What a finished session leaves loaded for the next one (see `warm`).
static WARM: LazyLock<WarmCache<Prepared>> =
    LazyLock::new(|| WarmCache::new(Arc::new(SystemClock::new())));

/// Free the engines a finished assistant session kept warm (call mode is starting).
pub(crate) fn release_warm() {
    WARM.release();
}

/// Free them when their time is over; the host calls it every turn.
pub(crate) fn expire_warm() {
    WARM.expire();
}

/// What the providers of a session are built from: when it is the same, the engines are reused.
fn warm_key(config: &AppConfig, system: Option<&str>) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{:?}",
        config.providers, config.assistant, config.voice, config.models, system
    )
}

/// A session that is running.
pub(crate) struct Launched {
    /// The loop.
    pub handle: AssistantHandle,
    /// The microphone's device changes, when the source reports them.
    pub devices: Option<EventQueue>,
    /// What to do when the session has ended (release the registry and report its engines).
    pub on_end: Option<Box<dyn FnOnce() + Send>>,
    /// The language the session speaks, as `[assistant].language` spells it.
    pub language: String,
}

/// Starts a session. The application has one implementation; the tests have another.
pub(crate) trait Launcher: Send {
    /// Resolve, build and start a session for `config`.
    ///
    /// # Errors
    /// Every issue that keeps it from starting; nothing is left running.
    fn launch(&mut self, config: &AppConfig) -> Result<Launched, Vec<AssistantIssue>>;
}

/// The launcher of the application.
pub(crate) struct RealLauncher;

impl Launcher for RealLauncher {
    fn launch(&mut self, config: &AppConfig) -> Result<Launched, Vec<AssistantIssue>> {
        let system = sys_locale::get_locale();
        let key = warm_key(config, system.as_deref());
        let prepared = match WARM.take(&key) {
            Some(warm) => {
                tracing::info!("the assistant's engines were still loaded; reusing them");
                warm
            }
            None => kit::prepare(config, crate::config_load::app_paths(), system.as_deref())?,
        };
        let keep = Duration::from_secs(u64::from(config.assistant.keep_warm_s));
        let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
        let (source, devices) = microphone(config, Arc::clone(&clock));
        let params = session_params(
            config,
            prepared.lang,
            prepared.voice.clone(),
            ConversationId::new(uuid::Uuid::new_v4().to_string()),
        );
        let language = match prepared.lang {
            prxvc_media::Lang::Zh => "zh-CN",
            prxvc_media::Lang::En => "en",
        }
        .to_owned();
        let deps = AssistantDeps {
            source,
            vad: Arc::clone(&prepared.vad),
            asr: Arc::clone(&prepared.asr),
            dialogue: Arc::clone(&prepared.dialogue),
            tts: Arc::clone(&prepared.tts),
            output: Box::new(PlayerOutput::new(config)),
            gate: Box::new(echo_gate(config, clock.as_ref())),
            clock,
            trace: Arc::new(NullTrace),
        };
        let handle = start(deps, params).map_err(|e| vec![issues::from_start_error(&e)])?;
        // Kept for a while after the session so that a second click starts at once; the model
        // server's hold is kept with them, so that the server is still running for it.
        let registry = Arc::clone(&prepared.registry);
        let kept = prepared;
        Ok(Launched {
            handle,
            devices: Some(devices),
            on_end: Some(Box::new(move || {
                WARM.keep(key, kept, keep);
                let live = registry.live_engines();
                tracing::info!(
                    ?live,
                    keep_warm_s = keep.as_secs(),
                    "the assistant session ended; engines still held by handles are listed"
                );
            })),
            language,
        })
    }
}
