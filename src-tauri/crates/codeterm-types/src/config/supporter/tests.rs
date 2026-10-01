use super::{MemoryRerankConfig, SupporterConfig, SupporterSources, SupporterTabStateConfig};
use crate::config::schema::AppConfig;

#[test]
fn supporter_config_round_trip() {
    let config: AppConfig = toml::from_str(
        r#"
[supporter]
adapter = "fixture"
model = "fixture-model"
deadline_ms = 8000

[supporter.memory_rerank]
enabled = true
candidates = 24

[supporter.memory_rerank.sources]
recall = 12
graph = 2

[supporter.tab_state]
enabled = true
"#,
    )
    .unwrap();
    let encoded = toml::to_string(&config).unwrap();
    let decoded: AppConfig = toml::from_str(&encoded).unwrap();

    assert_eq!(decoded.supporter, config.supporter);
    assert_eq!(
        decoded.supporter,
        SupporterConfig {
            adapter: "fixture".into(),
            model: Some("fixture-model".into()),
            deadline_ms: 8000,
            memory_rerank: MemoryRerankConfig {
                enabled: true,
                candidates: 24,
                sources: SupporterSources {
                    recall: 12,
                    graph: 2,
                    ..SupporterSources::default()
                },
                ..MemoryRerankConfig::default()
            },
            tab_state: SupporterTabStateConfig { enabled: true },
            ..SupporterConfig::default()
        }
    );
}

#[test]
fn supporter_missing_table_defaults_all_off() {
    let config: AppConfig = toml::from_str("[memory]\nauto_extract = false\n").unwrap();

    assert!(!config.supporter.memory_rerank.enabled);
    assert!(!config.supporter.tab_state.enabled);
    assert_eq!(config.supporter, SupporterConfig::default());
}
