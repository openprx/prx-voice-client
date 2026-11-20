//! Live check of the translation provider against an OpenAI-compatible server (CT-14).
//! `#[ignore]`d. No server is started: when none is listening the test reports the classified
//! connection error and passes on that classification.
//!
//! ```text
//! PRXVC_MT_URL=http://127.0.0.1:8080/v1 PRXVC_MT_MODEL=gemma-4-e4b-it-qat-q4-0 \
//!     cargo test -p prxvc-providers --test mt_live -- --ignored --nocapture
//! ```
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::time::Duration;

use prxvc_config::{AppConfig, ProviderEntry};
use prxvc_media::Lang;
use prxvc_models::{Catalogue, ModelId, NotReady, ReadyModel};
use prxvc_providers::{
    EnvCredentials, FailureClass, ModelSource, ProviderRegistry, VoiceDirs, test_provider,
};

struct NoModels;

impl ModelSource for NoModels {
    fn ready(&self, id: &ModelId) -> Result<ReadyModel, NotReady> {
        Err(NotReady::NotInstalled {
            id: id.clone(),
            size: 0,
        })
    }
}

#[test]
#[ignore = "needs an OpenAI-compatible server (PRXVC_MT_URL)"]
fn the_translation_provider_answers_or_reports_a_classified_connection_error() {
    let url = std::env::var("PRXVC_MT_URL").unwrap_or_else(|_| "http://127.0.0.1:8080/v1".into());
    let model =
        std::env::var("PRXVC_MT_MODEL").unwrap_or_else(|_| "gemma-4-e4b-it-qat-q4-0".into());
    let mut cfg = AppConfig::default();
    cfg.providers.entries.push(ProviderEntry {
        id: "live-mt".into(),
        template: "llama-server".into(),
        protocol: "openai-chat".into(),
        base_url: url.clone(),
        model,
        local_endpoint: true,
        ..ProviderEntry::default()
    });
    cfg.providers
        .active
        .mt
        .insert("zh>en".into(), "live-mt".into());
    cfg.providers
        .active
        .mt
        .insert("en>zh".into(), "live-mt".into());
    let registry = ProviderRegistry::new(
        &cfg,
        Arc::new(Catalogue::embedded()),
        Arc::new(NoModels),
        Arc::new(EnvCredentials::from_process()),
        VoiceDirs {
            profiles: "unused".into(),
            presets: "unused".into(),
        },
    );
    for (source, target) in [(Lang::En, Lang::Zh), (Lang::Zh, Lang::En)] {
        let spec = registry
            .resolve_mt(source, target)
            .expect("resolves")
            .primary;
        let provider = registry.build_mt(&spec).expect("builds without connecting");
        let report =
            test_provider::test_translator(&*provider, source, target, Duration::from_secs(20));
        println!(
            "{url} {source}>{target}: ok={} first token={:?} total={:?} text={:?} error={:?}",
            report.ok, report.first_response, report.total, report.detail, report.error
        );
        match report.error {
            None => assert!(report.ok),
            Some(e) => assert_eq!(
                e.class,
                FailureClass::Unreachable,
                "only a missing server may fail this test: {e:?}"
            ),
        }
    }
}
