use serde::{Deserialize, Serialize};

pub type TranscriptSeq = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptRange {
    pub from_seq: TranscriptSeq,
    pub to_seq: TranscriptSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TranscriptContent {
    Speech { text: String },
    Paused { reason: String },
    Resumed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptInput {
    pub at_ms: u64,
    pub content: TranscriptContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptRow {
    pub seq: TranscriptSeq,
    pub at_ms: u64,
    pub content: TranscriptContent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AppendReceipt {
    pub row: TranscriptRow,
    pub sealed: Vec<TranscriptRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum BindingTarget {
    Attached { epoch_id: String },
    Local,
    External { tab_id: String },
    Detached,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorStart {
    Beginning,
    Latest,
    After(TranscriptSeq),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptBinding {
    pub owner_tab_id: String,
    pub revision: u64,
    pub target: BindingTarget,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptCursor {
    pub consumer_key: String,
    pub acked_seq: TranscriptSeq,
    pub seen_seq: TranscriptSeq,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ConciergeEpoch {
    pub epoch_id: String,
    pub runtime_tab_id: String,
    pub session_id: String,
    pub binding_nonce: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EpochArchive {
    pub history_ref: String,
    pub ended_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct EpochHistory {
    pub epoch: ConciergeEpoch,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<EpochArchive>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RoutedAction {
    pub action_id: String,
    pub range: TranscriptRange,
    pub at_ms: u64,
    pub destination: String,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RoutedActionRecord {
    pub action_seq: u64,
    pub binding: TranscriptBinding,
    pub consumer_key: String,
    pub action: RoutedAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RoutedActionsPage {
    pub actions: Vec<RoutedActionRecord>,
    pub next_after_seq: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SegmentSummary {
    pub range: TranscriptRange,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptPosition {
    pub seq: TranscriptSeq,
    pub char_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AgentTranscriptPage {
    pub text: String,
    pub through_seq: TranscriptSeq,
    pub committed_seq: TranscriptSeq,
    pub next: Option<TranscriptPosition>,
    pub gaps: Vec<TranscriptRange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptHistoryPage {
    pub rows: Vec<TranscriptRow>,
    pub gaps: Vec<TranscriptRange>,
    pub next_from_seq: Option<TranscriptSeq>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum TranscriptSearchHit {
    Raw { row: TranscriptRow },
    Summary { summary: SegmentSummary },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct TranscriptSearchPage {
    pub hits: Vec<TranscriptSearchHit>,
    pub gaps: Vec<TranscriptRange>,
    pub next_from_seq: Option<TranscriptSeq>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ClosedTranscriptOwner {
    pub tab_id: String,
    pub group_id: String,
    pub closed_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum TranscriptOwnerState {
    Open { live_tab_id: String },
    Closed { identity: ClosedTranscriptOwner },
    Retired { identity: ClosedTranscriptOwner },
}
