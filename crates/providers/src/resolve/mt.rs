//! Translation providers: an OpenAI-compatible chat endpoint from a user entry, the built-in
//! `local-llama-server`, or a catalogue model served by `llama-server`.

use std::str::FromStr;
use std::time::Duration;

use prxvc_adapters::mt::{OpenAiChatConfig, PromptTemplate, Timeouts};
use prxvc_config::{Blocked, ProviderEntry};
use prxvc_media::Lang;
use prxvc_models::catalogue::ProviderTemplate;
use prxvc_models::{LanguageSpec, ModelId, Role, Streaming};
use prxvc_ports::ProviderId;
use serde_json::{Map, Value};
use url::Url;

use super::local::not_ready_problem;
use super::protocol::Protocol;
use super::{DEFAULT_MT_PROVIDER, DEFAULT_MT_TEMPLATE, Resolver};
use crate::credentials::{CredentialReadError, CredentialRef, Secret};
use crate::keys::CallRole;
use crate::problem::ProblemKind;
use crate::runtime::Runtime;
use crate::schema::apply_schema;
use crate::spec::{MtSpec, locality_of};

/// An endpoint provider before its checks, from whichever source named it.
struct Draft {
    id: String,
    display_name: String,
    template: Option<String>,
    protocol: String,
    base_url: String,
    model: String,
    credential: String,
    key_found_in_file: bool,
    params: Map<String, Value>,
    serves: Option<ModelId>,
}

impl Draft {
    fn from_entry(e: &ProviderEntry) -> Self {
        Self {
            id: e.id.clone(),
            display_name: if e.display_name.is_empty() {
                e.id.clone()
            } else {
                e.display_name.clone()
            },
            template: Some(e.template.trim().to_owned()).filter(|t| !t.is_empty() && t != "custom"),
            protocol: e.protocol.trim().to_owned(),
            base_url: e.base_url.trim().to_owned(),
            model: e.model.trim().to_owned(),
            credential: e.credential.clone(),
            key_found_in_file: e.pending_api_key.is_some()
                || e.pending_token.is_some()
                || e.pending_secret.is_some(),
            params: match serde_json::to_value(&e.params) {
                Ok(Value::Object(m)) => m,
                _ => Map::new(),
            },
            serves: None,
        }
    }

    fn from_template(id: &str, template: &ProviderTemplate) -> Self {
        Self {
            id: id.to_owned(),
            display_name: template.display_name.clone(),
            template: Some(template.id.clone()),
            protocol: template.protocol.clone(),
            base_url: String::new(),
            model: String::new(),
            credential: String::new(),
            key_found_in_file: false,
            params: Map::new(),
            serves: template.default_model.as_deref().map(ModelId::new),
        }
    }
}

fn duration_ms(params: &mut Map<String, Value>, key: &str) -> Result<Option<Duration>, String> {
    match params.remove(key) {
        None => Ok(None),
        Some(v) => match v.as_u64().filter(|ms| *ms > 0) {
            Some(ms) => Ok(Some(Duration::from_millis(ms))),
            None => Err("must be a positive number of milliseconds".to_owned()),
        },
    }
}

/// What an OpenAI-compatible chat provider resolves to, whichever role it serves.
pub(super) struct ChatEndpoint {
    pub(super) id: ProviderId,
    pub(super) display_name: String,
    pub(super) template: Option<String>,
    pub(super) base_url: String,
    pub(super) host: String,
    pub(super) locality: crate::spec::Locality,
    pub(super) model: String,
    pub(super) serves: Option<ModelId>,
    pub(super) secret: Option<Secret>,
    pub(super) params: Map<String, Value>,
    pub(super) prompt_template: PromptTemplate,
    pub(super) timeouts: Timeouts,
}

/// The sampling temperature of a spoken assistant when the entry sets none.
const DIALOGUE_TEMPERATURE: f64 = 0.7;
/// How long a dialogue server may take to its first token, and to go quiet, when the entry sets
/// neither: a reply starts later than the translation of one clause.
const DIALOGUE_FIRST_TOKEN: Duration = Duration::from_secs(15);
const DIALOGUE_IDLE: Duration = Duration::from_secs(8);

