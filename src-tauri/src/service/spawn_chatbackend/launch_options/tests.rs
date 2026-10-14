use super::*;

#[test]
fn explicit_host_false_cannot_be_broadened_by_an_available_authorization() {
    assert!(!permission_request(Some(false), true));
    assert!(!permission_request(Some(false), false));
}

#[test]
fn permission_bypass_requires_authorization_and_defaults_only_when_unspecified() {
    assert!(permission_request(Some(true), true));
    assert!(!permission_request(Some(true), false));
    assert!(permission_request(None, true));
    assert!(!permission_request(None, false));
}

fn prepared_options() -> crate::multiplexer::types::RestorableRuntimeLaunchOptions {
    crate::multiplexer::types::RestorableRuntimeLaunchOptions {
        provider_id: "fixture-runtime".into(),
        binding_nonce: "fixture-nonce".into(),
        mode: "auto-accept-edits".into(),
        additional_directories: vec!["opaque-host-root".into()],
        network_access: Some(false),
    }
}

#[test]
fn prepared_options_override_caller_grants_and_preserve_launch_context() {
    let options = prepared_options();
    let config = apply_prepared(
        serde_json::json!({
            "mode": "full-access", "additionalDirectories": ["opaque-caller-root"],
            "networkAccess": true, "model": "opaque-model", "effort": "opaque-effort",
            "systemPrompt": "opaque-prompt"
        }),
        &options.provider_id,
        Some(&options.binding_nonce),
        Some(&options),
    )
    .unwrap();
    assert_eq!(config["mode"], options.mode);
    assert_eq!(
        config["additionalDirectories"],
        serde_json::json!(options.additional_directories)
    );
    assert_eq!(config["networkAccess"], false);
    assert_eq!(config["model"], "opaque-model");
    assert_eq!(config["effort"], "opaque-effort");
    assert_eq!(config["systemPrompt"], "opaque-prompt");
    let mut no_network = options.clone();
    no_network.network_access = None;
    let config = apply_prepared(
        config,
        &options.provider_id,
        Some(&options.binding_nonce),
        Some(&no_network),
    )
    .unwrap();
    assert!(config.get("networkAccess").is_none());
}

#[test]
fn prepared_options_refuse_foreign_provider_or_nonce_before_launch() {
    let options = prepared_options();
    for (provider, nonce) in [
        ("foreign-runtime", Some(options.binding_nonce.as_str())),
        (options.provider_id.as_str(), Some("foreign-nonce")),
        (options.provider_id.as_str(), None),
    ] {
        assert!(apply_prepared(serde_json::json!({}), provider, nonce, Some(&options)).is_err());
    }
}
