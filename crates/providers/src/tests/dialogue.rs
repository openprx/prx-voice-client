//! The assistant's dialogue role: defaults, entries, the OpenPRX placeholder, parameters,
//! construction and the offline policy.

use prxvc_ports::StreamingKind;

use super::fixtures::*;
use crate::keys::RoleKey;
use crate::problem::{Problem, ProblemKind, Slot};
use crate::resolve::{
    DEFAULT_DIALOGUE_PROVIDER, DEFAULT_MT_PROVIDER, DialogueKind, OPENPRX_PROVIDER,
};
use crate::spec::Locality;

fn kinds(problems: &[Problem]) -> Vec<&ProblemKind> {
    problems.iter().map(|p| &p.kind).collect()
}

#[test]
fn a_fresh_config_resolves_to_the_local_server_with_thinking_off() {
    let rig = Rig::new();
    let spec = rig
        .registry()
        .resolve_dialogue()
        .expect("the default resolves")
        .primary;
    assert_eq!(spec.id.as_str(), DEFAULT_DIALOGUE_PROVIDER);
    assert_eq!(DEFAULT_DIALOGUE_PROVIDER, DEFAULT_MT_PROVIDER);
    assert_eq!(spec.base_url, "http://127.0.0.1:8080/v1");
    assert_eq!(spec.model, "gemma-4-e4b-it-qat-q4-0");
    assert_eq!(spec.locality, Locality::ThisComputer);
    assert_eq!(
        spec.params["chat_template_kwargs"]["enable_thinking"],
        false
    );
    assert!(
        !spec.params.contains_key("prompt_template"),
        "a translation prompt is not sent to the assistant"
    );
    assert_eq!(
        spec.params["temperature"], 0.7,
        "a conversation is not translated at zero"
    );
    assert_eq!(spec.timeouts.first_token.as_secs(), 15);
    assert_eq!(spec.history.max_turns, 12);
    assert_eq!(spec.history.max_tokens, 3_000);
}

#[test]
fn the_history_bounds_come_from_the_assistant_section() {
    let mut rig = Rig::new();
    rig.cfg.assistant.history_max_turns = 4;
    rig.cfg.assistant.history_max_tokens = 800;
    let spec = rig.registry().resolve_dialogue().unwrap().primary;
    assert_eq!((spec.history.max_turns, spec.history.max_tokens), (4, 800));
}

#[test]
fn an_entry_chosen_for_the_dialogue_keeps_its_own_sampling_and_timeouts() {
    let mut rig = Rig::new();
    rig.openai_entry("cloud-chat", "https://api.openai.com/v1");
    rig.cfg.providers.entries[0] = toml_entry(
        "id = \"cloud-chat\"\nprotocol = \"openai-chat\"\nbase_url = \"https://api.openai.com/v1\"\n\
         model = \"gpt-4.1-mini\"\ncredential = \"env:OPENAI_KEY\"\n\
         [providers.entries.params]\ntemperature = 0.2\nfirst_token_timeout_ms = 4000\n",
    );
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), "cloud-chat".into());
    let spec = rig.registry().resolve_dialogue().unwrap().primary;
    assert_eq!(spec.id.as_str(), "cloud-chat");
    assert_eq!(spec.params["temperature"], 0.2);
    assert_eq!(spec.timeouts.first_token.as_secs(), 4);
    assert_eq!(
        spec.timeouts.idle.as_secs(),
        8,
        "the idle default of a dialogue"
    );
    assert_eq!(spec.locality, Locality::Remote);
    assert!(spec.secret.is_some(), "the credential reference is read");
    assert_eq!(spec.data_leaving(), crate::spec::DataLeaving::Text);
}

#[test]
fn an_entry_without_its_credential_is_a_typed_problem() {
    let mut rig = Rig::new();
    rig.openai_entry("cloud-chat", "https://api.openai.com/v1");
    rig.env.lock().clear();
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), "cloud-chat".into());
    let problems = rig.registry().resolve_dialogue().unwrap_err();
    assert!(
        matches!(
            kinds(&problems)[..],
            [ProblemKind::CredentialMissing { .. }]
        ),
        "{problems:?}"
    );
    assert_eq!(problems[0].role, Some(RoleKey::Dialogue));
}

