use super::DecisionRerankConfig;
use crate::config::schema::AppConfig;

#[test]
fn decision_rerank_defaults_off() {
    let cfg = DecisionRerankConfig::default();
    assert!(!cfg.enabled);
    assert_eq!(cfg.candidates, 10);
    assert_eq!(cfg.max_concurrency, 8);
    assert_eq!(cfg.deadline_ms, 3000);
    assert_eq!(cfg.low_confidence_threshold, 0.15);
    assert_eq!(cfg.max_body_chars, 8000);
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

    let partial: AppConfig =
        toml::from_str("[memory.decision_rerank]\nenabled = true\ncandidates = 20\n").unwrap();
    assert_eq!(
        partial.memory.decision_rerank,
        DecisionRerankConfig {
            enabled: true,
            candidates: 20,
            ..DecisionRerankConfig::default()
        }
    );
}
