use super::*;
use crate::agent::provider::AgentProvider;
use crate::agent::test_provider::MockProvider;

fn profile_runtime() -> codeterm_plugin::agent_runtime::AgentRuntimeCapability {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/provider-acp/plugin.json"
    )))
    .unwrap();
    serde_json::from_value(manifest["capabilities"]["agentRuntime"].clone()).unwrap()
}

#[test]
fn general_eligibility_uses_profile_capability_instead_of_provider_identity() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(MockProvider::new("acp")));
    assert!(validate_general_agent_provider(&registry, "acp").is_ok());
    for empty in [false, true] {
        let mut runtime = profile_runtime();
        if empty {
            runtime.launch_profiles = Some(Default::default());
        }
        let adapter = MockProvider::new("opaque-adapter").with_agent_runtime(runtime.clone());
        registry.register_provider(Box::new(adapter));
        assert!(validate_general_agent_provider(&registry, "opaque-adapter")
            .unwrap_err()
            .starts_with("general_agent_provider_ineligible:"));
        runtime.launch_profiles = None;
        let harness = MockProvider::new("opaque-harness").with_agent_runtime(runtime);
        assert!(eligible(&harness.describe()));
        let mut plain_chat = MockProvider::new("plain-chat").describe();
        plain_chat.transport = ProviderTransport::ChatBackend;
        assert!(!eligible(&plain_chat));
    }
    assert!(validate_general_agent_provider(&registry, "missing")
        .unwrap_err()
        .starts_with("Unknown provider:"));
}

#[test]
fn rejected_general_choices_leave_configuration_and_source_session_untouched() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    super::super::identity::apply_general_agent_kind(tab);
    tab.agent_type = Some("source".into());
    tab.agent_session_id = Some("source-session".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::new("opaque-adapter").with_agent_runtime(profile_runtime()),
    ));
    let config = fixture.service.ext::<crate::config::ConfigState>();
    let service = std::sync::Arc::new(fixture.service.clone());
    let before = serde_json::to_value(fixture.service.configuration()).unwrap();
    assert!(
        super::super::complete_first_run_core(&service, config, "opaque-adapter")
            .unwrap_err()
            .starts_with("general_agent_provider_ineligible:")
    );
    assert!(fixture
        .service
        .switch_provider_in_place(&tab_id, "opaque-adapter", None, None, false)
        .err()
        .expect("switch to an ineligible provider must refuse")
        .starts_with("general_agent_provider_ineligible:"));
    assert_eq!(
        serde_json::to_value(fixture.service.configuration()).unwrap(),
        before
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("source"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("source-session"));
}
