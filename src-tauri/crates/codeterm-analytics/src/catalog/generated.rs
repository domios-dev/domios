use super::Version;
use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Feature {
    #[serde(rename = "dreaming_run")]
    DreamingRun,
    #[serde(rename = "cron_created")]
    CronCreated,
    #[serde(rename = "blueprint_created")]
    BlueprintCreated,
    #[serde(rename = "theme_changed")]
    ThemeChanged,
    #[serde(rename = "telegram_bridge_used")]
    TelegramBridgeUsed,
    #[serde(rename = "voice_input_used")]
    VoiceInputUsed,
    #[serde(rename = "plugin_installed")]
    PluginInstalled,
    #[serde(rename = "team_created")]
    TeamCreated,
    #[serde(rename = "memory_saved")]
    MemorySaved,
    #[serde(rename = "web_ui_opened")]
    WebUiOpened,
}
impl Feature {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "dreaming_run" => Some(Self::DreamingRun),
            "cron_created" => Some(Self::CronCreated),
            "blueprint_created" => Some(Self::BlueprintCreated),
            "theme_changed" => Some(Self::ThemeChanged),
            "telegram_bridge_used" => Some(Self::TelegramBridgeUsed),
            "voice_input_used" => Some(Self::VoiceInputUsed),
            "plugin_installed" => Some(Self::PluginInstalled),
            "team_created" => Some(Self::TeamCreated),
            "memory_saved" => Some(Self::MemorySaved),
            "web_ui_opened" => Some(Self::WebUiOpened),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
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
        mesh_machines: u32,
        workspaces: u32,
    },
    MeshPeerPaired {
        mesh_machines: u32,
    },
    WorkspaceCreated {
        workspaces: u32,
    },
    SignedIn {
        auth_provider: AuthProvider,
    },
    CheckoutStarted {
        plan: PlanKey,
    },
    SubscriptionChanged {
        plan: PlanKey,
        status: SubscriptionStatus,
    },
    AnalyticsOptedIn {},
    AgentSpawned {
        provider_id: ProviderId,
        role: Role,
    },
    UpdateInstalled {
        kind: UpdateKind,
    },
    PlanCreated {},
    ActiveDay {
        mesh_machines: u32,
        workspaces: u32,
    },
    FeatureUsed {
        feature: Feature,
    },
}
