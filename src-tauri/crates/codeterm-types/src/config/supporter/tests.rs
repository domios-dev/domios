use super::{SupporterConfig, DEFAULT_SUPPORTER_CALLS_PER_HOUR};
use crate::config::schema::AppConfig;

#[test]
fn supporter_config_round_trip() {
    let config: AppConfig = toml::from_str(
        r#"
[supporter]
adapter = "fixture"
deadline_ms = 8000
calls_per_hour = 120
[supporter.connections.fixture]
model = "fixture-model"
endpoint = "http://localhost:1234"
[supporter.memory_rerank]
candidates = 24
[supporter.memory_rerank.sources]
recall = 12
graph = 2
"#,
    )
    .unwrap();
    let decoded: AppConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(decoded.supporter, config.supporter);
    assert_eq!(decoded.supporter.calls_per_hour, Some(120));
    assert_eq!(decoded.supporter.memory_rerank.candidates, 24);
}

#[test]
fn supporter_missing_table_keeps_defaults() {
    let config: AppConfig = toml::from_str("[memory]\nauto_extract = false\n").unwrap();
    assert_eq!(config.supporter, SupporterConfig::default());
    assert_eq!(
        config.supporter.calls_per_hour,
        DEFAULT_SUPPORTER_CALLS_PER_HOUR
    );
}

#[test]
fn supporter_removed_keys_are_ignored_without_round_trip() {
    let text = format!(
        r#"
[supporter.memory_rerank]
enabled = true
[supporter.tab_state]
enabled = true
{strong} = true
{weak} = true
"#,
        strong = ["strong", "actions"].join("_"),
        weak = ["weak", "hints"].join("_")
    );
    let config: AppConfig = toml::from_str(&text).unwrap();
    assert_eq!(config.supporter, SupporterConfig::default());
    let value = serde_json::to_value(config.supporter).unwrap();
    assert!(value["memory_rerank"].get("enabled").is_none());
    assert!(value.get("tab_state").is_none());
}

#[test]
fn supporter_rerank_request_mode_is_selectable_and_stays_per_candidate_until_ab() {
    use super::RerankRequestMode;
    assert_eq!(
        SupporterConfig::default().memory_rerank.request_mode,
        RerankRequestMode::PerCandidate
    );
    let config: AppConfig =
        toml::from_str("[supporter.memory_rerank]\nrequest_mode = 'shared_state'\n").unwrap();
    assert_eq!(
        config.supporter.memory_rerank.request_mode,
        RerankRequestMode::SharedState
    );
    assert!(
        toml::from_str::<AppConfig>("[supporter.memory_rerank]\nrequest_mode = 'invalid'\n")
            .is_err()
    );
}

#[test]
fn supporter_default_has_no_cap_and_zero_is_unlimited() {
    assert_eq!(SupporterConfig::default().calls_per_hour, None);
    for text in ["", "[supporter]\ncalls_per_hour = 0\n"] {
        let config: AppConfig = toml::from_str(text).unwrap();
        assert_eq!(config.supporter.calls_per_hour.unwrap_or(0), 0);
        let serialized = toml::to_string(&config).unwrap();
        let decoded: AppConfig = toml::from_str(&serialized).unwrap();
        assert_eq!(decoded.supporter, config.supporter);
    }
}

#[test]
fn supporter_connections_optional_active_and_removed_shared_keys() {
    let config: SupporterConfig = toml::from_str("model = 'old'\nendpoint = 'old'\n").unwrap();
    assert_eq!(config.adapter, None);
    let value = serde_json::to_value(&config).unwrap();
    assert!(value.get("model").is_none());
    assert!(value.get("endpoint").is_none());
    assert!(value.get("adapter").is_none());
    let config: SupporterConfig = toml::from_str("adapter = 'fixture'\n[connections.fixture]\nmodel = 'one'\n[connections.second]\nmodel = 'two'\n").unwrap();
    let decoded: SupporterConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
    assert_eq!(config, decoded);
    assert_eq!(decoded.connection("fixture").model.as_deref(), Some("one"));
    assert_eq!(decoded.connection("second").model.as_deref(), Some("two"));
}
