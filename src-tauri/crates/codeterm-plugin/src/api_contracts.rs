use crate::agent_runtime::RuntimeBackgroundProcess;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundProcessEntry {
    pub thread_id: String,
    #[serde(flatten)]
    pub process: RuntimeBackgroundProcess,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct BackgroundProcessSnapshot {
    pub root_thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub processes: Vec<BackgroundProcessEntry>,
}

use crate::agent_runtime::{RuntimeQuestion, RuntimeQuestionAnswer, RuntimeQuestionResponse};

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "AgentRuntimeQuestionReceipt")]
pub struct QuestionReceipt {
    pub answers: Vec<RuntimeQuestionAnswer>,
    pub send_id: String,
    pub status: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "AgentRuntimeQuestion")]
pub struct QuestionRecord {
    pub thread_id: String,
    pub item_id: String,
    pub questions: Vec<RuntimeQuestion>,
    pub response: RuntimeQuestionResponse,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resolved: Option<QuestionReceipt>,
}

use crate::agent_runtime::{RuntimeAccountCredits, RuntimeRateLimit, TokenUsage};

#[derive(Clone, Serialize, ts_rs::TS)]
#[ts(rename = "RuntimeUsageReading")]
pub struct Reading<T> {
    pub captured_at: u64,
    pub value: T,
}

#[derive(Clone, Serialize, ts_rs::TS)]
#[ts(rename = "RuntimeUsageProjection")]
pub struct UsageProjection {
    pub tab_id: String,
    pub thread_id: Option<String>,
    #[serde(skip)]
    #[ts(skip)]
    pub provider: String,
    #[serde(skip)]
    #[ts(skip)]
    pub nonce: String,
    #[serde(skip)]
    #[ts(skip)]
    pub epoch: u64,
    pub billing_mode: codeterm_usage::contract::BillingMode,
    pub captured_at: u64,
    pub token_usage: Option<Reading<TokenUsage>>,
    pub rate_limits: Option<Reading<Vec<RuntimeRateLimit>>>,
    pub resets: Option<crate::agent_runtime::ResetCredits>,
    pub account_credits: Option<RuntimeAccountCredits>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalBinding {
    pub thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum GoalSnapshot {
    Unsupported {
        supported: bool,
    },
    Supported {
        supported: bool,
        binding: GoalBinding,
        intent: Option<codeterm_types::agent::chat_types::ChatSessionIntent>,
        actions: Vec<crate::agent_runtime::RuntimeGoalAction>,
    },
}

#[derive(Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum UsageLatestResponse {
    Stored {
        #[serde(flatten)]
        snapshot: codeterm_usage::contract::UsageSnapshotRow,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        runtime_usage: Option<Vec<UsageProjection>>,
    },
    RuntimeOnly {
        provider: String,
        limits_reported: bool,
        runtime_usage: Vec<UsageProjection>,
    },
}

#[cfg(test)]
mod tests;
pub use codeterm_plugins::snapshot_contracts::Capabilities as PluginSnapshotCapabilities;
pub use codeterm_plugins::snapshot_contracts::{MarketplaceRow, Snapshot as PluginSnapshot};

pub use codeterm_plugins::{ChannelDiffRow, ChannelListRow, InstallSource};
