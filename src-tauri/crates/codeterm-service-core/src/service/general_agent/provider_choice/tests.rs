use super::test_support::profile_runtime;
use super::*;
use crate::agent::provider::AgentProvider;
use crate::agent::test_provider::MockProvider;

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
