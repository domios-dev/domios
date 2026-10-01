use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupporterConfig {
    pub adapter: String,
    pub endpoint: Option<String>,
    pub model: Option<String>,
    pub deadline_ms: u64,
    pub memory_rerank: MemoryRerankConfig,
    pub tab_state: SupporterTabStateConfig,
}

impl Default for SupporterConfig {
    fn default() -> Self {
        Self {
            adapter: "typesafe-jev".into(),
            endpoint: None,
            model: Some("jev-latest".into()),
            deadline_ms: 3000,
            memory_rerank: MemoryRerankConfig::default(),
            tab_state: SupporterTabStateConfig::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryRerankConfig {
    pub enabled: bool,
    pub docs_hybrid: bool,
    pub sources: SupporterSources,
    pub candidates: usize,
    pub max_concurrency: usize,
    pub low_confidence_threshold: f64,
    pub max_body_chars: usize,
}

impl Default for MemoryRerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            docs_hybrid: false,
            sources: SupporterSources::default(),
            candidates: 50,
            max_concurrency: 16,
            low_confidence_threshold: 0.15,
            max_body_chars: 2000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupporterSources {
    pub recall: usize,
    pub graph: usize,
    pub docs: usize,
    pub sessions: usize,
}

impl Default for SupporterSources {
    fn default() -> Self {
        Self {
            recall: 30,
            graph: 3,
            docs: 3,
            sessions: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupporterTabStateConfig {
    pub enabled: bool,
    pub strong_actions: bool,
    pub weak_hints: bool,
}

impl Default for SupporterTabStateConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            strong_actions: true,
            weak_hints: true,
        }
    }
}

#[cfg(test)]
mod tests;
