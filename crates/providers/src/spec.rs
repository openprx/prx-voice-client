//! Typed provider specifications: what resolution produces and what `build_*` consumes.
//!
//! A spec is plain data. It holds no engine, so resolving a whole call touches no model and no
//! network; the `Resolved` pair carries the primary and the configured fallback as separate
//! specs because the decision to switch belongs to the pipeline (CS-2.2.4).

use std::net::IpAddr;
use std::path::PathBuf;

use prxvc_adapters::mt::{PromptTemplate, Timeouts};
use prxvc_media::Lang;
use prxvc_models::{ModelId, ReadyModel};
use prxvc_ports::{ProviderId, VoiceSelection};
use serde_json::{Map, Value};
use url::Host;

use crate::credentials::Secret;
use crate::runtime::Runtime;

/// A primary provider and, when configured and valid, its fallback.
#[derive(Clone, Debug)]
pub struct Resolved<S> {
    /// The provider of `providers.active`.
    pub primary: S,
    /// The provider of `providers.fallback`, resolved and validated the same way; `None` when no
    /// fallback is configured or it could not be resolved (the latter is a reported problem).
    pub fallback: Option<S>,
    /// Problems of the fallback (empty when there is none or it is valid). They never stop the
    /// primary from being used, but the UI shows them: a role without a working fallback.
    pub warnings: Vec<crate::problem::Problem>,
}

/// A local model run by an in-process runtime, for VAD and the recognition roles.
#[derive(Clone, Debug)]
pub struct LocalModelSpec {
    /// The catalogue model id.
    pub id: ProviderId,
    /// The language the spec was resolved for; `None` for the VAD.
    pub lang: Option<Lang>,
    /// The runtime that runs it.
    pub runtime: Runtime,
    /// The verified files.
    pub model: ReadyModel,
    /// Whether the model streams or works per segment.
    pub streaming: prxvc_models::Streaming,
}

/// Where a provider's endpoint is, for the disclosure row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Locality {
    /// Runs in this process or on this computer.
    ThisComputer,
    /// A private-network address.
    LocalNetwork,
    /// Anywhere else.
    Remote,
}

/// What a provider receives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataLeaving {
    /// Nothing leaves this process.
    Nothing,
    /// Text.
    Text,
    /// Audio.
    Audio,
}

/// The locality of a parsed URL host.
pub(crate) fn locality_of(host: Option<Host<&str>>) -> Locality {
    match host {
        Some(Host::Domain(name)) => {
            if name.trim_end_matches('.').eq_ignore_ascii_case("localhost") {
                Locality::ThisComputer
            } else {
                Locality::Remote
            }
        }
        Some(Host::Ipv4(ip)) => classify_ip(IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => classify_ip(
            ip.to_ipv4_mapped()
                .map_or(IpAddr::V6(ip), IpAddr::V4),
        ),
        None => Locality::Remote,
    }
}

fn classify_ip(ip: IpAddr) -> Locality {
    match ip {
        _ if ip.is_loopback() => Locality::ThisComputer,
        IpAddr::V4(v4) if v4.is_private() || v4.is_link_local() => Locality::LocalNetwork,
        IpAddr::V6(v6) if (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80 => {
            Locality::LocalNetwork
        }
        _ => Locality::Remote,
    }
}

/// A translation provider over an OpenAI-compatible chat endpoint.
#[derive(Clone, Debug)]
pub struct MtSpec {
    /// The provider id as configured.
    pub id: ProviderId,
    /// Display name of the entry or template.
    pub display_name: String,
    /// The template the entry came from, when any.
    pub template: Option<String>,
    /// Chat endpoint base URL.
    pub base_url: String,
    /// Host of `base_url`.
    pub host: String,
    /// Where the host is.
    pub locality: Locality,
    /// Model name sent in requests.
    pub model: String,
    /// The catalogue model this endpoint is expected to serve, for display and the download
    /// offer; not a readiness requirement (the endpoint, not the disk, is the truth).
    pub serves: Option<ModelId>,
    /// The bearer token, when the entry has a credential.
    pub secret: Option<Secret>,
    /// Extra request-body fields: schema defaults overlaid with the entry's values.
    pub params: Map<String, Value>,
    /// Prompt template of the model family.
    pub prompt_template: PromptTemplate,
    /// Timeouts.
    pub timeouts: Timeouts,
    /// Language spoken.
    pub source: Lang,
    /// Language produced.
    pub target: Lang,
}

impl MtSpec {
    /// What the endpoint receives: the text of every clause.
    pub const fn data_leaving(&self) -> DataLeaving {
        match self.locality {
            Locality::ThisComputer => DataLeaving::Nothing,
            _ => DataLeaving::Text,
        }
    }
}

/// The voice a synthesis spec speaks with, resolved to files.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoicePlan {
    /// The owner's own voice, prepared from a recorded profile.
    Cloned {
        /// The profile id.
        profile_id: String,
        /// The profile directory (`<voices>/<id>`), which holds the derived-data cache.
        profile_dir: PathBuf,
        /// The reference recording.
        reference: PathBuf,
        /// What the recording says, when known.
        transcript: Option<String>,
        /// The language of the recording.
        lang: Lang,
    },
    /// A voice that ships with the catalogue: a stored reference prompt.
    Preset {
        /// The preset id.
        name: String,
        /// The directory of the preset, which holds its derived-data cache.
        dir: PathBuf,
        /// The reference recording.
        reference: PathBuf,
        /// The language of the recording.
        lang: Lang,
    },
}

impl VoicePlan {
    /// The selection to put in a synthesis request.
    pub fn selection(&self) -> VoiceSelection {
        match self {
            Self::Cloned { profile_id, .. } => VoiceSelection::Cloned {
                profile_id: profile_id.clone(),
            },
            Self::Preset { name, .. } => VoiceSelection::Preset { name: name.clone() },
        }
    }
}

/// Model files of the in-process synthesis engine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TtsFiles {
    /// The talker model.
    pub talker: PathBuf,
    /// The codec model.
    pub codec: PathBuf,
}

/// A local synthesis model with its voice.
#[derive(Clone, Debug)]
pub struct TtsSpec {
    /// The catalogue model id.
    pub id: ProviderId,
    /// Display name of the model.
    pub label: String,
    /// The language synthesised.
    pub lang: Lang,
    /// The runtime.
    pub runtime: Runtime,
    /// The verified model.
    pub model: ReadyModel,
    /// The files the engine loads.
    pub files: TtsFiles,
    /// Fingerprint of the files from the catalogue's checksums; keys prepared voices.
    pub fingerprint: String,
    /// The voice.
    pub voice: VoicePlan,
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::Url;

    fn locality(url: &str) -> Locality {
        locality_of(Url::parse(url).unwrap().host())
    }

    #[test]
    fn hosts_are_classified_by_address() {
        assert_eq!(locality("http://127.0.0.1:8080/v1"), Locality::ThisComputer);
        assert_eq!(locality("http://localhost/v1"), Locality::ThisComputer);
        assert_eq!(locality("http://[::1]/v1"), Locality::ThisComputer);
        assert_eq!(locality("http://192.168.1.20:8080"), Locality::LocalNetwork);
        assert_eq!(locality("https://api.openai.com/v1"), Locality::Remote);
        assert_eq!(locality("http://0.0.0.0:9"), Locality::Remote);
    }
}
