use super::*;
use serde_json::{json, Value};

fn shared(section: &str) -> Vec<Value> {
    let path = crate::test_support::fixture_manifest_dir()
        .join("src/js_runtime/parity_test/fixtures/limits-sources.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    fixture[section].as_array().unwrap().clone()
}

fn shared_cases() -> Vec<Value> {
    shared("cases")
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
    let quota: RuntimeProfileLimitsSource =
        serde_json::from_value(json!({"kind":"quota","reader":"fixture-quota"})).unwrap();
    assert_eq!(
        quota,
        RuntimeProfileLimitsSource::Quota {
            reader: "fixture-quota".into()
        }
    );
    assert_eq!(
        serde_json::to_value(&quota).unwrap(),
        json!({"kind":"quota","reader":"fixture-quota"})
    );
}

#[test]
fn only_balance_and_quota_sources_are_read_by_the_plugin() {
    for (source, read) in [
        (json!({"kind":"quota","reader":"fixture-quota"}), true),
        (
            json!({"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}),
            true,
        ),
        (json!({"kind":"account","provider":"fixture-native"}), false),
        (json!({"kind":"unavailable","reason":"Opaque"}), false),
    ] {
        let parsed: RuntimeProfileLimitsSource = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(parsed.plugin_read(), read, "{source}");
    }
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

#[test]
fn manifest_validator_accepts_and_refuses_shared_registry_default_fixtures() {
    let cases = shared("registryDefaultCases");
    assert!(cases.len() >= 5);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        let mut manifest: Value =
            serde_json::from_str(&manifest_with(&json!({"binary":"fixture-agent","args":[]})))
                .unwrap();
        manifest["capabilities"]["agentRuntime"]["registryDefaults"] =
            case["registryDefaults"].clone();
        let accepted =
            crate::js_runtime::PluginManifest::from_json_str(&manifest.to_string()).is_ok();
        assert_eq!(accepted, case["valid"].as_bool().unwrap(), "{name}");
    }
}

fn registry_profile(value: Value) -> RuntimeLaunchProfile {
    serde_json::from_value(value).unwrap()
}

fn defaults() -> std::collections::BTreeMap<String, RuntimeRegistryDefaults> {
    serde_json::from_value(json!({
        "fixture-acp":{"limitsSource":{"kind":"account","provider":"fixture-native"}},
        "fixture-keyed":{"limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}
    }))
    .unwrap()
}

#[test]
fn installed_registry_profiles_resolve_their_declared_default_and_own_sources_win() {
    let mut installed = registry_profile(json!({"binary":"npx","args":["fixture@1.0.0"],
        "ownerConfigured":true,"registryId":"fixture-acp"}));
    resolve_registry_defaults(&mut installed, &defaults());
    assert_eq!(
        installed.limits_source,
        Some(RuntimeProfileLimitsSource::Account {
            provider: "fixture-native".into()
        })
    );
    let mut explicit = registry_profile(json!({"binary":"npx","args":["fixture@1.0.0"],
        "ownerConfigured":true,"registryId":"fixture-acp",
        "limitsSource":{"kind":"unavailable","reason":"Owner reason"}}));
    resolve_registry_defaults(&mut explicit, &defaults());
    assert_eq!(
        explicit.limits_source,
        Some(RuntimeProfileLimitsSource::Unavailable {
            reason: "Owner reason".into()
        })
    );
    for registry_id in [Some("unknown-acp"), None] {
        let mut unknown = registry_profile(
            json!({"binary":"npx","args":["fixture@1.0.0"],"ownerConfigured":true,"registryId":registry_id}),
        );
        resolve_registry_defaults(&mut unknown, &defaults());
        assert_eq!(unknown.limits_source, None, "{registry_id:?}");
    }
    let mut credential_less = registry_profile(json!({"binary":"npx","args":["fixture@1.0.0"],
        "ownerConfigured":true,"registryId":"fixture-keyed"}));
    resolve_registry_defaults(&mut credential_less, &defaults());
    assert_eq!(
        credential_less.limits_source, None,
        "a default must still hold for the profile"
    );
}

#[test]
fn registry_profiles_inherit_a_declared_full_access_mode_and_own_declarations_win() {
    let defaults: std::collections::BTreeMap<String, RuntimeRegistryDefaults> =
        serde_json::from_value(json!({"fixture-acp":{"fullAccessMode":"opaque-approve-all"}}))
            .unwrap();
    assert!(valid_registry_defaults(&defaults));
    let mut installed = registry_profile(json!({"binary":"npx","args":["fixture@1.0.0"],
        "ownerConfigured":true,"registryId":"fixture-acp"}));
    resolve_registry_defaults(&mut installed, &defaults);
    assert_eq!(
        installed.full_access_mode.as_deref(),
        Some("opaque-approve-all")
    );
    let mut explicit = registry_profile(json!({"binary":"npx","args":["fixture@1.0.0"],
        "ownerConfigured":true,"registryId":"fixture-acp","fullAccessMode":"opaque-own"}));
    resolve_registry_defaults(&mut explicit, &defaults);
    assert_eq!(explicit.full_access_mode.as_deref(), Some("opaque-own"));
    for invalid in ["", "line\nbreak"] {
        let defaults: std::collections::BTreeMap<String, RuntimeRegistryDefaults> =
            serde_json::from_value(json!({"fixture-acp":{"fullAccessMode":invalid}})).unwrap();
        assert!(!valid_registry_defaults(&defaults), "{invalid:?}");
    }
}
