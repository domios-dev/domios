use super::{recurrence::RecurrenceRule, DeliveryTickView, MessageOrigin};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelayedMessageState {
    Queued,
    Stashed,
    Sending,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayedMessage {
    pub id: String,
    pub tab_id: String,
    pub workspace_id: Option<String>,
    pub text: String,
    pub scheduled_at_ms: u64,
    pub created_at_ms: u64,
    pub state: DelayedMessageState,
    pub error: Option<String>,
    pub session_id_at_create: Option<String>,
    #[serde(default)]
    pub created_by_tab: Option<String>,
    #[serde(default)]
    pub created_by_label: Option<String>,
    #[serde(default)]
    pub created_by_model: Option<String>,
    #[serde(default)]
    pub recurrence: Option<RecurrenceRule>,
    #[serde(default)]
    pub series_start_at_ms: Option<u64>,
    #[serde(default)]
    pub tick_index: u64,
    #[serde(default)]
    pub skipped_ticks: u64,
    #[serde(default)]
    pub team_id: Option<String>,
    #[serde(default)]
    pub last_tick_unconfirmed: bool,
    #[serde(skip)]
    pub last_tick_send_sequence: u64,
    #[serde(default, skip_serializing_if = "MessageOrigin::is_user")]
    pub origin: MessageOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ticks: Vec<DeliveryTickView>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DelayedMessageView {
    #[serde(flatten)]
    pub message: DelayedMessage,
    pub current_session_id: Option<String>,
    pub session_changed: bool,
}

/// Who queued a reminder, resolved from the caller tab at enqueue time.
#[derive(Debug, Default, Clone)]
pub struct ReminderCreator {
    pub tab: Option<String>,
    pub label: Option<String>,
    pub model: Option<String>,
}
