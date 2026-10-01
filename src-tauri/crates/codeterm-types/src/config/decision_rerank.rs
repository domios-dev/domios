use serde::{Deserialize, Serialize};

/// Stage-2 decision re-rank over mem recall; off unless enabled here or per request.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionRerankConfig {
    pub enabled: bool,
    pub docs_hybrid: bool,
    /// Registry id of the decision adapter resolved at search time.
    pub adapter: String,
    pub sources: DecisionSources,
    pub max_concurrency: usize,
    pub deadline_ms: u64,
    pub low_confidence_threshold: f64,
    pub max_body_chars: usize,
    pub endpoint: Option<String>,
    pub model: Option<String>,
}

impl Default for DecisionRerankConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            docs_hybrid: false,
            adapter: "typesafe-jev".into(),
            sources: DecisionSources::default(),
            max_concurrency: 16,
            deadline_ms: 3000,
            low_confidence_threshold: 0.15,
            max_body_chars: 2000,
            endpoint: None,
            model: None,
        }
    }
}

/// Candidate sources pooled for stage 2: recall hits, plus optional sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DecisionSources {
    pub recall: usize,
    pub graph: usize,
    pub docs: usize,
    pub sessions: usize,
}

impl Default for DecisionSources {
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
