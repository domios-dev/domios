use super::*;
use serde_json::{json, Value};

fn shared_cases() -> Vec<Value> {
    let path = crate::test_support::fixture_manifest_dir()
        .join("src/js_runtime/parity_test/fixtures/limits-sources.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fixture["cases"].as_array().unwrap().clone()
}

fn manifest_with(profile: &Value) -> String {
    json!({
        "id":"fixture", "version":"1.0.0", "displayName":"Fixture",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{
            "runtime":"stdio-jsonrpc","command":{"binary":"fixture-agent","args":[]},"requestMethods":{"initialize":["initialize"]},
            "notificationMethods":[],"launchProfiles":{"selected":profile}
        }},"permissions":{"subprocess":{"allow":["fixture-agent"]},"secrets":true}
    })
    .to_string()
}

#[test]
fn manifest_validator_accepts_and_refuses_shared_limits_source_fixtures() {
    let cases = shared_cases();
    assert!(cases.len() >= 10);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let accepted =
            crate::js_runtime::PluginManifest::from_json_str(&manifest_with(&case["profile"]))
                .is_ok();
        assert_eq!(accepted, case["valid"].as_bool().unwrap(), "{name}");
    }
}

#[test]
fn declared_sources_round_trip_with_their_wire_names() {
    let profile: RuntimeLaunchProfile = serde_json::from_value(
        json!({"binary":"fixture-agent","args":[],"secretEnv":{"KEY":"domios:provider:fixture:key"},
        "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}),
    )
    .unwrap();
    assert_eq!(
        profile.limits_source,
        Some(RuntimeProfileLimitsSource::Balance {
            reader: "fixture-balance".into(),
            credential_env: "KEY".into()
        })
    );
    assert_eq!(
        serde_json::to_value(&profile).unwrap()["limitsSource"],
        json!({"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"})
    );
}

#[test]
fn balance_account_keys_name_only_valid_profiles() {
    assert_eq!(balance_account_key("deep-1"), "profile:deep-1");
    assert_eq!(
        profile_from_balance_account_key("profile:deep-1"),
        Some("deep-1")
    );
    for account in [
        "deep-1",
        "profile:",
        "profile:../escape",
        "profile:two words",
        "profile:NUL",
    ] {
        assert_eq!(profile_from_balance_account_key(account), None, "{account}");
    }
}
