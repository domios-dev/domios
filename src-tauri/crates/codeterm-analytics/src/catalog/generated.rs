use super::Version;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Platform {
    #[serde(rename = "macos")]
    Macos,
    #[serde(rename = "windows")]
    Windows,
    #[serde(rename = "linux")]
    Linux,
}
impl Platform {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "macos" => Some(Self::Macos),
            "windows" => Some(Self::Windows),
            "linux" => Some(Self::Linux),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateChannel {
    #[serde(rename = "stable")]
    Stable,
    #[serde(rename = "rc")]
    Rc,
    #[serde(rename = "nightly")]
    Nightly,
}
impl UpdateChannel {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "stable" => Some(Self::Stable),
            "rc" => Some(Self::Rc),
            "nightly" => Some(Self::Nightly),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateKind {
    #[serde(rename = "app")]
    App,
    #[serde(rename = "frontend")]
    Frontend,
}
impl UpdateKind {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "app" => Some(Self::App),
            "frontend" => Some(Self::Frontend),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum TabKind {
    #[serde(rename = "shell")]
    Shell,
    #[serde(rename = "agent")]
    Agent,
}
impl TabKind {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "shell" => Some(Self::Shell),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Role {
    #[serde(rename = "general")]
    General,
    #[serde(rename = "orchestrator")]
    Orchestrator,
    #[serde(rename = "planner")]
    Planner,
    #[serde(rename = "worker")]
    Worker,
    #[serde(rename = "manager")]
    Manager,
    #[serde(rename = "supporter")]
    Supporter,
    #[serde(rename = "none")]
    None,
    #[serde(rename = "other")]
    Other,
}
impl Role {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "general" => Some(Self::General),
            "orchestrator" => Some(Self::Orchestrator),
            "planner" => Some(Self::Planner),
            "worker" => Some(Self::Worker),
            "manager" => Some(Self::Manager),
            "supporter" => Some(Self::Supporter),
            "none" => Some(Self::None),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum RestoreOutcome {
    #[serde(rename = "resumed")]
    Resumed,
    #[serde(rename = "fresh_launch")]
    FreshLaunch,
    #[serde(rename = "failed")]
    Failed,
}
impl RestoreOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "resumed" => Some(Self::Resumed),
            "fresh_launch" => Some(Self::FreshLaunch),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ExecutionIntent {
    #[serde(rename = "read_only")]
    ReadOnly,
    #[serde(rename = "mutating")]
    Mutating,
    #[serde(rename = "unspecified")]
    Unspecified,
}
impl ExecutionIntent {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "read_only" => Some(Self::ReadOnly),
            "mutating" => Some(Self::Mutating),
            "unspecified" => Some(Self::Unspecified),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthProvider {
    #[serde(rename = "local")]
    Local,
    #[serde(rename = "fake")]
    Fake,
    #[serde(rename = "firebase")]
    Firebase,
}
impl AuthProvider {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "local" => Some(Self::Local),
            "fake" => Some(Self::Fake),
            "firebase" => Some(Self::Firebase),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlanKey {
    #[serde(rename = "free")]
    Free,
    #[serde(rename = "pro")]
    Pro,
}
impl PlanKey {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "free" => Some(Self::Free),
            "pro" => Some(Self::Pro),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SubscriptionStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "past_due")]
    PastDue,
    #[serde(rename = "canceled")]
    Canceled,
    #[serde(rename = "revoked")]
    Revoked,
    #[serde(rename = "incomplete")]
    Incomplete,
    #[serde(rename = "trialing")]
    Trialing,
    #[serde(rename = "unpaid")]
    Unpaid,
    #[serde(rename = "paused")]
    Paused,
}
impl SubscriptionStatus {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "active" => Some(Self::Active),
            "past_due" => Some(Self::PastDue),
            "canceled" => Some(Self::Canceled),
            "revoked" => Some(Self::Revoked),
            "incomplete" => Some(Self::Incomplete),
            "trialing" => Some(Self::Trialing),
            "unpaid" => Some(Self::Unpaid),
            "paused" => Some(Self::Paused),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProviderId {
    #[serde(rename = "claude")]
    Claude,
    #[serde(rename = "codex")]
    Codex,
    #[serde(rename = "cursor")]
    Cursor,
    #[serde(rename = "opencode")]
    Opencode,
    #[serde(rename = "other")]
    Other,
}
impl ProviderId {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "claude" => Some(Self::Claude),
            "codex" => Some(Self::Codex),
            "cursor" => Some(Self::Cursor),
            "opencode" => Some(Self::Opencode),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelFamily {
    #[serde(rename = "anthropic/claude-opus-4-8")]
    AnthropicClaudeOpus48,
    #[serde(rename = "anthropic/claude-sonnet-4-6")]
    AnthropicClaudeSonnet46,
    #[serde(rename = "auto")]
    Auto,
    #[serde(rename = "best")]
    Best,
    #[serde(rename = "claude-4.6-sonnet-medium")]
    Claude46SonnetMedium,
    #[serde(rename = "claude-opus-4-8-thinking-high")]
    ClaudeOpus48ThinkingHigh,
    #[serde(rename = "claude-opus-4-8-thinking-low")]
    ClaudeOpus48ThinkingLow,
    #[serde(rename = "composer-2.5")]
    Composer25,
    #[serde(rename = "fable")]
    Fable,
    #[serde(rename = "gemini-3.1-pro")]
    Gemini31Pro,
    #[serde(rename = "gpt-5.2-codex")]
    Gpt52Codex,
    #[serde(rename = "gpt-5.5")]
    Gpt55,
    #[serde(rename = "gpt-5.5-high")]
    Gpt55High,
    #[serde(rename = "gpt-5.6-luna")]
    Gpt56Luna,
    #[serde(rename = "gpt-5.6-sol")]
    Gpt56Sol,
    #[serde(rename = "gpt-5.6-terra")]
    Gpt56Terra,
    #[serde(rename = "gpt-6-astra")]
    Gpt6Astra,
    #[serde(rename = "gpt-6-luna")]
    Gpt6Luna,
    #[serde(rename = "gpt-6-sol")]
    Gpt6Sol,
    #[serde(rename = "gpt-6.1-sol")]
    Gpt61Sol,
    #[serde(rename = "grok-4.3")]
    Grok43,
    #[serde(rename = "haiku")]
    Haiku,
    #[serde(rename = "kimi-k2.5")]
    KimiK25,
    #[serde(rename = "opencode/big-pickle")]
    OpencodeBigPickle,
    #[serde(rename = "opencode/deepseek-v4-flash-free")]
    OpencodeDeepseekV4FlashFree,
    #[serde(rename = "opencode/gpt-5.4-mini")]
    OpencodeGpt54Mini,
    #[serde(rename = "opencode/gpt-5.5")]
    OpencodeGpt55,
    #[serde(rename = "opus")]
    Opus,
    #[serde(rename = "opusplan")]
    Opusplan,
    #[serde(rename = "sonnet")]
    Sonnet,
    #[serde(rename = "unknown")]
    Unknown,
}
impl ModelFamily {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "anthropic/claude-opus-4-8" => Some(Self::AnthropicClaudeOpus48),
            "anthropic/claude-sonnet-4-6" => Some(Self::AnthropicClaudeSonnet46),
            "auto" => Some(Self::Auto),
            "best" => Some(Self::Best),
            "claude-4.6-sonnet-medium" => Some(Self::Claude46SonnetMedium),
            "claude-opus-4-8-thinking-high" => Some(Self::ClaudeOpus48ThinkingHigh),
            "claude-opus-4-8-thinking-low" => Some(Self::ClaudeOpus48ThinkingLow),
            "composer-2.5" => Some(Self::Composer25),
            "fable" => Some(Self::Fable),
            "gemini-3.1-pro" => Some(Self::Gemini31Pro),
            "gpt-5.2-codex" => Some(Self::Gpt52Codex),
            "gpt-5.5" => Some(Self::Gpt55),
            "gpt-5.5-high" => Some(Self::Gpt55High),
            "gpt-5.6-luna" => Some(Self::Gpt56Luna),
            "gpt-5.6-sol" => Some(Self::Gpt56Sol),
            "gpt-5.6-terra" => Some(Self::Gpt56Terra),
            "gpt-6-astra" => Some(Self::Gpt6Astra),
            "gpt-6-luna" => Some(Self::Gpt6Luna),
            "gpt-6-sol" => Some(Self::Gpt6Sol),
            "gpt-6.1-sol" => Some(Self::Gpt61Sol),
            "grok-4.3" => Some(Self::Grok43),
            "haiku" => Some(Self::Haiku),
            "kimi-k2.5" => Some(Self::KimiK25),
            "opencode/big-pickle" => Some(Self::OpencodeBigPickle),
            "opencode/deepseek-v4-flash-free" => Some(Self::OpencodeDeepseekV4FlashFree),
            "opencode/gpt-5.4-mini" => Some(Self::OpencodeGpt54Mini),
            "opencode/gpt-5.5" => Some(Self::OpencodeGpt55),
            "opus" => Some(Self::Opus),
            "opusplan" => Some(Self::Opusplan),
            "sonnet" => Some(Self::Sonnet),
            "unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "event_type",
    content = "event_properties",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Event {
    AppStarted {
        version: Version,
        platform: Platform,
        update_channel: UpdateChannel,
    },
    UpdateInstalled {
        from_version: Version,
        to_version: Version,
        kind: UpdateKind,
        attempt_count: u32,
    },
    WorkspaceCreated {},
    TabOpened {
        kind: TabKind,
        provider_id: Option<ProviderId>,
    },
    AgentSpawned {
        provider_id: ProviderId,
        role: Role,
        model_family: ModelFamily,
    },
    AgentRestored {
        provider_id: ProviderId,
        outcome: RestoreOutcome,
    },
    PlanCreated {},
    TaskDispatched {
        execution_intent: ExecutionIntent,
    },
    MeshPeerPaired {},
    SignedIn {
        auth_provider: AuthProvider,
    },
    CheckoutStarted {
        plan_key: PlanKey,
    },
    SubscriptionChanged {
        plan: PlanKey,
        status: SubscriptionStatus,
    },
    AnalyticsOptedIn {},
}
