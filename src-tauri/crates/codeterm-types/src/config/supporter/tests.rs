use super::{SupporterConfig, DEFAULT_SUPPORTER_CALLS_PER_HOUR};
use crate::config::schema::AppConfig;

#[test]
fn supporter_config_round_trip() {
    let config: AppConfig = toml::from_str(
        r#"
[supporter]
adapter = "fixture"
model = "fixture-model"
deadline_ms = 8000
calls_per_hour = 120
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
    assert_eq!(decoded.supporter.calls_per_hour, 120);
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
