use super::*;

#[test]
fn config_bounds_table_enforced() {
    let mut defaults = serde_json::to_value(AppConfig::default()).unwrap();
    defaults["notifications"]["channels"]["test"] = serde_json::json!({"per_scope_limit": 12});
    for bound in CONFIG_BOUNDS {
        let values = values_at_path(&defaults, bound.path);
        assert!(!values.is_empty(), "bound path is missing: {}", bound.path);
        for value in values {
            validate_bound(bound, value)
                .unwrap_or_else(|error| panic!("default config violates {}: {error}", bound.path));
        }
        match bound.kind {
            ValueKind::Integer | ValueKind::Number => {
                assert!(bound.min.is_some());
                assert!(bound.max.is_some());
            }
            ValueKind::Enum(values) => assert!(!values.is_empty()),
        }
    }

    let mut config = AppConfig::default();
    config.font.size = 0;
    let error = validate_app_config(&config).unwrap_err();
    assert!(error.contains("font.size"));
    assert!(error.contains("8..=72"));

    config = AppConfig::default();
    config.font.size = 99999;
    let error = validate_app_config(&config).unwrap_err();
    assert!(error.contains("font.size"));
    assert!(error.contains("8..=72"));

    config = AppConfig::default();
    config.integrations.text_hooks.fallback_on_error = "ignore".into();
    let error = validate_app_config(&config).unwrap_err();
    assert!(error.contains("integrations.text_hooks.fallback_on_error"));
    assert!(error.contains("forward, drop"));

    config = AppConfig::default();
    config.agent.poll_interval_ms = 0;
    let error = validate_app_config(&config).unwrap_err();
    assert!(error.contains("agent.poll_interval_ms"));
    assert!(error.contains("100..=60000"));

    config = AppConfig::default();
    config.memory.workers_per_provider = 0;
    let error = validate_app_config(&config).unwrap_err();
    assert!(error.contains("memory.workers_per_provider"));
    assert!(error.contains("1..=16"));

    let error = validate_config_text("[font]\nsize = 0\n").unwrap_err();
    assert!(error.contains("font.size"));
    assert!(error.contains("8..=72"));

    let mut value = defaults;
    value["font"]["family"] = serde_json::Value::String("x".repeat(MAX_CONFIG_STRING_BYTES + 1));
    assert!(validate_config_value(&value)
        .unwrap_err()
        .contains("65536 UTF-8 bytes"));

    let mut value = serde_json::to_value(AppConfig::default()).unwrap();
    value["font"]["family"] = serde_json::Value::String("raw\u{1b}escape".into());
    assert!(validate_config_value(&value)
        .unwrap_err()
        .contains("control character"));
}
