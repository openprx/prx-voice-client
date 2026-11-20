//! Hand-off of API keys found in the configuration file.
//!
//! Design 2.5 and 4.8.4: a key (`api_key`, `token`, `secret`) under
//! `[providers]` is moved into the OS credential store under the provider
//! entry's id, and the file keeps only `credential = "keychain:<id>"`.
//!
//! The credential store itself belongs to the providers work package. This
//! module defines the interface ([`CredentialStore`]) and the detection and
//! file rewrite. Until a store is supplied, [`UnavailableCredentialStore`] is
//! used: it refuses every key, so the key stays in the file (and, for a
//! provider entry, in the typed config as a [`PendingSecret`]) and nothing is
//! lost.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Keys that must not appear in the file under `providers`.
const SECRET_KEYS: [&str; 3] = ["api_key", "token", "secret"];

/// Error from a [`CredentialStore`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct CredentialError(pub String);

/// Where secrets are kept. Implemented by the providers work package on top of
/// the OS keychain.
pub trait CredentialStore: Send + Sync {
    /// Store `secret` under provider entry `id`, replacing an earlier value.
    fn store(&self, id: &str, secret: &str) -> Result<(), CredentialError>;
}

/// The store used before a real one exists: refuses every key.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableCredentialStore;

impl CredentialStore for UnavailableCredentialStore {
    fn store(&self, _id: &str, _secret: &str) -> Result<(), CredentialError> {
        Err(CredentialError("no credential store is available".to_owned()))
    }
}

/// A secret held in memory because it could not be stored yet. Its `Debug`
/// output is redacted.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PendingSecret(String);

impl PendingSecret {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The secret itself.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for PendingSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PendingSecret(<redacted>)")
    }
}

/// Result of [`extract_secrets`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SecretsOutcome {
    /// Ids under which a secret was stored and removed from the table.
    pub moved: Vec<String>,
    /// Locations whose secret was refused by the store and left in place.
    pub kept: Vec<String>,
    /// The subset of `kept` that is not a provider entry (it has no `id`), so
    /// the typed config cannot hold it.
    pub stray_kept: Vec<String>,
}

/// Move every non-empty `api_key`, `token` or `secret` found anywhere under
/// `providers` into `store`.
///
/// For each key that the store accepts, the key is removed from its table and
/// `credential = "keychain:<id>"` is set in that table, where `<id>` is the
/// table's `id` (or its dotted path when it has none). A key the store refuses
/// stays untouched. An empty value holds no secret and is just removed.
pub fn extract_secrets(root: &mut toml::Table, store: &dyn CredentialStore) -> SecretsOutcome {
    let mut outcome = SecretsOutcome::default();
    if let Some(providers) = root.get_mut("providers") {
        walk(providers, "providers", store, &mut outcome);
    }
    outcome
}

fn walk(value: &mut toml::Value, path: &str, store: &dyn CredentialStore, out: &mut SecretsOutcome) {
    match value {
        toml::Value::Table(table) => {
            handle_table(table, path, store, out);
            for (key, child) in table.iter_mut() {
                walk(child, &format!("{path}.{key}"), store, out);
            }
        }
        toml::Value::Array(items) => {
            for (i, child) in items.iter_mut().enumerate() {
                walk(child, &format!("{path}[{i}]"), store, out);
            }
        }
        _ => {}
    }
}

fn handle_table(table: &mut toml::Table, path: &str, store: &dyn CredentialStore, out: &mut SecretsOutcome) {
    let own_id = table.get("id").and_then(|v| v.as_str()).filter(|s| !s.is_empty()).map(str::to_owned);
    let id = own_id.clone().unwrap_or_else(|| path.to_owned());
    let found: Vec<(String, String)> = table
        .iter()
        .filter(|(k, _)| SECRET_KEYS.iter().any(|s| k.eq_ignore_ascii_case(s)))
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
        .collect();
    let mut stored = false;
    for (key, secret) in found {
        if secret.is_empty() {
            table.remove(&key);
            continue;
        }
        match store.store(&id, &secret) {
            Ok(()) => {
                table.remove(&key);
                stored = true;
            }
            Err(e) => {
                tracing::warn!("could not store the secret `{key}` of {path}: {e}");
                out.kept.push(format!("{path}.{key}"));
                if own_id.is_none() {
                    out.stray_kept.push(format!("{path}.{key}"));
                }
            }
        }
    }
    if stored {
        table.insert("credential".to_owned(), toml::Value::String(format!("keychain:{id}")));
        out.moved.push(id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_util::FakeStore;

    fn parse(s: &str) -> toml::Table {
        toml::from_str(s).unwrap()
    }

    #[test]
    fn moves_the_key_and_sets_a_reference() {
        let mut t = parse(
            "[[providers.entries]]\nid = \"openai-mt\"\napi_key = \"sk-1\"\ncredential = \"\"\n\
             [[providers.entries]]\nid = \"b\"\ntoken = \"t-2\"\n",
        );
        let store = FakeStore::default();
        let out = extract_secrets(&mut t, &store);
        assert_eq!(out.moved, vec!["openai-mt", "b"]);
        assert!(out.kept.is_empty());
        let entries = t["providers"]["entries"].as_array().unwrap();
        assert_eq!(entries[0]["credential"].as_str(), Some("keychain:openai-mt"));
        assert!(entries[0].get("api_key").is_none());
        assert_eq!(entries[1]["credential"].as_str(), Some("keychain:b"));
        assert!(entries[1].get("token").is_none());
        let saved = store.saved.lock().unwrap();
        assert_eq!(saved.as_slice(), [("openai-mt".into(), "sk-1".into()), ("b".into(), "t-2".into())]);
    }

    #[test]
    fn refused_keys_stay_in_place() {
        let mut t = parse("[[providers.entries]]\nid = \"a\"\napi_key = \"sk-1\"\n[providers.active]\nsecret = \"x\"\n");
        let out = extract_secrets(&mut t, &UnavailableCredentialStore);
        assert!(out.moved.is_empty());
        assert_eq!(out.kept.len(), 2);
        assert_eq!(out.stray_kept, vec!["providers.active.secret"]);
        assert_eq!(t["providers"]["entries"][0]["api_key"].as_str(), Some("sk-1"));
        assert_eq!(t["providers"]["active"]["secret"].as_str(), Some("x"));
    }

    #[test]
    fn empty_values_are_dropped_and_other_sections_ignored() {
        let mut t = parse("[[providers.entries]]\nid = \"a\"\napi_key = \"\"\n[network]\nserver_url = \"x\"\ntoken = \"keep\"\n");
        let store = FakeStore::default();
        let out = extract_secrets(&mut t, &store);
        assert_eq!(out, SecretsOutcome::default());
        assert!(t["providers"]["entries"][0].get("api_key").is_none());
        assert_eq!(t["network"]["token"].as_str(), Some("keep"));
        assert!(store.saved.lock().unwrap().is_empty());
    }

    #[test]
    fn pending_secret_debug_is_redacted() {
        let s = PendingSecret::new("sk-very-secret");
        assert!(!format!("{s:?}").contains("sk-very"));
    }
}
