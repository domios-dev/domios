use codeterm_types::agent::chat_types::{ChatAttachment, ChatMsg};
use codeterm_types::pty::PendingPrompt;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use codeterm_provider_signals::{RuntimeErrorSignal, RuntimeRetryableErrorKind};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
pub struct RuntimeServiceTierSelection {
    #[serde(deserialize_with = "deserialize_service_tier_id")]
    pub id: Option<String>,
}

fn deserialize_service_tier_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(deserializer)
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeServiceTier {
    pub id: String,
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub unavailable_reason: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commands: Option<Vec<RuntimeCommand>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config_options: Option<Vec<RuntimeConfigOption>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modes: Option<Vec<RuntimeMode>>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier: Option<RuntimeServiceTierSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_tier_availability:
        Option<std::collections::BTreeMap<String, Vec<RuntimeServiceTier>>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub additional_directories: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network_access: Option<bool>,
}

impl RuntimeSettings {
    fn mode_option(&self) -> Option<&RuntimeConfigOption> {
        self.config_options
            .iter()
            .flatten()
            .find(|option| option.category.as_deref() == Some("mode"))
    }

    /// The session's one mode: what its mode config option reads, otherwise its session mode.
    pub fn effective_mode(&self) -> Option<&str> {
        self.mode_option()
            .map(|option| option.current_value.as_str())
            .or(self.mode.as_deref())
    }

    /// Every mode the session can enter, through its mode config option or its session modes.
    pub fn offered_modes(&self) -> Vec<&str> {
        let mut offered = self
            .mode_option()
            .into_iter()
            .flat_map(|option| option.options.iter().map(|value| value.value.as_str()))
            .collect::<Vec<_>>();
        for mode in self.modes.iter().flatten() {
            if !offered.contains(&mode.id.as_str()) {
                offered.push(mode.id.as_str());
            }
        }
        offered
    }

    /// A session mode reported alone is news: the mode config option reads it too.
    pub fn follow_session_mode(&mut self, mode: String) {
        if let Some(option) = self.mode_option_mut() {
            option.current_value.clone_from(&mode);
        }
        self.mode = Some(mode);
    }

    /// Makes both places read one value when a report carries both: the option decides, except
    /// for a session mode it cannot express, which only `session/set_mode` can have entered.
    pub fn settle_mode(&mut self) {
        let session_mode = self.mode.clone();
        let Some(option) = self.mode_option_mut() else {
            return;
        };
        let current = match session_mode {
            Some(mode) if !option.options.iter().any(|value| value.value == mode) => {
                option.current_value.clone_from(&mode);
                mode
            }
            _ => option.current_value.clone(),
        };
        self.mode = Some(current);
    }

