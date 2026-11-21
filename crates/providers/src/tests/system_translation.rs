//! System translation in the registry: the engine choice, resolution, construction over a fake
//! operating-system service, typed problems and the absence of any silent fallback.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use prxvc_adapters::mt::apple::{
    Availability, Strategy, SystemFault, SystemFaultKind, SystemSession, SystemTranslationApi,
};
use prxvc_media::{ClauseId, Direction, Lang, SegmentId};
use prxvc_ports::{CancellationToken, MtDelta, MtRequest};

use super::fixtures::*;
use crate::problem::{BuildError, ProblemKind, Slot};
use crate::resolve::{DEFAULT_MT_PROVIDER, SYSTEM_MT_PROVIDER};
use crate::spec::{DataLeaving, Locality, MtEngineKind};

struct FakeSession;

impl SystemSession for FakeSession {
    fn prepare(&self, _t: Duration) -> Result<(), SystemFault> {
        Ok(())
    }
    fn translate(&self, text: &str, _t: Duration) -> Result<String, SystemFault> {
        Ok(format!("translated: {text}"))
    }
    fn cancel(&self) {}
}

#[derive(Default)]
struct FakeApi {
    fail: Option<SystemFaultKind>,
    opened: AtomicUsize,
}

impl SystemTranslationApi for FakeApi {
    fn os_support(&self) -> Result<(), SystemFault> {
        match self.fail {
            Some(SystemFaultKind::OsTooOld) => Err(SystemFault::new(SystemFaultKind::OsTooOld, "")),
            _ => Ok(()),
        }
    }
    fn availability(&self, _s: &str, _t: &str) -> Result<Availability, SystemFault> {
        Ok(Availability::Installed)
    }
    fn open(
        &self,
        _s: &str,
        _t: &str,
        _strategy: Strategy,
    ) -> Result<Arc<dyn SystemSession>, SystemFault> {
        self.opened.fetch_add(1, Ordering::SeqCst);
        match self.fail {
            Some(kind) if kind != SystemFaultKind::OsTooOld => {
                Err(SystemFault::new(kind, "scripted"))
            }
            _ => Ok(Arc::new(FakeSession)),
        }
    }
}

fn rig_with_system() -> Rig {
    let mut rig = Rig::new();
    rig.cfg.call.translation_engine = "system".to_owned();
    rig
}

fn registry(rig: &Rig, api: Option<Arc<FakeApi>>) -> crate::registry::ProviderRegistry {
    rig.registry()
        .with_system_translation(api.map(|a| a as Arc<dyn SystemTranslationApi>))
}

fn request(text: &str, source: Lang, target: Lang) -> MtRequest {
    MtRequest {
        clause: ClauseId::new(SegmentId::first(Direction::Outgoing), 0),
        source,
        target,
        text: text.to_owned(),
        context: Vec::new(),
        glossary: Vec::new(),
        strict_language: false,
    }
}

#[test]
fn the_system_engine_resolves_to_system_translation_without_an_endpoint() {
    let rig = rig_with_system();
    let r = registry(&rig, Some(Arc::new(FakeApi::default())));
    let plan = r.resolve_call().expect("resolves");
    for d in [&plan.outgoing, &plan.incoming] {
        let mt = &d.mt.primary;
        assert_eq!(mt.engine, MtEngineKind::System);
        assert!(mt.is_system());
        assert_eq!(mt.id.as_str(), SYSTEM_MT_PROVIDER);
        assert_eq!(mt.locality, Locality::ThisComputer);
        assert_eq!(mt.data_leaving(), DataLeaving::Nothing);
        assert!(mt.base_url.is_empty() && mt.secret.is_none());
        assert!(d.mt.fallback.is_none(), "no fallback unless configured");
    }
    let summary = r.egress_summary(&plan);
    let row = summary
        .rows
        .iter()
        .find(|row| row.provider.as_str() == SYSTEM_MT_PROVIDER)
        .expect("a disclosure row");
    assert_eq!(row.data, DataLeaving::Nothing);
    assert!(row.host.is_none());
}

#[test]
fn the_language_model_stays_the_alternative_and_its_selection_is_kept() {
    let mut rig = rig_with_system();
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), "local-llama-server".into());
    // The selection does not matter while the engine is `system`...
    let system = registry(&rig, Some(Arc::new(FakeApi::default())))
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap();
    assert_eq!(system.primary.engine, MtEngineKind::System);
    // ...and is still there when the engine is switched back.
    rig.cfg.call.translation_engine = "llm".to_owned();
    let llm = registry(&rig, None).resolve_mt(Lang::Zh, Lang::En).unwrap();
    assert_eq!(llm.primary.engine, MtEngineKind::Chat);
    assert_eq!(llm.primary.id.as_str(), DEFAULT_MT_PROVIDER);
    assert_eq!(llm.primary.base_url, "http://127.0.0.1:8080/v1");
}

#[test]
fn naming_system_translation_as_the_language_model_primary_is_refused() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .active
        .mt
        .insert("zh>en".into(), SYSTEM_MT_PROVIDER.into());
    let problems = registry(&rig, Some(Arc::new(FakeApi::default())))
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap_err();
    assert!(matches!(
        &problems[0].kind,
        ProblemKind::InvalidSetting { key, .. } if key == "providers.active.mt"
    ));
}

