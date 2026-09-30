use super::{DecisionRerankConfig, DecisionSources};
use crate::config::schema::AppConfig;

#[test]
fn decision_rerank_defaults_off() {
    let cfg = DecisionRerankConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(
        cfg.sources,
        DecisionSources {
            recall: 30,
            graph: 3,
            docs: 0,
        }
    );
    assert_eq!(cfg.max_concurrency, 16);
    assert_eq!(cfg.deadline_ms, 3000);
    assert_eq!(cfg.low_confidence_threshold, 0.15);
    assert_eq!(cfg.max_body_chars, 2000);
    assert!(cfg.endpoint.is_none());
    assert!(cfg.model.is_none());
    assert!(!AppConfig::default().memory.decision_rerank.enabled);
}

#[test]
fn decision_rerank_missing_section_uses_defaults() {
    let config: AppConfig = toml::from_str("[memory]\nauto_extract = false\n").unwrap();
    assert!(!config.memory.auto_extract);
    assert_eq!(
        config.memory.decision_rerank,
        DecisionRerankConfig::default()
    );

    let partial: AppConfig = toml::from_str(
        "[memory.decision_rerank]\nenabled = true\n\n[memory.decision_rerank.sources]\ngraph = 3\ndocs = 4\n",
    )
    .unwrap();
    assert_eq!(
        partial.memory.decision_rerank,
        DecisionRerankConfig {
            enabled: true,
            sources: DecisionSources {
                recall: 30,
                graph: 3,
                docs: 4,
            },
            ..DecisionRerankConfig::default()
        }
    );
}
