//! The assistant's dialogue backend: an OpenAI-compatible chat endpoint (the built-in
//! `local-llama-server`, or a user entry of protocol `openai-chat`), and the placeholder entry of
//! the OpenPRX agent.
//!
//! The OpenPRX agent is a separate product whose interface is not specified yet. Its entry exists
//! so that the settings can name it; it resolves to the typed problem
//! [`ProblemKind::NotAvailableYet`] and never to another backend.

use prxvc_adapters::dialogue::HistoryLimits;
use prxvc_ports::ProviderId;

use super::{DEFAULT_MT_PROVIDER, Resolver};
use crate::keys::{CallRole, RoleKey};
use crate::problem::{Problem, ProblemKind};
use crate::spec::{DialogueSpec, Resolved};

/// The id of the placeholder entry of the OpenPRX agent.
pub const OPENPRX_PROVIDER: &str = "openprx";

/// What the OpenPRX entry is called.
pub const OPENPRX_NAME: &str = "OpenPRX agent";

/// The id of the built-in dialogue provider: the same local `llama-server` the translation uses.
pub const DEFAULT_DIALOGUE_PROVIDER: &str = DEFAULT_MT_PROVIDER;

/// Where a dialogue backend of the settings list comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogueKind {
    /// The built-in local server.
    LocalServer,
    /// A provider entry of `providers.entries`.
    Entry,
    /// The OpenPRX agent: listed, not available yet.
    OpenPrx,
}

/// One dialogue backend the settings can offer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueChoice {
    /// The id `providers.active.dialogue` takes.
    pub id: String,
    /// What to call it.
    pub display_name: String,
    /// Where it comes from.
    pub kind: DialogueKind,
}

impl Resolver<'_> {
    /// The dialogue backend of `providers.active.dialogue` (the built-in local server when
    /// nothing is selected), with its fallback.
    pub fn dialogue(&self) -> Result<Resolved<DialogueSpec>, Vec<Problem>> {
        self.resolve_pair(RoleKey::Dialogue, |id| self.dialogue_provider(id))
    }

    fn dialogue_provider(&self, id: &str) -> Result<DialogueSpec, Vec<ProblemKind>> {
        let has_entry = self.cfg.providers.entries.iter().any(|e| e.id == id);
        if id == OPENPRX_PROVIDER && !has_entry {
            return Err(vec![ProblemKind::NotAvailableYet {
                id: ProviderId::new(id),
                name: OPENPRX_NAME.to_owned(),
            }]);
        }
        let ep = self.chat_provider(id, CallRole::Dialogue, None)?;
        let a = &self.cfg.assistant;
        Ok(DialogueSpec {
            id: ep.id,
            display_name: ep.display_name,
            template: ep.template,
            base_url: ep.base_url,
            host: ep.host,
            locality: ep.locality,
            model: ep.model,
            serves: ep.serves,
            secret: ep.secret,
            params: ep.params,
            timeouts: ep.timeouts,
            history: HistoryLimits {
                max_turns: usize::try_from(a.history_max_turns.max(1)).unwrap_or(1),
                max_tokens: a.history_max_tokens.max(1),
            },
        })
    }

    /// Every backend the settings can offer for the dialogue: the built-in local server, the
    /// entries that speak a chat protocol, and the OpenPRX placeholder.
    pub fn dialogue_choices(&self) -> Vec<DialogueChoice> {
        let mut out = vec![DialogueChoice {
            id: DEFAULT_DIALOGUE_PROVIDER.to_owned(),
            display_name: "Local server".to_owned(),
            kind: DialogueKind::LocalServer,
        }];
        for e in &self.cfg.providers.entries {
            let chat = super::protocol::Protocol::parse(e.protocol.trim())
                .or_else(|| {
                    self.catalogue
                        .provider(e.template.trim())
                        .and_then(|t| super::protocol::Protocol::parse(&t.protocol))
                })
                .is_some_and(|p| p.serves(CallRole::Dialogue));
            if chat && e.id != DEFAULT_DIALOGUE_PROVIDER && e.id != OPENPRX_PROVIDER {
                out.push(DialogueChoice {
                    id: e.id.clone(),
                    display_name: if e.display_name.is_empty() {
                        e.id.clone()
                    } else {
                        e.display_name.clone()
                    },
                    kind: DialogueKind::Entry,
                });
            }
        }
        out.push(DialogueChoice {
            id: OPENPRX_PROVIDER.to_owned(),
            display_name: OPENPRX_NAME.to_owned(),
            kind: DialogueKind::OpenPrx,
        });
        out
    }
}
