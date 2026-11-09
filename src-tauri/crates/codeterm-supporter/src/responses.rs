use std::collections::BTreeMap;
use std::path::PathBuf;

use codeterm_decision::{
    DecisionMetadata, DecisionModelInfo, DecisionPricing, DecisionPrimitives, TokenMode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::connections::ConnectionError;
#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "SupporterSpendTotals")]
pub struct Totals {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub estimated_usd: Option<f64>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterTokenStatus {
    pub adapter: String,
    pub token_mode: TokenMode,
    pub present: bool,
    pub path: PathBuf,
    pub mode_ok: Option<bool>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterAdapter {
    pub id: String,
    pub token_mode: TokenMode,
    pub primitives: DecisionPrimitives,
    pub models: Vec<DecisionModelInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub token: Option<SupporterTokenStatus>,
    pub pricing: Option<DecisionPricing>,
    pub metadata: DecisionMetadata,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterSpend {
    pub today: Totals,
    pub month: Totals,
    pub exhausted: bool,
    pub next_probe_at: Option<i64>,
    pub storage_ready: bool,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterConnectionToken {
    pub present: bool,
    pub mode_ok: Option<bool>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterConnectionRow {
    pub adapter: String,
    pub model: Option<String>,
    pub active: bool,
    pub token_mode: Option<TokenMode>,
    pub token: SupporterConnectionToken,
    pub pricing: Option<DecisionPricing>,
    pub metadata: Option<DecisionMetadata>,
    pub error: Option<ConnectionError>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum SupporterMutation {
    Failure {
        error: ConnectionError,
    },
    Active {
        state: ActiveState,
        adapter: String,
        model: Option<String>,
    },
    Saved {
        state: SavedState,
        adapter: String,
        model: Option<String>,
        error: ConnectionError,
    },
    Model {
        adapter: String,
        model: Option<String>,
    },
    Token {
        adapter: String,
        present: bool,
    },
    Cleared {
        adapter: String,
        removed: bool,
    },
    Disconnected {
        adapter: String,
        removed: bool,
        active: Option<String>,
    },
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ActiveState {
    Active,
}

#[derive(Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum SavedState {
    SavedNotActive,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterBudget {
    pub reached: bool,
    pub calls_per_hour: Option<usize>,
    pub free_at: Option<i64>,
    pub storage_ready: bool,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterActivity {
    pub last_activity: Option<i64>,
    pub calls_last_hour: usize,
    pub calls_last_day: usize,
    pub last_verdict: BTreeMap<String, Value>,
    pub budget: SupporterBudget,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterObservationScheduler {
    pub questions_per_event: usize,
    pub global_in_flight: usize,
    pub per_tab_in_flight: usize,
    pub queue_limit: usize,
    pub active: usize,
    pub queued: usize,
    pub coalesced: u64,
    pub dropped: u64,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct SupporterStatus {
    pub state: SupporterState,
    pub token_mode: TokenMode,
    pub adapter: Option<String>,
    pub model: Option<String>,
    pub deadline_ms: u64,
    pub connected: bool,
    #[serde(flatten)]
    pub activity: SupporterActivity,
    pub observation_scheduler: SupporterObservationScheduler,
    pub pricing: Option<DecisionPricing>,
    pub spend: SupporterSpend,
    pub token: SupporterTokenStatus,
    pub adapter_registered: bool,
    pub adapters: Vec<SupporterAdapter>,
    pub connections: Vec<SupporterConnectionRow>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum SupporterState {
    Active,
    CreditExhausted,
    BudgetReached,
    NotConnected,
    NoToken,
    TokenUnusable,
    AdapterUnavailable,
}

#[cfg(test)]
mod tests;
