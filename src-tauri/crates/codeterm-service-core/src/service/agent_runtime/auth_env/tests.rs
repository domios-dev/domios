use super::*;
use serde_json::json;

fn profile() -> RuntimeLaunchProfile {
    serde_json::from_value(json!({"binary":"opaque-agent","args":["--opaque"],
    "authEnv":{
        "OPAQUE_KEY":{"secret":"domios:provider:opaque:key","methods":["opaque-key"]},
        "OPAQUE_ALT":{"secret":"domios:provider:opaque:alt","methods":["opaque-alt","opaque-alt-2"]}
    }}))
    .unwrap()
}

#[test]
fn stored_keys_reach_their_variables_and_absent_or_invalid_ones_stay_unset() {
    let env = compose_auth_environment(&profile(), |secret| {
        (secret == "domios:provider:opaque:key").then(|| "opaque-value".to_string())
    });
    assert_eq!(
        env,
        BTreeMap::from([("OPAQUE_KEY".to_string(), "opaque-value".to_string())])
    );
    for invalid in ["", "  ", "opaque\0value"] {
        assert!(compose_auth_environment(&profile(), |_| Some(invalid.into())).is_empty());
    }
    assert!(compose_auth_environment(&profile(), |_| None).is_empty());
}

#[test]
fn a_declared_method_selects_the_secret_that_receives_its_key() {
    let profile = profile();
    assert_eq!(
        key_secret_for_method(&profile, "opaque-key"),
        Some("domios:provider:opaque:key")
    );
    assert_eq!(
        key_secret_for_method(&profile, "opaque-alt-2"),
        Some("domios:provider:opaque:alt")
    );
    assert_eq!(key_secret_for_method(&profile, "opaque-browser"), None);
    let mut methods = api_key_methods(&profile);
    methods.sort();
    assert_eq!(methods, ["opaque-alt", "opaque-alt-2", "opaque-key"]);
}

#[test]
fn an_undirected_key_needs_exactly_one_key_method() {
    assert!(default_api_key_method(&[]).is_err());
    assert_eq!(
        default_api_key_method(&["opaque-key".to_string()]).unwrap(),
        "opaque-key"
    );
    let choice =
        default_api_key_method(&["opaque-a".to_string(), "opaque-b".to_string()]).unwrap_err();
    assert!(choice.contains("--method") && choice.contains("opaque-a, opaque-b"));
}

#[test]
fn profile_secrets_stay_inside_their_provider_namespace() {
    assert!(in_namespace("opaque", "domios:provider:opaque:key"));
    assert!(!in_namespace("opaque", "domios:provider:other:key"));
    assert!(!in_namespace("opaque", "domios:provider:opaque-other:key"));
}