impl Resolver<'_> {
    pub(super) fn mt_provider(
        &self,
        id: &str,
        source: Lang,
        target: Lang,
    ) -> Result<MtSpec, Vec<ProblemKind>> {
        let ep = self.chat_provider(id, CallRole::Mt, Some((source, target)))?;
        Ok(MtSpec {
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
            prompt_template: ep.prompt_template,
            timeouts: ep.timeouts,
            source,
            target,
        })
    }

    /// A chat endpoint for `role` (translation of `pair`, or the assistant's dialogue): a user
    /// entry, the built-in `local-llama-server`, or a catalogue model served by `llama-server`.
    pub(super) fn chat_provider(
        &self,
        id: &str,
        role: CallRole,
        pair: Option<(Lang, Lang)>,
    ) -> Result<ChatEndpoint, Vec<ProblemKind>> {
        let template_of = |t: &str| self.catalogue.provider(t);
        let (draft, model_problems) =
            if let Some(entry) = self.cfg.providers.entries.iter().find(|e| e.id == id) {
                (Draft::from_entry(entry), Vec::new())
            } else if id == DEFAULT_MT_PROVIDER {
                let Some(template) = template_of(DEFAULT_MT_TEMPLATE) else {
                    return Err(vec![ProblemKind::UnknownTemplate {
                        entry: id.to_owned(),
                        template: DEFAULT_MT_TEMPLATE.to_owned(),
                    }]);
                };
                (Draft::from_template(id, template), Vec::new())
            } else if let Some(model) = self.catalogue.get(&ModelId::new(id)) {
                let pid = ProviderId::new(id);
                if model.role != Role::Mt {
                    return Err(vec![ProblemKind::KindNotValidForRole {
                        id: pid,
                        role,
                        found: format!("a {:?} model", model.role),
                    }]);
                }
                let runtime = Runtime::parse(&model.runtime.name);
                if runtime != Runtime::LlamaServer {
                    return Err(vec![ProblemKind::RuntimeNotBuiltIn {
                        id: pid,
                        runtime: runtime.name().to_owned(),
                    }]);
                }
                let Some(template) = template_of(DEFAULT_MT_TEMPLATE) else {
                    return Err(vec![ProblemKind::UnknownTemplate {
                        entry: id.to_owned(),
                        template: DEFAULT_MT_TEMPLATE.to_owned(),
                    }]);
                };
                let mut draft = Draft::from_template(id, template);
                draft.display_name = model.display_name.clone();
                draft.model = id.to_owned();
                draft.serves = Some(model.id.clone());
                if let Some(name) = model
                    .runtime
                    .params
                    .as_ref()
                    .and_then(|p| p.get("prompt_template"))
                {
                    draft
                        .params
                        .insert("prompt_template".to_owned(), name.clone());
                }
                let problems = match self.models.ready(&model.id) {
                    Ok(_) => Vec::new(),
                    Err(nr) => vec![not_ready_problem(&pid, nr)],
                };
                (draft, problems)
            } else {
                return Err(vec![ProblemKind::UnknownProvider { id: id.to_owned() }]);
            };
        let mut problems = model_problems;
        match self.finish_chat(draft, role, pair) {
            Ok(spec) if problems.is_empty() => Ok(spec),
            Ok(_) => Err(problems),
            Err(mut more) => {
                problems.append(&mut more);
                Err(problems)
            }
        }
    }

    fn finish_chat(
        &self,
        draft: Draft,
        role: CallRole,
        pair: Option<(Lang, Lang)>,
    ) -> Result<ChatEndpoint, Vec<ProblemKind>> {
        let pid = ProviderId::new(draft.id.clone());
        let entry = draft.id.clone();
        let mut problems = Vec::new();

        let template = match &draft.template {
            Some(t) => {
                let found = self.catalogue.provider(t);
                if found.is_none() {
                    problems.push(ProblemKind::UnknownTemplate {
                        entry: entry.clone(),
                        template: t.clone(),
                    });
                }
                found
            }
            None => None,
        };

        // Protocol: the entry's, else its template's.
        let protocol_text = if draft.protocol.is_empty() {
            template.map(|t| t.protocol.clone()).unwrap_or_default()
        } else {
            draft.protocol.clone()
        };
        match Protocol::parse(&protocol_text) {
            None => {
                problems.push(ProblemKind::UnknownProtocol {
                    entry: entry.clone(),
                    protocol: protocol_text,
                });
                return Err(problems);
            }
            Some(p) if !p.serves(role) => {
                problems.push(ProblemKind::KindNotValidForRole {
                    id: pid,
                    role,
                    found: format!("an endpoint of protocol `{protocol_text}`"),
                });
                return Err(problems);
            }
            Some(Protocol::OllamaChat) => {
                problems.push(ProblemKind::RuntimeNotBuiltIn {
                    id: pid,
                    runtime: protocol_text,
                });
                return Err(problems);
            }
            Some(_) => {}
        }

        if let Some(t) = template {
            match t.streaming {
                Streaming::Native => {}
                Streaming::Redecode | Streaming::Segmented | Streaming::None => {
                    problems.push(ProblemKind::NotStreaming {
                        id: pid.clone(),
                        reason: t.ineligible_reason.as_ref().map_or_else(
                            || "The endpoint does not stream.".to_owned(),
                            |r| r.en.clone(),
                        ),
                    })
                }
            }
        }

        // Address, then the offline policy: no connection is attempted either way.
        let base = if draft.base_url.is_empty() {
            template
                .map(|t| t.default_base_url.clone())
                .unwrap_or_default()
        } else {
            draft.base_url.clone()
        };
        let parsed = match Url::parse(&base) {
            Ok(u) if matches!(u.scheme(), "http" | "https") && u.host().is_some() => Some(u),
            Ok(_) | Err(_) => {
                problems.push(ProblemKind::InvalidEndpoint {
                    entry: entry.clone(),
                    detail: format!("`{base}` is not an http(s) address"),
                });
                None
            }
        };
        if let Some(url) = &parsed
            && let Err(blocked) = self.policy.check(url.as_str())
        {
            problems.push(match blocked {
                Blocked::Offline { host } => ProblemKind::EndpointBlocked {
                    entry: entry.clone(),
                    host,
                },
                Blocked::MalformedUrl { detail, .. } => ProblemKind::InvalidEndpoint {
                    entry: entry.clone(),
                    detail,
                },
            });
        }

        let model = if draft.model.is_empty() {
            template
                .and_then(|t| t.default_model.clone())
                .unwrap_or_default()
        } else {
            draft.model.clone()
        };
        if model.is_empty() {
            problems.push(ProblemKind::InvalidParams {
                id: pid.clone(),
                param: "model".to_owned(),
                reason: "is empty".to_owned(),
            });
        } else if let Some((source, target)) = pair
            && let Some(t) = template
            && let Some(known) = t.known_models.iter().find(|k| k.name == model)
            && let Some(langs) = known.languages.as_ref().filter(|l| !l.is_empty())
            && !pair_listed(langs, source, target)
        {
            problems.push(ProblemKind::PairUnsupported {
                id: pid.clone(),
                from: source,
                to: target,
            });
        }

        let secret = self.read_secret(
            &draft,
            template.is_some_and(|t| t.needs_credential),
            &mut problems,
        );

        let params = self.mt_params(&draft, template, role, &pid, &mut problems);

        let (Some(url), Some(params)) = (parsed, params) else {
            return Err(problems);
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        let host = url.host_str().unwrap_or_default().to_owned();
        Ok(ChatEndpoint {
            id: pid,
            display_name: draft.display_name,
            template: draft.template,
            base_url: base,
            host,
            locality: locality_of(url.host()),
            model,
            serves: draft.serves,
            secret,
            params: params.body,
            prompt_template: params.prompt_template,
            timeouts: params.timeouts,
        })
    }

    fn read_secret(
        &self,
        draft: &Draft,
        needs_credential: bool,
        problems: &mut Vec<ProblemKind>,
    ) -> Option<Secret> {
        let entry = draft.id.clone();
        let reference = match CredentialRef::parse(&draft.credential) {
            Ok(r) => r,
            Err(bad) => {
                problems.push(ProblemKind::CredentialUnreadable {
                    entry,
                    detail: bad.0,
                });
                return None;
            }
        };
        if reference == CredentialRef::None {
            if draft.key_found_in_file {
                problems.push(ProblemKind::CredentialUnreadable {
                    entry,
                    detail: "a key was found in config.toml but no credential store holds it"
                        .to_owned(),
                });
            } else if needs_credential {
                problems.push(ProblemKind::CredentialMissing { entry });
            }
            return None;
        }
        match self.creds.read(&reference) {
            Ok(secret) => Some(secret),
            Err(CredentialReadError::NotFound) => {
                problems.push(ProblemKind::CredentialMissing { entry });
                None
            }
            Err(CredentialReadError::Unavailable(detail)) => {
                problems.push(ProblemKind::CredentialUnreadable { entry, detail });
                None
            }
        }
    }

    /// Schema defaults, the entry's values, then the adapter-owned keys taken out of the body.
    fn mt_params(
        &self,
        draft: &Draft,
        template: Option<&ProviderTemplate>,
        role: CallRole,
        pid: &ProviderId,
        problems: &mut Vec<ProblemKind>,
    ) -> Option<ResolvedParams> {
        let invalid = |param: &str, reason: String| ProblemKind::InvalidParams {
            id: pid.clone(),
            param: param.to_owned(),
            reason,
        };
        let schema = template.and_then(|t| t.params_schema.as_ref());
        let mut ok = true;
        // A schema violation does not hide the other problems: the rest is checked on the raw values.
        let mut merged = match apply_schema(schema, &draft.params) {
            Ok(m) => m,
            Err(errors) => {
                problems.extend(errors.into_iter().map(|e| invalid(&e.param, e.reason)));
                ok = false;
                draft.params.clone()
            }
        };
        let prompt_template = match merged.remove("prompt_template") {
            None => PromptTemplate::Generic,
            Some(v) => match v.as_str().map(PromptTemplate::from_str) {
                Some(Ok(t)) => t,
                _ => {
                    problems.push(invalid(
                        "prompt_template",
                        "is not a known template name".to_owned(),
                    ));
                    ok = false;
                    PromptTemplate::Generic
                }
            },
        };
        let mut timeouts = Timeouts::default();
        for (key, slot) in [
            ("connect_timeout_ms", &mut timeouts.connect),
            ("first_token_timeout_ms", &mut timeouts.first_token),
            ("idle_timeout_ms", &mut timeouts.idle),
        ] {
            match duration_ms(&mut merged, key) {
                Ok(Some(d)) => *slot = d,
                Ok(None) => {}
                Err(reason) => {
                    problems.push(invalid(key, reason));
                    ok = false;
                }
            }
        }
        // The adapter owns some body fields; ask it so the list lives in one place.
        let mut body = OpenAiChatConfig::new("http://placeholder.invalid", "m").params;
        body.extend(merged);
        if role == CallRole::Dialogue {
            // A spoken assistant is not translating: the template's temperature of zero and its
            // translation timeouts apply only when the entry sets them itself.
            if !draft.params.contains_key("temperature") {
                body.insert("temperature".to_owned(), DIALOGUE_TEMPERATURE.into());
            }
            if !draft.params.contains_key("first_token_timeout_ms") {
                timeouts.first_token = DIALOGUE_FIRST_TOKEN;
            }
            if !draft.params.contains_key("idle_timeout_ms") {
                timeouts.idle = DIALOGUE_IDLE;
            }
        }
        let probe =
            OpenAiChatConfig::new("http://placeholder.invalid", "m").with_params(body.clone());
        if let Err(prxvc_adapters::mt::ConfigError::ReservedParam(key)) = probe.validate() {
            problems.push(invalid(
                &key,
                "is set by the adapter and cannot be overridden".to_owned(),
            ));
            ok = false;
        }
        ok.then_some(ResolvedParams {
            body,
            prompt_template,
            timeouts,
        })
    }
}

struct ResolvedParams {
    body: Map<String, Value>,
    prompt_template: PromptTemplate,
    timeouts: Timeouts,
}

/// Whether `langs` (a known model's languages) lists the pair, as a pair or as two languages.
fn pair_listed(langs: &[LanguageSpec], source: Lang, target: Lang) -> bool {
    let has_lang = |l: Lang| {
        langs.iter().any(|s| match s {
            LanguageSpec::Any => true,
            LanguageSpec::Lang { code } => code == l.code(),
            LanguageSpec::Pair { .. } => false,
        })
    };
    langs.iter().any(|s| {
        matches!(s, LanguageSpec::Pair { source: a, target: b }
            if a == source.code() && b == target.code())
    }) || (has_lang(source) && has_lang(target))
}
