use codeterm_plugin::provider::AgentProvider;
use codeterm_plugin::provider_compat::{CompatibilityReason, CompatibilityVerdict};

#[test]
fn provider_compat_native_observation_uses_declared_policy() {
    let provider = codeterm_plugin::agent_claude::provider::ClaudeProvider::new();
    let policy = provider.cli_compatibility().unwrap();
    assert_eq!(
        policy.evaluate(Some("2.1.292")).verdict,
        CompatibilityVerdict::Supported
    );
    assert_eq!(
        policy.evaluate(None).reasons,
        vec![CompatibilityReason::VersionUnknown]
    );
    assert_eq!(
        policy.evaluate(Some("3.0.0")).verdict,
        CompatibilityVerdict::Unknown
    );
}
