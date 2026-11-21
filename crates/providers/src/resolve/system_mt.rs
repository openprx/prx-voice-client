//! System translation as a translation provider: the operating system's own service, with no
//! endpoint, model file or credential. Resolution only checks what is known without asking the
//! system: that the bridge is built into this binary. The system version and the language packs
//! are found when the engine is built (`BuildError::System`), because that needs the framework.

use std::time::Duration;

use prxvc_adapters::mt::apple::{MODEL_NAME, PROVIDER_ID};
use prxvc_adapters::mt::{PromptTemplate, Timeouts};
use prxvc_media::Lang;
use prxvc_ports::ProviderId;
use serde_json::Map;

use super::Resolver;
use crate::problem::ProblemKind;
use crate::spec::{Locality, MtEngineKind, MtSpec};

/// The provider id of system translation: what `call.translation_engine = "system"` resolves to,
/// and what `providers.fallback.mt` names to use it as a configured fallback.
pub const SYSTEM_MT_PROVIDER: &str = PROVIDER_ID;

/// The runtime name in the "not built in" problem.
const RUNTIME_NAME: &str = "apple-translation";

impl Resolver<'_> {
    /// System translation of `source` to `target`.
    pub(super) fn system_mt_provider(
        &self,
        source: Lang,
        target: Lang,
    ) -> Result<MtSpec, Vec<ProblemKind>> {
        let id = ProviderId::new(SYSTEM_MT_PROVIDER);
        if !self.built.apple_translation {
            return Err(vec![ProblemKind::RuntimeNotBuiltIn {
                id,
                runtime: RUNTIME_NAME.to_owned(),
            }]);
        }
        Ok(MtSpec {
            engine: MtEngineKind::System,
            id,
            display_name: "System translation".to_owned(),
            template: None,
            base_url: String::new(),
            host: String::new(),
            locality: Locality::ThisComputer,
            model: MODEL_NAME.to_owned(),
            serves: None,
            secret: None,
            params: Map::new(),
            prompt_template: PromptTemplate::Generic,
            timeouts: Timeouts {
                connect: Duration::ZERO,
                ..Timeouts::default()
            },
            source,
            target,
        })
    }
}
