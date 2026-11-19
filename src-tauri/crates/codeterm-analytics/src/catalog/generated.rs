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
pub enum Resource {
    #[serde(rename = "workspaces")]
    Workspaces,
    #[serde(rename = "teams")]
    Teams,
    #[serde(rename = "agent_tabs")]
    AgentTabs,
    #[serde(rename = "mesh_machines")]
    MeshMachines,
    #[serde(rename = "memory_items")]
    MemoryItems,
    #[serde(rename = "neighbors")]
    Neighbors,
}
impl Resource {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "workspaces" => Some(Self::Workspaces),
            "teams" => Some(Self::Teams),
            "agent_tabs" => Some(Self::AgentTabs),
            "mesh_machines" => Some(Self::MeshMachines),
            "memory_items" => Some(Self::MemoryItems),
            "neighbors" => Some(Self::Neighbors),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpgradeSource {
    #[serde(rename = "limit_dialog")]
    LimitDialog,
    #[serde(rename = "account_settings")]
    AccountSettings,
}
impl UpgradeSource {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "limit_dialog" => Some(Self::LimitDialog),
            "account_settings" => Some(Self::AccountSettings),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CheckoutOutcome {
    #[serde(rename = "opened")]
    Opened,
    #[serde(rename = "returned")]
    Returned,
    #[serde(rename = "failed")]
    Failed,
}
impl CheckoutOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "opened" => Some(Self::Opened),
            "returned" => Some(Self::Returned),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SettingsSection {
    #[serde(rename = "root")]
    Root,
    #[serde(rename = "account")]
    Account,
    #[serde(rename = "privacy")]
    Privacy,
    #[serde(rename = "general")]
    General,
    #[serde(rename = "mesh")]
    Mesh,
    #[serde(rename = "notifications")]
    Notifications,
    #[serde(rename = "agents")]
    Agents,
    #[serde(rename = "secrets")]
    Secrets,
    #[serde(rename = "jobs")]
    Jobs,
    #[serde(rename = "logs")]
    Logs,
    #[serde(rename = "guide")]
    Guide,
    #[serde(rename = "about")]
    About,
    #[serde(rename = "agent_provider")]
    AgentProvider,
}
impl SettingsSection {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "root" => Some(Self::Root),
            "account" => Some(Self::Account),
            "privacy" => Some(Self::Privacy),
            "general" => Some(Self::General),
            "mesh" => Some(Self::Mesh),
            "notifications" => Some(Self::Notifications),
            "agents" => Some(Self::Agents),
            "secrets" => Some(Self::Secrets),
            "jobs" => Some(Self::Jobs),
            "logs" => Some(Self::Logs),
            "guide" => Some(Self::Guide),
            "about" => Some(Self::About),
            "agent_provider" => Some(Self::AgentProvider),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderChangeOutcome {
    #[serde(rename = "applied")]
    Applied,
    #[serde(rename = "pending")]
    Pending,
}
impl ProviderChangeOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "applied" => Some(Self::Applied),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelChangeMode {
    #[serde(rename = "live")]
    Live,
    #[serde(rename = "restart")]
    Restart,
    #[serde(rename = "next_turn")]
    NextTurn,
}
impl ModelChangeMode {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "live" => Some(Self::Live),
            "restart" => Some(Self::Restart),
            "next_turn" => Some(Self::NextTurn),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelChangeOutcome {
    #[serde(rename = "observed")]
    Observed,
    #[serde(rename = "pending")]
    Pending,
}
impl ModelChangeOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "observed" => Some(Self::Observed),
            "pending" => Some(Self::Pending),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MeshOperation {
    #[serde(rename = "plan")]
    Plan,
    #[serde(rename = "spec")]
    Spec,
    #[serde(rename = "effort")]
    Effort,
    #[serde(rename = "task")]
    Task,
    #[serde(rename = "team")]
    Team,
    #[serde(rename = "other")]
    Other,
}
impl MeshOperation {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "plan" => Some(Self::Plan),
            "spec" => Some(Self::Spec),
            "effort" => Some(Self::Effort),
            "task" => Some(Self::Task),
            "team" => Some(Self::Team),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum RequestOutcome {
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "refused")]
    Refused,
    #[serde(rename = "failure")]
    Failure,
}
impl RequestOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "refused" => Some(Self::Refused),
            "failure" => Some(Self::Failure),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanvasOperation {
    #[serde(rename = "create")]
    Create,
    #[serde(rename = "update")]
    Update,
    #[serde(rename = "delete")]
    Delete,
}
impl CanvasOperation {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "create" => Some(Self::Create),
            "update" => Some(Self::Update),
            "delete" => Some(Self::Delete),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CanvasKind {
    #[serde(rename = "note")]
    Note,
    #[serde(rename = "mermaid")]
    Mermaid,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "page")]
    Page,
    #[serde(rename = "document")]
    Document,
    #[serde(rename = "canvas")]
    Canvas,
    #[serde(rename = "other")]
    Other,
}
impl CanvasKind {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "note" => Some(Self::Note),
            "mermaid" => Some(Self::Mermaid),
            "image" => Some(Self::Image),
            "html" => Some(Self::Html),
            "page" => Some(Self::Page),
            "document" => Some(Self::Document),
            "canvas" => Some(Self::Canvas),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DsBlockKind {
    #[serde(rename = "html")]
    Html,
    #[serde(rename = "page")]
    Page,
    #[serde(rename = "image")]
    Image,
    #[serde(rename = "document")]
    Document,
    #[serde(rename = "canvas")]
    Canvas,
    #[serde(rename = "guide")]
    Guide,
    #[serde(rename = "plugin")]
    Plugin,
    #[serde(rename = "connect")]
    Connect,
    #[serde(rename = "setting")]
    Setting,
    #[serde(rename = "agent")]
    Agent,
    #[serde(rename = "job")]
    Job,
    #[serde(rename = "checklist")]
    Checklist,
    #[serde(rename = "choice")]
    Choice,
    #[serde(rename = "tasks")]
    Tasks,
}
impl DsBlockKind {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "html" => Some(Self::Html),
            "page" => Some(Self::Page),
            "image" => Some(Self::Image),
            "document" => Some(Self::Document),
            "canvas" => Some(Self::Canvas),
            "guide" => Some(Self::Guide),
            "plugin" => Some(Self::Plugin),
            "connect" => Some(Self::Connect),
            "setting" => Some(Self::Setting),
            "agent" => Some(Self::Agent),
            "job" => Some(Self::Job),
            "checklist" => Some(Self::Checklist),
            "choice" => Some(Self::Choice),
            "tasks" => Some(Self::Tasks),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DsBlockAction {
    #[serde(rename = "open")]
    Open,
    #[serde(rename = "submit")]
    Submit,
}
impl DsBlockAction {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "open" => Some(Self::Open),
            "submit" => Some(Self::Submit),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum BlueprintOutcome {
    #[serde(rename = "success")]
    Success,
    #[serde(rename = "failure")]
    Failure,
    #[serde(rename = "cancelled")]
    Cancelled,
}
impl BlueprintOutcome {
    pub fn from_catalog(value: &str) -> Option<Self> {
        match value {
            "success" => Some(Self::Success),
            "failure" => Some(Self::Failure),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProviderId {
    #[serde(rename = "acp")]
    Acp,
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
            "acp" => Some(Self::Acp),
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
    AppFirstRun {
        version: Version,
        platform: Platform,
    },
    AppOpened {},
    PlanViewed {},
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
    LimitReachedShown {
        resource: Resource,
    },
    UpgradeClicked {
        source: UpgradeSource,
    },
    CheckoutOpened {
        outcome: CheckoutOutcome,
    },
    CheckoutReturned {
        outcome: CheckoutOutcome,
    },
    FriendAdded {
        count: u32,
    },
    NeighborMessageSent {
        count: u32,
    },
    FileShared {
        count: u32,
    },
    ReadReceiptShown {
        count: u32,
    },
    SettingsViewed {
        section: SettingsSection,
    },
    ChatMessageSubmitted {},
    MemorySearched {},
    CanvasOpened {},
    OnboardingCompleted {},
    AgentProviderChanged {
        provider_id: ProviderId,
        outcome: ProviderChangeOutcome,
    },
    AgentModelChanged {
        mode: ModelChangeMode,
        outcome: ModelChangeOutcome,
    },
    AgentTabSelected {},
    MeshRequestCompleted {
        operation: MeshOperation,
        outcome: RequestOutcome,
    },
    CanvasElementSaved {
        operation: CanvasOperation,
        kind: CanvasKind,
    },
    DsBlockInteracted {
        kind: DsBlockKind,
        action: DsBlockAction,
    },
    BlueprintRunCompleted {
        outcome: BlueprintOutcome,
    },
    PluginEnabledChanged {
        enabled: bool,
    },
    UpdateChecked {
        offers: u32,
    },
    AccountCreated {
        auth_provider: AuthProvider,
    },
    NeighborInviteCreated {},
}
