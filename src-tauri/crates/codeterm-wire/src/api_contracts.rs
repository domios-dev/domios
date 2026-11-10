use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct LocalPreviewFile {
    pub data: String,
    pub media_type: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, ts_rs::TS)]
pub struct TabFailure {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub boot_stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub runtime_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub code: Option<String>,
    pub reason: String,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct TeamCloseLaneRecord {
    pub path: Option<String>,
    pub branch: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Serialize, ts_rs::TS)]
pub struct TeamCloseInventory {
    pub removed: Vec<TeamCloseLaneRecord>,
    pub kept: Vec<TeamCloseLaneRecord>,
    pub orphan_branches: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct OpenedThread {
    pub tab_id: String,
    pub chat_id: i64,
    pub thread_id: i64,
    pub topic_name: String,
    pub link: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum ActivitySource {
    Push,
    Health,
    Peer,
    Cron,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum ActivityLevel {
    Info,
    Warn,
    Error,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum FlagCategory {
    SuspiciousInstructions,
    PrivateDataRequest,
    MaliciousLink,
    Spam,
    Other,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum NeighborNotify {
    Off,
    #[default]
    InApp,
    InAppAndBridges,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
#[ts(rename = "EmbedderRefusal")]
pub struct Refusal {
    pub message: String,
    pub hints: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum RestApiPhase {
    Starting,
    Listening,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum RestApiDiagnostic {
    ListenerExited,
    BindOwnershipFailed,
    WorkersBlocked,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct RestApiStatus {
    pub generation: u64,
    pub phase: RestApiPhase,
    pub diagnostic: Option<RestApiDiagnostic>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
pub struct AppLog {
    pub id: i64,
    pub ts: i64,
    pub level: String,
    pub source: String,
    pub message: String,
    pub context_json: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum SessionBindingStateKind {
    Healthy,
    DeferredPending,
    Refused,
    NeverBound,
    Unbound,
    StalePending,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct SessionBindingState {
    pub tab_id: String,
    pub kind: SessionBindingStateKind,
    pub current_session_id: Option<String>,
    pub requested_session_id: Option<String>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    pub every_ms: u64,
    pub until_ms: Option<u64>,
    pub count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum DelayedMessageState {
    Queued,
    Stashed,
    Sending,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryTickView {
    pub at_ms: u64,
    pub outcome: String,
    pub spinal_verdict: String,
    pub spinal_reason: String,
}

#[derive(Clone, serde::Serialize, ts_rs::TS)]
pub struct ProviderSwitchOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub previous_provider: String,
    pub previous_session_id: Option<String>,
    pub previous_history_path: String,
    pub binding_restored: bool,
    pub handoff_delivered: bool,
}

#[derive(Clone, serde::Serialize, ts_rs::TS)]
pub struct ProviderSwitchStatus {
    pub operation_id: String,
    pub tab_id: String,
    pub provider: String,
    pub phase: String,
    pub outcome: Option<ProviderSwitchOutcome>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum TabBudgetLevel {
    Soft,
    Strong,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct TabBudgetNotice {
    pub workspace_tab_count: usize,
    pub level: TabBudgetLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct OrchestrationPairResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub plan_id: Option<String>,
    pub orchestrator_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub orchestrator_model: Option<String>,
    pub planner_tab_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub planner_model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub warning: Option<String>,
    /// Advisory tab-budget nudge (task_4d649dfd) for the workspace AFTER both
    /// tabs land — a pair adds two, so its count is the authoritative one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub budget_notice: Option<TabBudgetNotice>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceState {
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub viewport_owner: Option<String>,
    pub owner: String,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub target_id: String,
    pub activity: String,
    /// The agent driving `activity` when it is not the surface owner.
    #[serde(skip_serializing_if = "String::is_empty")]
    #[ts(as = "Option<String>", optional)]
    pub actor: String,
    pub needs_human: String,
    pub profile: String,
    pub profiles: Vec<String>,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "debug" => Self::Debug,
            "info" => Self::Info,
            "warn" | "warning" => Self::Warn,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

#[derive(Serialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "LocalRecordingCaptureLease")]
pub struct CaptureLeaseView {
    pub lease: String,
    pub plugin_id: String,
    pub tab_id: String,
    pub microphone_id: String,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "lowercase")]
#[ts(rename = "SurfaceNoticeTone")]
pub enum NoticeTone {
    Info,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "SurfaceNoticeKind")]
pub enum NoticeKind {
    PreviewPaused,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "NeighborFlagBy")]
pub enum FlagBy {
    Concierge,
    Tab,
    Owner,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[ts(rename = "NeighborMessageFlag")]
pub struct MessageFlag {
    pub category: FlagCategory,
    pub reason: String,
    pub by: FlagBy,
    pub at: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "NeighborHandlingState")]
pub enum State {
    DeliveredToConcierge,
    Working,
    Answered,
    Refused,
    NoReply,
    Failed,
    Waiting,
    Stopped,
}

#[derive(Debug, Clone, Default, Deserialize, ts_rs::TS)]
pub struct OrchestrationPairParams {
    pub orchestrator_agent: String,
    pub orchestrator_model: Option<String>,
    /// Requested reasoning effort for the Orchestrator. Capability-validated
    /// against the provider at spawn; unsupported values are rejected.
    #[serde(default)]
    pub orchestrator_reasoning_effort: Option<String>,
    pub planner_agent: String,
    pub planner_model: Option<String>,
    /// Requested reasoning effort for the Planner (same validation as above).
    #[serde(default)]
    pub planner_reasoning_effort: Option<String>,
    pub workspace_id: Option<String>,
    pub title: Option<String>,
    pub spawned_by_tab_id: Option<String>,
    /// Blank picker tab to claim for the Orchestrator in place, instead of
    /// spawning it as a brand-new tab (new-session picker confirm path).
    pub existing_tab_id: Option<String>,
    /// Explicit "I know I am a leaf and I am doing this deliberately"
    /// declaration — codeterm#400. A caller tab holding a leaf role (Manager
    /// or Worker) is refused unless this is set; it never widens who may
    /// create a pair from a non-leaf tab, and it never substitutes an
    /// intermediary — the newly minted Team is still attributed to the
    /// declaring tab itself.
    #[serde(default)]
    pub leaf_creation_acknowledged: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(rename = "BridgeTopicsState")]
pub enum TopicsState {
    Available,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct ThreadMenuState {
    pub bridge_active: bool,
    pub topics: TopicsState,
    pub bound: bool,
    pub link: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, ts_rs::TS)]
pub struct SurfaceNotice {
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub tone: Option<NoticeTone>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub kind: Option<NoticeKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub detail: Option<String>,
}

pub const PREVIEW_PAUSED_TEXT: &str = "Preview paused while the window is hidden";

impl SurfaceNotice {
    fn toned(text: impl Into<String>, tone: Option<NoticeTone>) -> Self {
        Self {
            text: text.into(),
            tone,
            kind: None,
            detail: None,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::toned(text, None)
    }

    pub fn info(text: impl Into<String>) -> Self {
        Self::toned(text, Some(NoticeTone::Info))
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::toned(text, Some(NoticeTone::Error))
    }

    pub fn preview_paused(detail: impl Into<String>) -> Self {
        Self {
            kind: Some(NoticeKind::PreviewPaused),
            detail: Some(detail.into()),
            ..Self::plain(PREVIEW_PAUSED_TEXT)
        }
    }
}

#[derive(Deserialize, ts_rs::TS)]
pub struct WorkerSpawnBody {
    pub intent: String,
    pub task: String,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Deserialize, Default, ts_rs::TS)]
#[ts(rename = "MemoryUpdateBody")]
pub struct UpdateBody {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Option<Vec<String>>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub pinned: Option<bool>,
    #[serde(default)]
    pub changed_by: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "MeshAttachmentKind")]
#[serde(rename_all = "snake_case")]
pub enum AttachmentKind {
    Voice,
    File,
}

#[derive(Debug, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "MeshAttachmentIngestRequest")]
pub struct IngestRequest {
    pub kind: AttachmentKind,
    pub name: String,
    pub size: u64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(rename = "MeshIngestedAttachment")]
pub struct IngestedAttachment {
    pub path: String,
    pub kind: AttachmentKind,
    pub size: u64,
    #[serde(default)]
    pub reference: Option<String>,
}

#[derive(Deserialize, ts_rs::TS)]
#[ts(rename = "MeshAttachmentTransferBody")]
pub struct TransferBody {
    pub ordinary_tab_id: String,
    pub peer_id: String,
    pub path: String,
    pub kind: AttachmentKind,
}

#[derive(Debug, Clone, Serialize, ts_rs::TS)]
pub struct LogEventPayload {
    pub ts: i64,
    pub level: String,
    pub source: String,
    pub message: String,
    pub context: Option<serde_json::Value>,
}

#[derive(Serialize, ts_rs::TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CaptureEvent {
    Start { capture: CaptureLeaseView },
    Stop { lease: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub struct ActivityEvent {
    pub id: Option<i64>,
    pub occurred_at: i64,
    pub source: ActivitySource,
    pub source_id: Option<String>,
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source_peer: Option<String>,
    pub level: ActivityLevel,
    pub message: String,
    pub meta: serde_json::Value,
}

#[cfg(test)]
mod tests;
