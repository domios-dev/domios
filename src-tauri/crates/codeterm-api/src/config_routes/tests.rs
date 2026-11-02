use super::{api_listener_value, api_query_includes_listener, redact_rest_config};

#[test]
fn rest_config_never_serializes_api_token() {
    let config = serde_json::json!({
        "api": { "host": "0.0.0.0", "token": "permanent-secret" },
        "mesh": { "enabled": true }
    });
    let redacted = redact_rest_config(config);
    assert_eq!(redacted["api"]["token"], serde_json::Value::Null);
    assert_eq!(redacted["api"]["host"], "0.0.0.0");
    assert_eq!(redacted["mesh"]["enabled"], true);
}

#[test]
fn api_listener_readback_preserves_active_policy_until_restart() {
    let mut cfg = codeterm_service_core::config::schema::AppConfig::default();
    cfg.api.enabled = true;
    cfg.api.host = "localhost".into();
    cfg.api.port = 7686;
    cfg.api.cors = true;
    cfg.api.cors_allow_origins = vec!["https://configured.example".into()];
    let active = codeterm_service_core::startup::RestApiRuntimePolicy {
        enabled: true,
        cors: false,
        cors_allow_origins: Vec::new(),
    };

    let disclosure = api_listener_value(Some("127.0.0.1:7686".parse().unwrap()), active, &cfg);
    assert_eq!(disclosure["active"]["cors"], false);
    assert_eq!(
        disclosure["active"]["cors_allow_origins"],
        serde_json::json!([])
    );
    assert_eq!(disclosure["configured"]["cors"], true);
    assert_eq!(
        disclosure["configured"]["cors_allow_origins"],
        serde_json::json!(["https://configured.example"])
    );
    assert_eq!(disclosure["restart_required"], true);
}

#[test]
fn exact_api_runtime_queries_include_listener_truth() {
    for key in ["api", "api.enabled", "api.cors", "api.cors_allow_origins"] {
        assert!(
            api_query_includes_listener(key),
            "missing listener truth for {key}"
        );
    }
    assert!(!api_query_includes_listener("theme.name"));
}