#[test]
fn the_openprx_placeholder_is_not_available_yet_and_never_another_backend() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), OPENPRX_PROVIDER.into());
    let problems = rig.registry().resolve_dialogue().unwrap_err();
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].slot, Slot::Primary);
    assert!(matches!(
        &problems[0].kind,
        ProblemKind::NotAvailableYet { id, name }
            if id.as_str() == OPENPRX_PROVIDER && name.contains("OpenPRX")
    ));
    assert!(problems[0].to_string().contains("not available yet"));
}

#[test]
fn an_unknown_backend_and_a_non_chat_entry_are_typed_problems() {
    let mut rig = Rig::new();
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), "no-such".into());
    let problems = rig.registry().resolve_dialogue().unwrap_err();
    assert!(matches!(
        kinds(&problems)[..],
        [ProblemKind::UnknownProvider { .. }]
    ));

    let mut rig = Rig::new();
    rig.cfg.providers.entries.push(toml_entry(
        "id = \"asr-cloud\"\nprotocol = \"deepgram-listen\"\nbase_url = \"wss://example.invalid\"\nmodel = \"x\"\n",
    ));
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), "asr-cloud".into());
    let problems = rig.registry().resolve_dialogue().unwrap_err();
    assert!(matches!(
        kinds(&problems)[..],
        [ProblemKind::KindNotValidForRole { .. }]
    ));
}

#[test]
fn a_fallback_for_the_dialogue_is_resolved_like_the_primary() {
    let mut rig = Rig::new();
    rig.openai_entry("cloud-chat", "https://api.openai.com/v1");
    rig.cfg
        .providers
        .fallback
        .dialogue
        .insert(String::new(), "cloud-chat".into());
    let resolved = rig.registry().resolve_dialogue().unwrap();
    assert_eq!(resolved.fallback.unwrap().id.as_str(), "cloud-chat");
}

#[test]
fn the_settings_list_the_local_server_the_chat_entries_and_the_openprx_placeholder() {
    let mut rig = Rig::new();
    rig.openai_entry("cloud-chat", "https://api.openai.com/v1");
    rig.cfg.providers.entries.push(toml_entry(
        "id = \"asr-cloud\"\nprotocol = \"deepgram-listen\"\nbase_url = \"wss://example.invalid\"\nmodel = \"x\"\n",
    ));
    let choices = rig.registry().dialogue_choices();
    let listed: Vec<_> = choices.iter().map(|c| (c.id.as_str(), c.kind)).collect();
    assert_eq!(
        listed,
        [
            (DEFAULT_DIALOGUE_PROVIDER, DialogueKind::LocalServer),
            ("cloud-chat", DialogueKind::Entry),
            (OPENPRX_PROVIDER, DialogueKind::OpenPrx),
        ]
    );
}

#[test]
fn building_shares_one_client_between_handles_and_frees_it_with_the_last() {
    let rig = Rig::new();
    let registry = rig.registry();
    let spec = registry.resolve_dialogue().unwrap().primary;
    let a = registry.build_dialogue(&spec).unwrap();
    let b = registry.build_dialogue(&spec).unwrap();
    assert_eq!(registry.live_engines().dialogue, 1);
    assert_eq!(a.descriptor().model, "gemma-4-e4b-it-qat-q4-0");
    assert_eq!(a.descriptor().streaming, StreamingKind::Native);
    drop((a, b));
    assert_eq!(registry.live_engines().dialogue, 0);
}

#[test]
fn the_offline_policy_blocks_a_remote_dialogue_endpoint_before_anything_is_created() {
    let mut rig = Rig::new();
    rig.openai_entry("cloud-chat", "https://api.openai.com/v1");
    rig.cfg
        .providers
        .active
        .dialogue
        .insert(String::new(), "cloud-chat".into());
    let spec = rig.registry().resolve_dialogue().unwrap().primary;
    rig.cfg.network_policy.offline = true;
    let registry = rig.registry();
    let Err(err) = registry.build_dialogue(&spec) else {
        panic!("offline mode must block it")
    };
    assert!(matches!(err, crate::problem::BuildError::Blocked { .. }));
    let problems = registry.resolve_dialogue().unwrap_err();
    assert!(matches!(
        kinds(&problems)[..],
        [ProblemKind::EndpointBlocked { .. }]
    ));
    assert_eq!(registry.live_engines().dialogue, 0);
}
