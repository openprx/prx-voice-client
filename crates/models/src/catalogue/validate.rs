//! Catalogue validation: structural rules of design 4.1.3 (enforced by
//! `Catalogue::parse`) and the soft checks of `Catalogue::validate`. Single
//! responsibility: say whether a catalogue is acceptable.

use std::collections::HashSet;

use super::*;

impl Catalogue {
    /// Soft checks: unsupported runtimes, a catalogue that needs a newer app,
    /// and `supersedes` ids that do not exist.
    pub fn validate(
        &self,
        app_version: &semver::Version,
        runtimes: &[&str],
    ) -> Vec<CatalogueWarning> {
        let mut out = Vec::new();
        if let Ok(min) = semver::Version::parse(&self.min_app_version)
            && &min > app_version
        {
            out.push(CatalogueWarning::NeedsNewerApp {
                required: self.min_app_version.clone(),
                current: app_version.to_string(),
            });
        }
        let ids: HashSet<&ModelId> = self.models.iter().map(|m| &m.id).collect();
        for m in &self.models {
            if !runtimes.contains(&m.runtime.name.as_str()) {
                out.push(CatalogueWarning::UnsupportedRuntime {
                    model: m.id.clone(),
                    runtime: m.runtime.name.clone(),
                });
            }
            for s in m.supersedes.iter().flatten() {
                if !ids.contains(s) {
                    out.push(CatalogueWarning::UnknownSupersedes {
                        model: m.id.clone(),
                        id: s.clone(),
                    });
                }
            }
        }
        out
    }

    pub(super) fn structural_errors(&self) -> Vec<String> {
        let mut e = Vec::new();
        if semver::Version::parse(&self.min_app_version).is_err() {
            e.push(format!(
                "min_app_version '{}' is not semver",
                self.min_app_version
            ));
        }
        if self.models.is_empty() {
            e.push("models is empty".into());
        }
        let mut seen = HashSet::new();
        for m in &self.models {
            if !seen.insert(&m.id) {
                e.push(format!("duplicate model id '{}'", m.id));
            }
            check_model(m, &mut e);
        }
        let mut pids = HashSet::new();
        for p in &self.providers {
            if !pids.insert(&p.id) {
                e.push(format!("duplicate provider id '{}'", p.id));
            }
            if p.roles.is_empty() {
                e.push(format!("provider '{}' has no roles", p.id));
            }
            if p.protocol.is_empty() || p.default_base_url.is_empty() {
                e.push(format!(
                    "provider '{}' needs protocol and default_base_url",
                    p.id
                ));
            }
            if p.streaming == Streaming::None && p.ineligible_reason.is_none() {
                e.push(format!(
                    "provider '{}' does not stream but has no ineligible_reason",
                    p.id
                ));
            }
        }
        for sp in &self.setup_presets {
            for (role, r) in &sp.roles {
                if let PresetRole::Api { template } = r {
                    match self.provider(template) {
                        None => e.push(format!(
                            "setup preset '{}' role {role:?} names unknown template '{template}'",
                            sp.id
                        )),
                        Some(t) if !t.roles.contains(role) => e.push(format!(
                            "setup preset '{}': template '{template}' does not serve role {role:?}",
                            sp.id
                        )),
                        _ => {}
                    }
                }
            }
        }
        let mut vids = HashSet::new();
        for v in &self.presets {
            if !vids.insert(&v.id) {
                e.push(format!("duplicate voice preset id '{}'", v.id));
            }
            if self.get(&v.model_id).is_none() {
                e.push(format!(
                    "voice preset '{}' names unknown model '{}'",
                    v.id, v.model_id
                ));
            }
            check_file(&v.file, &format!("voice preset '{}'", v.id), &mut e);
        }
        e
    }
}

fn valid_model_id(id: &str) -> bool {
    let b = id.as_bytes();
    (2..=64).contains(&b.len())
        && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit())
        && b.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || *c == b'.' || *c == b'-')
}

fn valid_file_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('.')
        && !name.contains("..")
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'.' || c == b'_' || c == b'-')
}

fn is_lower_hex(s: &str, len: usize) -> bool {
    s.len() == len
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

fn valid_repo_path(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split('/').any(|s| s.is_empty() || s == "..")
}

fn check_file(f: &FileSpec, ctx: &str, e: &mut Vec<String>) {
    if !valid_file_name(&f.name) {
        e.push(format!("{ctx}: invalid file name '{}'", f.name));
    }
    if f.size == 0 {
        e.push(format!("{ctx}: file '{}' has size 0", f.name));
    }
    if !is_lower_hex(&f.sha256, 64) {
        e.push(format!(
            "{ctx}: file '{}' sha256 must be 64 lowercase hex",
            f.name
        ));
    }
    match &f.source {
        FileSource::Huggingface {
            repo,
            revision,
            path,
            modelscope,
        } => {
            if !is_lower_hex(revision, 40) {
                e.push(format!(
                    "{ctx}: file '{}' revision '{revision}' must be a 40-hex commit (branch names are not pins)",
                    f.name
                ));
            }
            if !valid_repo_path(repo) || !valid_repo_path(path) {
                e.push(format!(
                    "{ctx}: file '{}' has an invalid repo or path",
                    f.name
                ));
            }
            if let Some(ms) = modelscope
                && (!valid_repo_path(&ms.repo)
                    || !valid_repo_path(&ms.path)
                    || ms.revision.is_empty())
            {
                e.push(format!(
                    "{ctx}: file '{}' has an invalid modelscope block",
                    f.name
                ));
            }
        }
        FileSource::Url { url } => match url::Url::parse(url) {
            Ok(u) if u.scheme() == "https" => {}
            _ => e.push(format!(
                "{ctx}: file '{}' url source must be https://",
                f.name
            )),
        },
    }
}

fn check_model(m: &ModelEntry, e: &mut Vec<String>) {
    let ctx = format!("model '{}'", m.id);
    if !valid_model_id(&m.id.0) {
        e.push(format!("{ctx}: id must match [a-z0-9][a-z0-9.-]{{1,63}}"));
    }
    if m.files.is_empty() {
        e.push(format!("{ctx}: files is empty"));
    }
    let mut names = HashSet::new();
    for f in &m.files {
        if !names.insert(&f.name) {
            e.push(format!("{ctx}: duplicate file name '{}'", f.name));
        }
        check_file(f, &ctx, e);
    }
    for n in m.legacy_file_names.iter().flatten() {
        if !valid_file_name(n) {
            e.push(format!("{ctx}: invalid legacy file name '{n}'"));
        }
    }
    if m.languages.is_empty() {
        e.push(format!("{ctx}: languages is empty"));
    }
    let want = match m.role {
        Role::Vad => "any",
        Role::Asr | Role::Tts | Role::TtsFallback => "lang",
        Role::Mt => "pair",
    };
    for l in &m.languages {
        if l.kind_name() != want {
            e.push(format!(
                "{ctx}: language kind '{}' is not allowed for role {:?} (expected '{want}')",
                l.kind_name(),
                m.role
            ));
        }
    }
    if m.runtime.name.is_empty() {
        e.push(format!("{ctx}: runtime name is empty"));
    }
    if m.streaming == Streaming::Redecode && m.role != Role::Asr {
        e.push(format!(
            "{ctx}: streaming 'redecode' is only meaningful for recognition models (role {:?})",
            m.role
        ));
    }
}
