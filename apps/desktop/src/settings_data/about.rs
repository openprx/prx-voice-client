//! Facts for the About page: version, where the data, the settings and the log live, and the licence notices.

use std::path::{Path, PathBuf};

use prxvc_config::AppConfig;
use prxvc_config::paths::AppPaths;
use prxvc_desktop_ui::settings_data::AboutData;

use crate::logging::LOG_FILE_NAME;

/// Name of the file with the third-party notices.
const NOTICES_FILE: &str = "THIRD_PARTY_NOTICES.md";

/// The About facts for `paths`; `resources` is the bundle's `Contents/Resources`, when bundled.
pub(crate) fn about(paths: Option<&AppPaths>, resources: Option<&Path>) -> AboutData {
    let text = |p: PathBuf| p.display().to_string();
    let log = paths
        .map(|p| p.logs_dir.join(LOG_FILE_NAME))
        .filter(|p| p.is_file());
    AboutData {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        data_dir: paths.map(|p| text(p.data_dir.clone())).unwrap_or_default(),
        models_dir: paths
            .map(|p| text(p.models_dir.clone()))
            .unwrap_or_default(),
        config_file: paths
            .map(|p| text(p.config_dir.join(prxvc_config::CONFIG_FILE_NAME)))
            .unwrap_or_default(),
        log_file: log.map(text),
        notices_file: notices_file(resources).map(text),
    }
}

/// The notices file: in the bundle's resources, or, in a development build, in the repository.
pub(crate) fn notices_file(resources: Option<&Path>) -> Option<PathBuf> {
    let in_bundle = resources.map(|r| r.join(NOTICES_FILE));
    let in_repository = cfg!(debug_assertions).then(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(NOTICES_FILE)
    });
    in_bundle
        .into_iter()
        .chain(in_repository)
        .find(|p| p.is_file())
        .map(|p| p.canonicalize().unwrap_or(p))
}

/// Provider endpoints that stay reachable in offline mode: entries marked as local endpoints,
/// as `(name, host:port)`.
pub(crate) fn local_endpoints(config: &AppConfig) -> Vec<(String, String)> {
    config
        .providers
        .entries
        .iter()
        .filter(|e| e.local_endpoint)
        .filter_map(|e| {
            let url = url::Url::parse(&e.base_url).ok()?;
            let host = url.host_str()?;
            let address = match url.port_or_known_default() {
                Some(port) => format!("{host}:{port}"),
                None => host.to_owned(),
            };
            let name = if e.display_name.is_empty() {
                e.id.clone()
            } else {
                e.display_name.clone()
            };
            Some((name, address))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use prxvc_config::ProviderEntry;

    #[test]
    fn about_without_directories_is_still_a_version() {
        let a = about(None, None);
        assert_eq!(a.version, env!("CARGO_PKG_VERSION"));
        assert!(a.data_dir.is_empty() && a.log_file.is_none());
    }

    #[test]
    fn the_log_file_is_shown_only_when_it_exists() {
        let dir = tempfile::tempdir().expect("tmp");
        let config = AppConfig::default();
        let mut paths = AppPaths::resolve(&config).expect("paths");
        paths.logs_dir = dir.path().to_path_buf();
        assert!(about(Some(&paths), None).log_file.is_none());
        std::fs::write(dir.path().join(LOG_FILE_NAME), "x").expect("write");
        let log = about(Some(&paths), None).log_file.expect("a log file");
        assert!(log.ends_with(LOG_FILE_NAME));
    }

    #[test]
    fn the_notices_are_found_in_the_bundle_resources() {
        let dir = tempfile::tempdir().expect("tmp");
        assert!(notices_file(Some(dir.path())).is_none() || cfg!(debug_assertions));
        std::fs::write(dir.path().join(NOTICES_FILE), "x").expect("write");
        let found = notices_file(Some(dir.path())).expect("found");
        assert!(found.ends_with(NOTICES_FILE));
    }

    #[test]
    fn only_local_endpoints_are_listed_with_their_port() {
        let mut config = AppConfig::default();
        let entry = |id: &str, name: &str, url: &str, local: bool| ProviderEntry {
            id: id.into(),
            display_name: name.into(),
            base_url: url.into(),
            local_endpoint: local,
            ..ProviderEntry::default()
        };
        config.providers.entries = vec![
            entry(
                "llama",
                "Local llama-server",
                "http://127.0.0.1:8080/v1",
                true,
            ),
            entry("nas", "", "http://192.168.1.20/v1", true),
            entry("openai", "OpenAI", "https://api.openai.com/v1", false),
            entry("broken", "Broken", "not a url", true),
        ];
        assert_eq!(
            local_endpoints(&config),
            [
                ("Local llama-server".to_owned(), "127.0.0.1:8080".to_owned()),
                ("nas".to_owned(), "192.168.1.20:80".to_owned()),
            ]
        );
    }
}
