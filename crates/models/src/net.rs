//! Download endpoints, URL construction (design 4.2 rule 12) and the injected
//! network policy (design 4.8.7).

use url::Url;

use crate::catalogue::FileSource;

pub const HUGGINGFACE_BASE: &str = "https://huggingface.co";
pub const MODELSCOPE_BASE: &str = "https://modelscope.cn";

/// Where Hugging Face sourced files are fetched from. A user-entered custom
/// endpoint is `HuggingFace { base_url }` with that value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Endpoint {
    HuggingFace { base_url: String },
    ModelScope,
}

impl Default for Endpoint {
    fn default() -> Self {
        Endpoint::HuggingFace {
            base_url: HUGGINGFACE_BASE.to_string(),
        }
    }
}

impl Endpoint {
    /// Interpret the `models.endpoint` setting: `"huggingface"`,
    /// `"modelscope"` or an `https://` (or, for local mirrors, `http://`) base
    /// URL. `hf_endpoint_env` is the `HF_ENDPOINT` environment variable; it
    /// applies only when the setting is empty or `"huggingface"`.
    pub fn from_setting(value: &str, hf_endpoint_env: Option<&str>) -> Endpoint {
        let v = value.trim();
        match v {
            "modelscope" => Endpoint::ModelScope,
            "" | "huggingface" => match hf_endpoint_env.map(str::trim).filter(|e| !e.is_empty()) {
                Some(env) => Endpoint::HuggingFace {
                    base_url: env.trim_end_matches('/').to_string(),
                },
                None => Endpoint::default(),
            },
            custom => Endpoint::HuggingFace {
                base_url: custom.trim_end_matches('/').to_string(),
            },
        }
    }

    /// The URL a file is fetched from under this endpoint. `modelscope`
    /// endpoint + a file without a ModelScope block falls back to the default
    /// Hugging Face host; `url` sources are always used as written.
    pub fn url_for(&self, source: &FileSource) -> String {
        match source {
            FileSource::Url { url } => url.clone(),
            FileSource::Huggingface {
                repo,
                revision,
                path,
                modelscope,
            } => match (self, modelscope) {
                (Endpoint::ModelScope, Some(ms)) => {
                    format!(
                        "{MODELSCOPE_BASE}/models/{}/resolve/{}/{}",
                        ms.repo, ms.revision, ms.path
                    )
                }
                (Endpoint::HuggingFace { base_url }, _) => {
                    format!(
                        "{}/{repo}/resolve/{revision}/{path}",
                        base_url.trim_end_matches('/')
                    )
                }
                (Endpoint::ModelScope, None) => {
                    format!("{HUGGINGFACE_BASE}/{repo}/resolve/{revision}/{path}")
                }
            },
        }
    }

    /// True when `url_for(source)` is not served by ModelScope although this
    /// endpoint is ModelScope (the UI row says so).
    pub fn falls_back_to_huggingface(&self, source: &FileSource) -> bool {
        matches!(
            (self, source),
            (
                Endpoint::ModelScope,
                FileSource::Huggingface {
                    modelscope: None,
                    ..
                }
            )
        )
    }
}

/// A URL the policy refuses to contact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blocked {
    pub host: String,
    pub reason: String,
}

impl std::fmt::Display for Blocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "blocked: {} ({})", self.host, self.reason)
    }
}

/// Consulted before every connection the download engine makes, including
/// redirect targets. `prxvc_config::NetworkPolicy` is adapted to this trait by
/// the application.
pub trait NetworkPolicy: Send + Sync {
    fn check(&self, url: &Url) -> Result<(), Blocked>;
}

/// Policy that allows everything.
pub struct AllowAll;

impl NetworkPolicy for AllowAll {
    fn check(&self, _url: &Url) -> Result<(), Blocked> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalogue::ModelScopeSource;

    fn hf(ms: Option<ModelScopeSource>) -> FileSource {
        FileSource::Huggingface {
            repo: "org/repo".into(),
            revision: "a".repeat(40),
            path: "dir/file.bin".into(),
            modelscope: ms,
        }
    }

    #[test]
    fn huggingface_url() {
        let u = Endpoint::default().url_for(&hf(None));
        assert_eq!(
            u,
            format!(
                "https://huggingface.co/org/repo/resolve/{}/dir/file.bin",
                "a".repeat(40)
            )
        );
    }

    #[test]
    fn custom_base_has_trailing_slash_trimmed() {
        let e = Endpoint::from_setting("http://127.0.0.1:9/", None);
        assert!(
            e.url_for(&hf(None))
                .starts_with("http://127.0.0.1:9/org/repo/resolve/")
        );
    }

    #[test]
    fn modelscope_url_and_fallback() {
        let ms = ModelScopeSource {
            repo: "ms/repo".into(),
            revision: "master".into(),
            path: "f.bin".into(),
        };
        let with = hf(Some(ms));
        assert_eq!(
            Endpoint::ModelScope.url_for(&with),
            "https://modelscope.cn/models/ms/repo/resolve/master/f.bin"
        );
        let without = hf(None);
        assert!(
            Endpoint::ModelScope
                .url_for(&without)
                .starts_with("https://huggingface.co/org/repo/")
        );
        assert!(Endpoint::ModelScope.falls_back_to_huggingface(&without));
        assert!(!Endpoint::ModelScope.falls_back_to_huggingface(&with));
    }

    #[test]
    fn url_source_is_untouched() {
        let s = FileSource::Url {
            url: "https://example.org/x.bin".into(),
        };
        assert_eq!(
            Endpoint::ModelScope.url_for(&s),
            "https://example.org/x.bin"
        );
    }

    #[test]
    fn setting_parsing_and_hf_endpoint_env() {
        assert_eq!(
            Endpoint::from_setting("modelscope", Some("https://x")),
            Endpoint::ModelScope
        );
        assert_eq!(Endpoint::from_setting("", None), Endpoint::default());
        assert_eq!(
            Endpoint::from_setting("huggingface", Some("https://mirror.example/")),
            Endpoint::HuggingFace {
                base_url: "https://mirror.example".into()
            }
        );
        // A saved custom value wins over the environment.
        assert_eq!(
            Endpoint::from_setting("https://mine.example", Some("https://env.example")),
            Endpoint::HuggingFace {
                base_url: "https://mine.example".into()
            }
        );
    }
}