    fn mode_option_mut(&mut self) -> Option<&mut RuntimeConfigOption> {
        self.config_options
            .iter_mut()
            .flatten()
            .find(|option| option.category.as_deref() == Some("mode"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeCommand {
    pub name: String,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argument_hint: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeConfigOption {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    pub id: String,
    pub name: String,
    pub current_value: String,
    pub options: Vec<RuntimeConfigValue>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeConfigValue {
    pub value: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeMode {
    pub id: String,
    pub name: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeAuthentication {
    pub state: RuntimeAuthenticationState,
    pub methods: Vec<RuntimeAuthenticationMethod>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeAuthenticationState {
    Authenticated,
    NeedsAuth,
    Unavailable,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuntimeAuthenticationMethod {
    pub id: String,
    pub name: String,
    pub available: bool,
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
    AuthenticationRequired,
    ThreadNotFound,
    SessionLoadRefused,
    RuntimeStartUnbound,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored_row_ordinal: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_window: Option<super::RuntimeStreamWindow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host_mcp: Option<super::RuntimeHostMcp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_bridges: Option<super::RuntimeClientBridges>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restricted_cli: Option<RestrictedCliPolicy>,
    #[serde(default)]
    pub thread_parents: Vec<RuntimeThreadParent>,
    #[serde(default)]
    pub active_turns: Vec<RuntimeActiveTurn>,
    pub epoch: u64,
    pub tab_id: String,
    pub binding_nonce: String,
    #[serde(default)]
    pub host_owned: crate::provider::HostOwnedCapabilities,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RestrictedCliPolicy {
    pub tab_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub cwd: String,
    pub metadata_directory: String,
    pub executable: String,
    pub executable_sha256: String,
    pub config_sha256: String,
    pub rule_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeOperation {
    BackgroundProcesses {
        thread_id: String,
        action: super::RuntimeBackgroundProcessAction,
    },
    Initialize,
    OpenThread {
        instructions: String,
        settings: RuntimeSettings,
    },
    ForkThread {
        source_thread_id: String,
        turn_id: String,
    },
    ResumeThread {
        thread_id: String,
        instructions: String,
        settings: RuntimeSettings,
    },
    ReadHistory {
        thread_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        order: Option<HistoryOrder>,
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
            Self::BackgroundProcesses { .. } => "backgroundProcesses",
            Self::Initialize => "initialize",
            Self::OpenThread { .. } => "openThread",
            Self::ForkThread { .. } => "forkThread",
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
    Local,
    Refused { reason: String },
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HistoryOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeResult {
    Goal {
        thread_id: String,
        intent: Option<codeterm_types::agent::chat_types::ChatSessionIntent>,
    },
    BackgroundProcesses {
        thread_id: String,
        processes: Vec<super::RuntimeBackgroundProcess>,
        next_cursor: Option<String>,
    },
    BackgroundProcessTermination {
        thread_id: String,
        process_id: String,
        acknowledged: bool,
    },
    Initialized {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        binary_version: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_name: Option<String>,
        /// This agent takes input into a running turn, as it declared for this process.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        mid_turn_input: bool,
    },
    ThreadLoaded {
        thread_id: String,
        settings: RuntimeSettings,
    },
    ThreadForked {
        source_thread_id: String,
        thread_id: String,
        turn_id: String,
    },
    HistoryPage {
        thread_id: String,
        items: Vec<ChatMsg>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        next_cursor: Option<String>,
        open_items: Vec<OpenItem>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        questions: Option<Vec<RuntimeHistoryQuestion>>,
        /// Echo of the requested order; absent means ascending.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        order: Option<HistoryOrder>,
    },
    TurnAccepted {
        thread_id: String,
        send_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        turn_id: Option<String>,
    },
    ResetCreditConsumed {
        outcome: ResetCreditOutcome,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        history: Option<RuntimeResult>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<u64>,
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

/// Why a turn is in its state; each value pairs with one [`RuntimeTurnState`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeTurnDetail {
    Compacting,
    AwaitingApproval,
    AwaitingInput,
    SystemError,
    Interrupted,
    Failed,
}

impl RuntimeTurnDetail {
    pub fn state(self) -> RuntimeTurnState {
        match self {
            Self::Compacting => RuntimeTurnState::Working,
            Self::AwaitingApproval | Self::AwaitingInput | Self::SystemError => {
                RuntimeTurnState::NeedsInput
            }
            Self::Interrupted | Self::Failed => RuntimeTurnState::Idle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AgentRuntimeEvent {
    Authentication {
        authentication: RuntimeAuthentication,
    },
    Notice {
        thread_id: String,
        message: String,
    },
    BackgroundProcesses {
        thread_id: String,
        processes: Vec<super::RuntimeBackgroundProcess>,
    },
    AuthLogin {
        login: RuntimeLoginState,
    },
    Auth {
        state: RuntimeAuthState,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        billing_mode: Option<codeterm_usage::contract::BillingMode>,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        resets: Option<ResetCredits>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        account_credits: Option<RuntimeAccountCredits>,
    },
    ChatItem {
        thread_id: String,
        item: ChatMsg,
    },
    ChatItemPatch {
        thread_id: String,
        item: ChatMsg,
    },
    ChatItemAppend {
        thread_id: String,
        item_id: String,
        content: String,
        offset: usize,
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
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<RuntimeTurnDetail>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status_line: Option<String>,
    },
    SendEvidence {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        item_id: Option<String>,
        thread_id: String,
        send_id: String,
        turn_id: String,
    },
    Usage {
        thread_id: String,
        usage: TokenUsage,
    },
    Context {
        thread_id: String,
        #[serde(rename = "used_tokens")]
        used_tokens: u64,
        #[serde(rename = "window_tokens")]
        window_tokens: u64,
        #[serde(
            rename = "cost_micros",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        cost_micros: Option<u64>,
    },
    Prompt {
        thread_id: String,
        request_id: RpcId,
        prompt: PendingPrompt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        permission: Option<RuntimePermission>,
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
    ThreadReverted {
        thread_id: String,
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
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum RuntimeAccountCredits {
    Balance { balance: String },
    Unlimited,
}

/// Agent-declared meaning of a permission prompt; the host answers it from the launch policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimePermission {
    pub action: RuntimePermissionAction,
    pub options: Vec<RuntimePermissionOption>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePermissionAction {
    Read,
    Write,
    Command,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimePermissionOption {
    pub value: String,
    pub decision: RuntimePermissionDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resume_instructions_round_trip_as_required_thread_context() {
        let operation = RuntimeOperation::ResumeThread {
            thread_id: "own".into(),
            instructions: "opaque instructions".into(),
            settings: RuntimeSettings::default(),
        };
        let value = serde_json::to_value(&operation).unwrap();
        assert_eq!(value["instructions"], "opaque instructions");
        assert_eq!(
            serde_json::from_value::<RuntimeOperation>(value).unwrap(),
            operation
        );
        assert!(
            serde_json::from_value::<RuntimeOperation>(serde_json::json!({
                "kind":"resumeThread","threadId":"own","settings":{}
            }))
            .is_err()
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLoginState {
    pub status: RuntimeLoginStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<super::RuntimeLoginMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RuntimeLoginStatus {
    Pending,
    Completed,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetCredits {
    pub available_count: u64,
    pub next_expiry: Option<u64>,
    pub credits: Option<Vec<ResetCredit>>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResetCredit {
    pub id: String,
    pub status: ResetCreditStatus,
    pub expires_at: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetCreditStatus {
    Available,
    Redeeming,
    Redeemed,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResetCreditOutcome {
    Reset,
    NothingToReset,
    NoCredit,
    AlreadyRedeemed,
}
