use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const DEFAULT_SUPPORTER_CALLS_PER_HOUR: Option<usize> = None;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupporterConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    pub connections: BTreeMap<String, SupporterConnection>,
    pub deadline_ms: u64,
    pub memory_rerank: MemoryRerankConfig,
    pub calls_per_hour: Option<usize>,
}

impl Default for SupporterConfig {
    fn default() -> Self {
        Self {
            adapter: None,
            connections: BTreeMap::new(),
            deadline_ms: 3000,
            memory_rerank: MemoryRerankConfig::default(),
            calls_per_hour: DEFAULT_SUPPORTER_CALLS_PER_HOUR,
        }
    }
}

impl SupporterConfig {
    pub fn active_id(&self) -> Option<&str> {
        self.adapter.as_deref().filter(|id| !id.is_empty())
    }

    pub fn connection(&self, id: &str) -> SupporterConnection {
        self.connections.get(id).cloned().unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SupporterConnection {
    pub model: Option<String>,
    pub endpoint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryRerankConfig {
    pub docs_hybrid: bool,
    pub sources: SupporterSources,
    pub candidates: usize,
    pub max_concurrency: usize,
    pub request_mode: RerankRequestMode,
    pub low_confidence_threshold: f64,
    pub max_body_chars: usize,
}

impl Default for MemoryRerankConfig {
    fn default() -> Self {
        Self {
            docs_hybrid: false,
            sources: SupporterSources::default(),
            candidates: 50,
            max_concurrency: 16,
            request_mode: RerankRequestMode::default(),
            low_confidence_threshold: 0.15,
            max_body_chars: 2000,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RerankRequestMode {
    #[default]
    PerCandidate,
    SharedState,
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

#[cfg(test)]
mod tests;