#[test]
fn an_unknown_engine_value_is_a_typed_setting_problem() {
    let mut rig = Rig::new();
    rig.cfg.call.translation_engine = "cloud".to_owned();
    let problems = registry(&rig, None)
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap_err();
    assert_eq!(problems.len(), 1);
    assert_eq!(
        problems[0].kind,
        ProblemKind::InvalidSetting {
            key: "call.translation_engine".into(),
            value: "cloud".into()
        }
    );
}

#[test]
fn a_binary_without_the_bridge_reports_it_and_never_uses_the_language_model() {
    let rig = rig_with_system();
    let problems = registry(&rig, None)
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap_err();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(matches!(
        &problems[0].kind,
        ProblemKind::RuntimeNotBuiltIn { runtime, .. } if runtime == "apple-translation"
    ));
    assert_eq!(problems[0].slot, Slot::Primary);
}

#[test]
fn both_directions_build_their_own_session_and_dropping_the_handles_frees_them() {
    let rig = rig_with_system();
    let api = Arc::new(FakeApi::default());
    let r = registry(&rig, Some(Arc::clone(&api)));
    let plan = r.resolve_call().unwrap();
    let out = r.build_mt(&plan.outgoing.mt.primary).unwrap();
    let inc = r.build_mt(&plan.incoming.mt.primary).unwrap();
    assert_eq!(
        api.opened.load(Ordering::SeqCst),
        2,
        "one session per direction"
    );
    assert_eq!(r.live_engines().mt, 2);
    let d = out.descriptor();
    assert!(!d.supports_context && !d.supports_instructions);
    assert!(d.require_live().is_ok());

    let mut stream = out.translate(request("你好", Lang::Zh, Lang::En)).unwrap();
    let cancel = CancellationToken::new();
    assert_eq!(
        stream.next(&cancel),
        Some(Ok(MtDelta::Text("translated: 你好".into())))
    );
    // A second handle of the same direction shares the engine.
    let again = r.build_mt(&plan.outgoing.mt.primary).unwrap();
    assert_eq!(api.opened.load(Ordering::SeqCst), 2);
    drop((out, again, stream));
    assert_eq!(r.live_engines().mt, 1);
    drop(inc);
    assert_eq!(r.live_engines().mt, 0);
}

#[test]
fn the_problems_the_user_can_act_on_are_typed_build_errors() {
    for kind in [
        SystemFaultKind::PackNotInstalled,
        SystemFaultKind::PairUnsupported,
        SystemFaultKind::OsTooOld,
        SystemFaultKind::Framework,
    ] {
        let rig = rig_with_system();
        let r = registry(
            &rig,
            Some(Arc::new(FakeApi {
                fail: Some(kind),
                ..FakeApi::default()
            })),
        );
        let plan = r.resolve_call().unwrap();
        let err = r.build_mt(&plan.outgoing.mt.primary).err().expect("fails");
        match err {
            BuildError::System { fault, provider } => {
                assert_eq!(fault.kind, kind);
                assert_eq!(provider.as_str(), SYSTEM_MT_PROVIDER);
            }
            other => panic!("expected a system error, got {other:?}"),
        }
        assert_eq!(r.live_engines().mt, 0, "nothing stays loaded");
    }
}

#[test]
fn a_spec_without_the_bridge_is_a_not_built_in_error_at_build_time() {
    let rig = rig_with_system();
    let plan = registry(&rig, Some(Arc::new(FakeApi::default())))
        .resolve_call()
        .unwrap();
    let r = registry(&rig, None);
    let err = r.build_mt(&plan.outgoing.mt.primary).err().expect("fails");
    assert!(matches!(err, BuildError::NotBuiltIn { .. }), "{err:?}");
}

#[test]
fn a_configured_fallback_may_be_the_other_engine_and_is_a_separate_spec() {
    // System primary, language-model fallback.
    let mut rig = rig_with_system();
    rig.cfg
        .providers
        .fallback
        .mt
        .insert("zh>en".into(), DEFAULT_MT_PROVIDER.into());
    let r = registry(&rig, Some(Arc::new(FakeApi::default())));
    let resolved = r.resolve_mt(Lang::Zh, Lang::En).unwrap();
    assert_eq!(resolved.primary.engine, MtEngineKind::System);
    assert_eq!(resolved.fallback.unwrap().engine, MtEngineKind::Chat);
    // Language-model primary, system fallback.
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .fallback
        .mt
        .insert("zh>en".into(), SYSTEM_MT_PROVIDER.into());
    let resolved = registry(&rig, Some(Arc::new(FakeApi::default())))
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap();
    assert_eq!(resolved.primary.engine, MtEngineKind::Chat);
    assert_eq!(resolved.fallback.unwrap().engine, MtEngineKind::System);
    // The system as its own fallback is refused, as for any provider.
    let mut rig = rig_with_system();
    rig.cfg
        .providers
        .fallback
        .mt
        .insert("zh>en".into(), SYSTEM_MT_PROVIDER.into());
    let resolved = registry(&rig, Some(Arc::new(FakeApi::default())))
        .resolve_mt(Lang::Zh, Lang::En)
        .unwrap();
    assert!(resolved.fallback.is_none());
    assert!(
        resolved
            .warnings
            .iter()
            .any(|p| matches!(p.kind, ProblemKind::FallbackIsPrimary { .. }))
    );
}
