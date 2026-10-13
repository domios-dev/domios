use codeterm_types::agent::chat_types::{ChatAttachment, ChatMsg};
use codeterm_types::pty::PendingPrompt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    String(String),
    Number(i64),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RpcFailure {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum RpcReply {
    Success { value: Value },
    Error { error: RpcFailure },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeFrame {
    Notification {
        method: String,
        params: Value,
    },
    Request {
        request_id: RpcId,
        method: String,
        params: Value,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TurnInput {
    pub text: String,
    pub attachments: Vec<ChatAttachment>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum PromptAnswer {
    Choice { values: Vec<String> },
    Text { text: String },
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeFailureCode {
    Unsupported,
    InvalidSchema,
    SizeLimit,
    HookFailed,
    MethodNotAllowed,
    BindingMismatch,
    Deadline,
    ProcessUnavailable,
    ProtocolError,
    ProviderRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeFailure {
    pub code: RuntimeFailureCode,
    pub message: String,
}

impl RuntimeFailure {
    pub fn new(code: RuntimeFailureCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RuntimeFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}
impl std::error::Error for RuntimeFailure {}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClaimedSend {
    pub send_id: String,
    pub thread_id: String,
    pub input: TurnInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PendingRuntimeRequest {
    pub request_id: RpcId,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeContext {
    #[serde(default)]
    pub thread_parents: Vec<RuntimeThreadParent>,
    #[serde(default)]
    pub active_turns: Vec<RuntimeActiveTurn>,
    pub epoch: u64,
    pub tab_id: String,
    pub binding_nonce: String,
    pub cwd: String,
    pub host_version: String,
    pub claimed_sends: Vec<ClaimedSend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_turn_id: Option<String>,
    pub settings: RuntimeSettings,
    pub provider_settings: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_request: Option<PendingRuntimeRequest>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeOperation {
    Initialize,
    OpenThread {
        instructions: String,
        settings: RuntimeSettings,
    },
    ResumeThread {
        thread_id: String,
        settings: RuntimeSettings,
    },
    ReadHistory {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    },
    SubmitTurn {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_turn_id: Option<String>,
        send_id: String,
        input: TurnInput,
        settings: RuntimeSettings,
    },
    Interrupt {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        active_turn_id: Option<String>,
    },
    AnswerQuestion {
        thread_id: String,
        item_id: String,
        answers: Vec<RuntimeQuestionAnswer>,
    },
    Answer {
        request_id: RpcId,
        answer: PromptAnswer,
    },
    Control {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        action: String,
        value: Value,
    },
}

impl RuntimeOperation {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::OpenThread { .. } => "openThread",
            Self::ResumeThread { .. } => "resumeThread",
            Self::ReadHistory { .. } => "readHistory",
            Self::SubmitTurn { .. } => "submitTurn",
            Self::Interrupt { .. } => "interrupt",
            Self::Answer { .. } => "answer",
            Self::AnswerQuestion { .. } => "answerQuestion",
            Self::Control { .. } => "control",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RpcCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<RuntimeCallTarget>,
    pub method: String,
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeAction {
    QuestionAnswer { input: TurnInput },
    Request { call: RpcCall },
    Notification { notification: RpcCall },
    Reply { request_id: RpcId, result: RpcReply },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimePlan {
    pub action: RuntimeAction,
    pub state: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OpenItem {
    pub turn_id: String,
    pub item: ChatMsg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeResult {
    Initialized,
    ThreadLoaded {
        thread_id: String,
        settings: RuntimeSettings,
    },
    HistoryPage {
        thread_id: String,
        items: Vec<ChatMsg>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
        open_items: Vec<OpenItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        questions: Option<Vec<RuntimeHistoryQuestion>>,
    },
    TurnAccepted {
        thread_id: String,
        send_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeStep {
    Continue {
        plan: RuntimePlan,
        #[serde(default)]
        before_acceptance: bool,
    },
    Complete {
        result: RuntimeResult,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        events: Option<Vec<AgentRuntimeEvent>>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notification: Option<RpcCall>,
    },
    Refused {
        reason: RuntimeFailure,
        before_acceptance: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub used_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeTurnState {
    Working,
    Idle,
    NeedsInput,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentRuntimeEvent {
    Auth {
        state: RuntimeAuthState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
    Question {
        thread_id: String,
        item_id: String,
        questions: Vec<RuntimeQuestion>,
        response: RuntimeQuestionResponse,
    },
    RateLimits {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
        rate_limits: Vec<RuntimeRateLimit>,
    },
    ChatItem {
        thread_id: String,
        item: ChatMsg,
    },
    TurnState {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        started_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        completed_at: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parent_thread_id: Option<String>,
        state: RuntimeTurnState,
    },
    SendEvidence {
        thread_id: String,
        send_id: String,
        turn_id: String,
    },
    Usage {
        thread_id: String,
        usage: TokenUsage,
    },
    Prompt {
        thread_id: String,
        request_id: RpcId,
        prompt: PendingPrompt,
    },
    PromptResolved {
        thread_id: String,
        request_id: RpcId,
    },
    ControlsConfirmed {
        thread_id: String,
        settings: RuntimeSettings,
    },
    ContextCompacted {
        thread_id: String,
        item_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeReduction {
    pub state: Value,
    pub events: Vec<AgentRuntimeEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostAgentRuntimeEvent {
    pub at: u64,
    #[serde(flatten)]
    pub event: AgentRuntimeEvent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRateLimit {
    pub window: String,
    pub used_percent: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resets_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_minutes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeQuestion {
    pub id: String,
    pub text: String,
    pub options: Vec<codeterm_types::pty::PromptOption>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multi_select: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_free_text: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeHistoryQuestion {
    pub thread_id: String,
    pub item_id: String,
    pub questions: Vec<RuntimeQuestion>,
    pub response: RuntimeQuestionResponse,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeQuestionAnswer {
    pub question_id: String,
    pub values: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeQuestionResponse {
    Message,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeActiveTurn {
    pub thread_id: String,
    pub turn_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_thread_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeAuthState {
    SignedIn,
    SignedOut,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCallTarget {
    pub thread_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeThreadParent {
    pub thread_id: String,
    pub parent_thread_id: String,
}
