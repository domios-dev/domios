use super::chat_types::SystemSend;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
pub struct SourceRowRef {
    pub source_session_id: String,
    pub record: SourceRecord,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SourceRecord {
    Jsonl {
        #[serde(default)]
        incarnation: String,
        record_start: u64,
        record_row: u32,
    },
    Structured {
        thread_id: String,
        item_id: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
pub struct MatchedRow {
    pub source: SourceRowRef,
    pub ordinal: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum ChatSendState {
    #[serde(rename = "held")]
    Pending,
    Written,
    #[serde(rename = "submit_unconfirmed")]
    Unknown,
    #[serde(rename = "delivered")]
    Sent,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
pub struct ChatSendStatus {
    pub send_id: String,
    pub tab_id: String,
    pub state: ChatSendState,
    pub error: Option<String>,
    pub ledger_id: String,
    pub sequence: u64,
    pub retained_from: u64,
    #[serde(rename = "correlationId", skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub correlation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub matched_row: Option<MatchedRow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pending_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub system_send: Option<SystemSend>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalBinding {
    pub thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
}
#[derive(Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LocalRecordingCommitOutcome {
    Tracked { status: ChatSendStatus },
    PreHandoffFailure { message: String },
    PossibleHandoff { message: String },
}

#[derive(serde::Serialize, ts_rs::TS)]
pub struct ChatSendResult {
    #[serde(flatten)]
    pub status: ChatSendStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub pad_file: Option<String>,
}

#[cfg(test)]
mod tests;
