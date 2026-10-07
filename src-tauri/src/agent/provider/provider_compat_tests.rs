use codeterm_plugin::provider::AgentProvider;
use codeterm_plugin::provider_compat::{CompatibilityReason, CompatibilityVerdict};

#[test]
fn provider_compat_opencode_direct_sqlite_is_declared_and_fails_closed() {
    let manifest =
        codeterm_plugin::js_runtime::PluginManifest::from_json_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-opencode/plugin.json"
        )))
        .unwrap();
    let policy = manifest.cli_compatibility.unwrap();
    assert!(policy.direct_sqlite);
    for version in [None, Some("bad"), Some("1.18.33")] {
        assert_eq!(
            policy.evaluate(version).verdict,
            CompatibilityVerdict::Unknown
        );
    }
    for version in ["2.0.0", "2.0.0-beta.1", "2.0.18"] {
        let evaluation = policy.evaluate(Some(version));
        assert_eq!(evaluation.verdict, CompatibilityVerdict::Breaking);
        assert_eq!(
            evaluation.reasons,
            vec![
                CompatibilityReason::SqliteSchemaConversion,
                CompatibilityReason::EventRouteChanged,
                CompatibilityReason::ServerPasswordRequired
            ]
        );
    }
}

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
