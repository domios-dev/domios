use crate::{
    permissions::Permission,
    supervisor::ListenerState,
    wire::{DeliveryStatus, Kind},
};
use codeterm_wire::api_contracts::{MessageFlag, NeighborNotify, State};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(untagged)]
#[ts(rename = "NeighborHistoryAuthor")]
pub enum HistoryAuthor {
    Person(String),
    Agent { agent: String, name: String },
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "NeighborHistorySubmission")]
pub enum Submission {
    Accepted,
    Unconfirmed,
    Refused,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "NeighborHistoryEntry")]
pub struct HistoryEntry {
    pub direction: String,
    pub author: HistoryAuthor,
    pub sender_uid: String,
    pub body: String,
    pub at: u64,
    pub message_id: String,
    pub kind: Kind,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub in_reply_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub limit: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub resets_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub retry_after_seconds: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivery_status: Option<DeliveryStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub delivered_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub read_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub submission: Option<Submission>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handling_state: Option<State>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handling_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub handling_updated_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<MessageFlag>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "NeighborHistoryPage")]
pub struct HistoryPage {
    pub entries: Vec<HistoryEntry>,
    pub next_before: Option<String>,
    pub ended: bool,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "NeighborReceiverState")]
pub struct ReceiverState {
    pub receiving_here: bool,
    pub receiver_machine_label: Option<String>,
    pub receiving_enabled: bool,
    pub revision: u64,
    pub is_me: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub not_receiving_reason: Option<NotReceivingReason>,
}

#[derive(Clone, Copy, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum NotReceivingReason {
    MembershipExpired,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct NeighborPermission {
    #[serde(flatten)]
    pub permission: Permission,
    pub initial_context_digest: Option<String>,
    pub effective_folder: String,
    pub folder_enforcement: FolderEnforcement,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct FolderEnforcement {
    pub writes: FolderWrites,
    pub reads: FolderReads,
    pub network: FolderNetwork,
    pub note: String,
}
#[derive(Clone, Copy, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum FolderWrites {
    ProviderSandbox,
    PadOnly,
    Unrestricted,
    Advisory,
}
#[derive(Clone, Copy, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum FolderReads {
    NotConfined,
}
#[derive(Clone, Copy, Debug, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum FolderNetwork {
    NotConfined,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, ts_rs::TS)]
pub struct ListenerFailure {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub operation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional = nullable)]
    pub last_error: Option<Option<Box<ListenerFailure>>>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct NeighborWatchState {
    pub status: ListenerState,
    pub error: Option<ListenerFailure>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct NeighborHandlingSummary {
    pub message_id: String,
    pub handling_state: State,
    pub handling_reason: Option<String>,
    pub handling_updated_at: Option<u64>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
pub struct NeighborConciergeState {
    pub concierge_dormant: bool,
    pub reasoning_effort: Option<String>,
    pub service_tier: Option<String>,
    pub initial_context_digest: Option<String>,
    pub gateway_tab: Option<String>,
    pub paused: bool,
    pub my_paused: bool,
    pub concierge_missing: bool,
    pub pending_count: Option<u64>,
    pub pending_count_capped: bool,
    pub receiving_here: bool,
    pub receiver_machine_label: Option<String>,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub not_receiving_reason: Option<NotReceivingReason>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[ts(rename = "NeighborChannelStatus")]
pub struct ChannelStatus {
    pub watches: BTreeMap<String, NeighborWatchState>,
    pub incoming_handling: BTreeMap<String, Vec<NeighborHandlingSummary>>,
    pub enabled: bool,
    pub enrolled: bool,
    pub bindings: BTreeMap<String, String>,
    pub permissions: BTreeMap<String, NeighborPermission>,
    pub concierges: BTreeMap<String, NeighborConciergeState>,
    pub friend_receivers: BTreeMap<String, ReceiverState>,
    pub friend_receivers_version: u64,
    pub machine_label: String,
    pub notification_preferences: BTreeMap<String, NeighborNotify>,
    pub listeners: BTreeMap<String, ListenerState>,
    pub listener_errors: BTreeMap<String, ListenerFailure>,
}

#[derive(Clone, Debug, Serialize, ts_rs::TS)]
#[serde(untagged)]
pub enum NeighborListenerEvent {
    Listener {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        friendship_id: Option<String>,
        status: ListenerState,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional = nullable)]
        error: Option<Option<ListenerFailure>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        reason: Option<String>,
    },
    Receiver {
        #[serde(flatten)]
        receiver: ReceiverState,
        friendship_id: String,
        friend_receivers_version: u64,
    },
    Unavailable {
        receiver_unavailable: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        friendship_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        friend_receivers_version: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        message: Option<String>,
    },
}
