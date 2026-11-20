//! Model catalogue: the schema types of design section 4.1.1, parsing and
//! lookup. Validation is in `validate`, candidate ordering in `select`.

mod select;
mod validate;

pub use select::{Candidate, NotSelectable};

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::hardware::{Arch, HardwareInfo, Os};

/// The only catalogue schema version this build understands.
pub const SUPPORTED_SCHEMA_VERSION: u32 = 1;

const EMBEDDED_JSON: &str = include_str!("../../catalogue/catalogue.json");

// ---------------------------------------------------------------------------
// Basic identifiers
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Role {
    Vad,
    Asr,
    Mt,
    Tts,
    TtsFallback,
}

impl Role {
    /// Live roles must stream (4.5): a `streaming = none` entry cannot be active.
    pub fn is_live(self) -> bool {
        !matches!(self, Role::Vad)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn new(id: impl Into<String>) -> Self {
        ModelId(id.into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The language key a role is selected by (4.5).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LangKey {
    Any,
    Lang(String),
    Pair { source: String, target: String },
}

impl LangKey {
    /// `""` for `Any`, `"zh"` for a language, `"zh>en"` for a pair.
    pub fn config_key(&self) -> String {
        match self {
            LangKey::Any => String::new(),
            LangKey::Lang(l) => l.clone(),
            LangKey::Pair { source, target } => format!("{source}>{target}"),
        }
    }

    /// Inverse of [`LangKey::config_key`].
    pub fn from_config_key(key: &str) -> LangKey {
        if key.is_empty() {
            LangKey::Any
        } else if let Some((s, t)) = key.split_once('>') {
            LangKey::Pair {
                source: s.to_string(),
                target: t.to_string(),
            }
        } else {
            LangKey::Lang(key.to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Schema types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct LocalisedText {
    pub en: String,
    #[serde(rename = "zh-CN")]
    pub zh_cn: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum LanguageSpec {
    Any,
    Lang { code: String },
    Pair { source: String, target: String },
}

impl LanguageSpec {
    pub fn matches(&self, key: &LangKey) -> bool {
        match (self, key) {
            (LanguageSpec::Any, LangKey::Any) => true,
            (LanguageSpec::Lang { code }, LangKey::Lang(l)) => code == l,
            (
                LanguageSpec::Pair { source, target },
                LangKey::Pair {
                    source: s,
                    target: t,
                },
            ) => source == s && target == t,
            _ => false,
        }
    }

    fn kind_name(&self) -> &'static str {
        match self {
            LanguageSpec::Any => "any",
            LanguageSpec::Lang { .. } => "lang",
            LanguageSpec::Pair { .. } => "pair",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct RuntimeSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_runtime_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct ModelScopeSource {
    pub repo: String,
    pub revision: String,
    pub path: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum FileSource {
    Huggingface {
        repo: String,
        revision: String,
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modelscope: Option<ModelScopeSource>,
    },
    Url {
        url: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileSpec {
    pub name: String,
    pub purpose: String,
    pub size: u64,
    pub sha256: String,
    pub source: FileSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Licence {
    pub code: String,
    pub weights: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
    pub weights_url: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Streaming {
    Native,
    Segmented,
    None,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    Recommended,
    Fastest,
    MostAccurate,
    Smallest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MeasuredBy {
    Vendor,
    Project,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Quality {
    pub value: f64,
    pub dataset: String,
    pub source: String,
    pub measured_by: MeasuredBy,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct RecommendRule {
    pub badge: Badge,
    pub rank: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<LanguageSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_memory_gb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_gb: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub os: Option<Vec<Os>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub arch: Option<Vec<Arch>>,
}

impl RecommendRule {}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ModelEntry {
    pub id: ModelId,
    pub role: Role,
    pub display_name: String,
    pub description: LocalisedText,
    pub version_label: String,
    pub languages: Vec<LanguageSpec>,
    pub runtime: RuntimeSpec,
    pub files: Vec<FileSpec>,
    pub licence: Licence,
    pub memory_mb: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disk_mb: Option<u32>,
    pub streaming: Streaming,
    #[serde(default)]
    pub recommended_for: Vec<RecommendRule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quality: Option<BTreeMap<String, Quality>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supersedes: Option<Vec<ModelId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_file_names: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hidden: Option<bool>,
}

impl ModelEntry {
    pub fn is_hidden(&self) -> bool {
        self.hidden.unwrap_or(false)
    }

    /// Total bytes of all files.
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// `disk_mb` when given, otherwise derived from the files (rounded up).
    pub fn disk_mb(&self) -> u32 {
        self.disk_mb
            .unwrap_or_else(|| self.total_size().div_ceil(1_000_000).min(u32::MAX as u64) as u32)
    }

    /// First 12 hex characters of the first file's revision (Hugging Face
    /// sources) or of its SHA-256 (URL sources); see design 4.3.
    pub fn revision_key(&self) -> String {
        let first = &self.files[0];
        let basis = match &first.source {
            FileSource::Huggingface { revision, .. } => revision.as_str(),
            FileSource::Url { .. } => first.sha256.as_str(),
        };
        basis.chars().take(12).collect()
    }

    pub fn file_by_purpose(&self, purpose: &str) -> Option<&FileSpec> {
        self.files.iter().find(|f| f.purpose == purpose)
    }

    fn best_rule(&self, key: &LangKey, hw: &HardwareInfo) -> Option<&RecommendRule> {
        self.recommended_for
            .iter()
            .filter(|r| r.matches(key, hw))
            .min_by_key(|r| r.rank)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct SourceDef {
    pub kind: String,
    pub base_url: String,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct VoicePreset {
    pub id: String,
    pub model_id: ModelId,
    pub language: String,
    pub display_name: String,
    pub file: FileSpec,
    pub licence: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EgressKind {
    Audio,
    Text,
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct KnownModel {
    pub name: String,
    pub roles: Vec<Role>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub languages: Option<Vec<LanguageSpec>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Cloud or server API template (design 4.8.1).
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct ProviderTemplate {
    pub id: String,
    pub display_name: String,
    pub protocol: String,
    pub roles: Vec<Role>,
    pub default_base_url: String,
    pub local_endpoint_default: bool,
    pub needs_credential: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_help_url: Option<String>,
    #[serde(default)]
    pub known_models: Vec<KnownModel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    pub streaming: Streaming,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ineligible_reason: Option<LocalisedText>,
    pub egress: EgressKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params_schema: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_headers: Option<BTreeMap<String, String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum PresetRole {
    LocalModel,
    Api { template: String },
}

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct SetupPreset {
    pub id: String,
    pub display_name: LocalisedText,
    pub roles: BTreeMap<Role, PresetRole>,
}

// ---------------------------------------------------------------------------
// Errors and warnings
// ---------------------------------------------------------------------------

#[derive(Debug, thiserror::Error)]
pub enum CatalogueError {
    #[error("catalogue is not valid JSON for this schema: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported catalogue schema_version {0}")]
    UnsupportedSchema(u32),
    #[error("catalogue failed validation: {}", .0.join("; "))]
    Invalid(Vec<String>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CatalogueWarning {
    /// `supersedes` names an id that is not in the catalogue (it may have
    /// existed earlier, so this is only a warning).
    UnknownSupersedes { model: ModelId, id: ModelId },
    /// The runtime is not compiled into this build.
    UnsupportedRuntime { model: ModelId, runtime: String },
    /// The catalogue needs a newer app.
    NeedsNewerApp { required: String, current: String },
}

// ---------------------------------------------------------------------------
// Catalogue
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
pub struct Catalogue {
    pub schema_version: u32,
    pub catalogue_version: u64,
    pub min_app_version: String,
    #[serde(default)]
    pub sources: BTreeMap<String, SourceDef>,
    pub models: Vec<ModelEntry>,
    #[serde(default)]
    pub providers: Vec<ProviderTemplate>,
    #[serde(default)]
    pub setup_presets: Vec<SetupPreset>,
    #[serde(default)]
    pub presets: Vec<VoicePreset>,
}

impl Catalogue {
    /// The catalogue shipped inside the binary. Panics only if the embedded
    /// file is invalid, which a unit test rules out.
    pub fn embedded() -> Catalogue {
        Catalogue::parse(EMBEDDED_JSON).expect("embedded catalogue.json must be valid")
    }

    /// Parse and structurally validate (design 4.1.3). A schema version this
    /// build does not know is rejected before anything else is looked at.
    pub fn parse(json: &str) -> Result<Catalogue, CatalogueError> {
        #[derive(Deserialize)]
        struct Head {
            schema_version: u32,
        }
        let head: Head = serde_json::from_str(json)?;
        if head.schema_version != SUPPORTED_SCHEMA_VERSION {
            return Err(CatalogueError::UnsupportedSchema(head.schema_version));
        }
        let cat: Catalogue = serde_json::from_str(json)?;
        let errors = cat.structural_errors();
        if errors.is_empty() {
            Ok(cat)
        } else {
            Err(CatalogueError::Invalid(errors))
        }
    }

    pub fn get(&self, id: &ModelId) -> Option<&ModelEntry> {
        self.models.iter().find(|m| &m.id == id)
    }

    pub fn models(&self) -> &[ModelEntry] {
        &self.models
    }

    pub fn provider(&self, id: &str) -> Option<&ProviderTemplate> {
        self.providers.iter().find(|p| p.id == id)
    }

    pub fn setup_preset(&self, id: &str) -> Option<&SetupPreset> {
        self.setup_presets.iter().find(|p| p.id == id)
    }
}

/// Models grouped by role in catalogue order (helper for UIs).
pub fn group_by_role(models: &[ModelEntry]) -> HashMap<Role, Vec<&ModelEntry>> {
    let mut map: HashMap<Role, Vec<&ModelEntry>> = HashMap::new();
    for m in models {
        map.entry(m.role).or_default().push(m);
    }
    map
}
