use super::*;
use serde_json::json;

fn profile(value: serde_json::Value) -> RuntimeLaunchProfile {
    serde_json::from_value(value).unwrap()
}

#[test]
fn only_the_declared_balance_credential_in_the_plugin_namespace_resolves() {
    let required = profile(json!({"binary":"fixture-agent","args":[],
        "secretEnv":{"KEY":"domios:provider:fixture:key","OTHER":"domios:provider:fixture:other"},
        "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}));
    assert_eq!(
        balance_secret_name("fixture", &required, "KEY").as_deref(),
        Some("domios:provider:fixture:key")
    );
    assert_eq!(balance_secret_name("fixture", &required, "OTHER"), None);
    assert_eq!(balance_secret_name("foreign", &required, "KEY"), None);
    let optional = profile(json!({"binary":"fixture-agent","args":[],
        "authEnv":{"KEY":{"secret":"domios:provider:fixture:key","methods":["api-key"]}},
        "limitsSource":{"kind":"balance","reader":"fixture-balance","credentialEnv":"KEY"}}));
    assert_eq!(
        balance_secret_name("fixture", &optional, "KEY").as_deref(),
        Some("domios:provider:fixture:key")
    );
    let undeclared = profile(json!({"binary":"fixture-agent","args":[],
        "secretEnv":{"KEY":"domios:provider:fixture:key"},
        "limitsSource":{"kind":"unavailable","reason":"Fixture quota has no endpoint"}}));
    assert_eq!(balance_secret_name("fixture", &undeclared, "KEY"), None);
}

#[test]
fn balance_readings_are_keyed_by_profile_and_never_retain_raw_payloads() {
    let raw = json!({"provider":"forged","account_id":"forged","captured_at_ms":7,"billing_mode":"api","limits_reported":true,
        "session_pct":null,"session_resets_at_ms":null,"weekly_pct":null,"weekly_resets_at_ms":null,"raw_text":"opaque-body",
        "usage_details":{"balance":{"amount":"12.50","currency":"USD","available":true}}})
    .to_string();
    let snapshot = decode("fixture", "deep-1", &raw).unwrap();
    assert_eq!(snapshot.provider, "fixture");
    assert_eq!(snapshot.account_id.as_deref(), Some("profile:deep-1"));
    assert!(snapshot.raw_text.is_empty());
    assert_eq!(
        snapshot.usage_details.unwrap()["balance"]["amount"],
        "12.50"
    );
    assert!(decode("fixture", "deep-1", "not json").is_err());
    assert!(decode("fixture", "deep-1", &"x".repeat(64 * 1024 + 1)).is_err());
}
