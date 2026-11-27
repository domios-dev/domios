use serde::{Deserialize, Serialize};

pub const WARM_BOOTSTRAP_SCHEMA_VERSION: u8 = 1;
pub const MAX_WAKE_UP_ITEMS: usize = 48;
pub const MAX_REGISTERED_SKILLS: usize = 16;
pub const MAX_IDENTITY_BYTES: usize = 4 * 1024;
pub const MAX_ITEM_BODY_BYTES: usize = 2 * 1024;
pub const MAX_SKILL_BODY_BYTES: usize = 8 * 1024;
pub const MAX_PROJECTION_BYTES: usize = 128 * 1024;
pub const WARM_BOOTSTRAP_BUDGET_SECS: f64 = 10.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WarmBootstrapProjection {
    pub schema_version: u8,
    pub account_id: String,
    pub owner_installation_id: String,
    pub target_installation_id: String,
    pub creation_generation: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_revision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<PortableOwnerIdentity>,
    #[serde(default)]
    pub wake_up_items: Vec<PortableMemEntry>,
    #[serde(default)]
    pub skills: Vec<PortableRegisteredSkill>,
    #[serde(default)]
    pub preferences: PortablePreferences,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableOwnerIdentity {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub soul_md: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nickname: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortableMemKind {
    Rule,
    Preference,
    Fact,
    Summary,
    Learning,
}

impl PortableMemKind {
    pub fn to_mem_kind(self) -> crate::mem::MemKind {
        match self {
            Self::Rule => crate::mem::MemKind::Rule,
            Self::Preference => crate::mem::MemKind::Preference,
            Self::Fact => crate::mem::MemKind::Fact,
            Self::Summary => crate::mem::MemKind::Summary,
            Self::Learning => crate::mem::MemKind::Learning,
        }
    }

    pub fn from_mem_kind_str(kind: &str) -> Option<Self> {
        match kind {
            "rule" => Some(Self::Rule),
            "preference" => Some(Self::Preference),
            "fact" => Some(Self::Fact),
            "summary" => Some(Self::Summary),
            "learning" => Some(Self::Learning),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableMemEntry {
    pub kind: PortableMemKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    pub importance: f64,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortableSkillSource {
    MemSkill,
    TrustedFolderSkill,
    PluginSkillRef { plugin_id: String, version: String },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableRegisteredSkill {
    pub slug: String,
    pub title: String,
    pub version: String,
    pub registry_source: PortableSkillSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub area: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    pub importance: f64,
    #[serde(default)]
    pub pinned: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableGeneralAgentPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability_hints: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableMemoryPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub l1_budget_tokens: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_extract: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub librarian_enabled: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableNetworkPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_idle_sleep_minutes: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud_max_awake_without_owner_minutes: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableAgentPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_sessions: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continue_after_restart: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_ceiling: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableProviderRef {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_sessions: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortableAcpProfileRef {
    pub profile_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry_id: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PortablePreferences {
    #[serde(default)]
    pub general_agent: PortableGeneralAgentPrefs,
    #[serde(default)]
    pub memory: PortableMemoryPrefs,
    #[serde(default)]
    pub network: PortableNetworkPrefs,
    #[serde(default)]
    pub agent: PortableAgentPrefs,
    #[serde(default)]
    pub configured_providers: Vec<PortableProviderRef>,
    #[serde(default)]
    pub acp_profiles: Vec<PortableAcpProfileRef>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionCategory {
    Identity,
    WakeUpItem,
    Skill,
    Preference,
    ProviderRef,
    AcpProfile,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExclusionReason {
    SecretShapedContent,
    ForbiddenPath,
    UntrustedSkillSource,
    InvalidSlug,
    NeighborAuthored,
    Oversized,
    BoundExceeded,
    DisallowedPreference,
    InvalidValue,
}

/// Sanitized exclusion receipt — names only the category and typed reason, never the rejected payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SanitizedExclusion {
    pub category: ExclusionCategory,
    pub reason: ExclusionReason,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmBootstrapStatus {
    Applied,
    Partial,
    AlreadyApplied,
    SkippedRestoredState,
    Rejected,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmBootstrapRejectReason {
    UnsupportedSchema,
    WrongAccount,
    InstallationMismatch,
    StaleGeneration,
    TargetNotEmpty,
    OversizedProjection,
    InvalidProvenance,
    ReadbackUnverified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSignInState {
    Ready,
    SignInRequired,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSignInCardStatus {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub display_name: String,
    pub state: ProviderSignInState,
    #[serde(default)]
    pub supported_methods: Vec<String>,
    #[serde(default)]
    pub code_input: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login_tab_id: Option<String>,
    #[serde(default)]
    pub requires_owner_input: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarmBootstrapAppliedCounts {
    pub identity_fields: usize,
    pub wake_up_items: usize,
    pub rules: usize,
    pub skills: usize,
    pub preference_fields: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarmBootstrapApplyResult {
    pub schema_version: u8,
    pub status: WarmBootstrapStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<WarmBootstrapRejectReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_revision: Option<String>,
    pub creation_generation: String,
    #[serde(default)]
    pub applied_counts: WarmBootstrapAppliedCounts,
    #[serde(default)]
    pub excluded: Vec<SanitizedExclusion>,
    #[serde(default)]
    pub warnings: Vec<String>,
    pub readable_owner_context: bool,
    pub wake_up_verified: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub general_tab_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<ProviderSignInCardStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_console_ready_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_bootstrap_delta_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_budget: Option<bool>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudCreatePhases {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enrolled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub peer_ready_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_console_ready_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warm_bootstrap_delta_secs: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within_budget: Option<bool>,
    #[serde(default)]
    pub authoritative_backend_phases: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_signin_started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_signin_completed_at_ms: Option<u64>,
}

impl CloudCreatePhases {
    /// Mirrors authoritative backend `last_create_phases` (Unix seconds) onto the local record.
    /// `warm_bootstrap_delta_secs` is derived strictly from backend timestamps, never client clock alone.
    pub fn apply_backend_phases(&mut self, backend: &BackendCreatePhases) {
        self.authoritative_backend_phases = true;
        if let Some(sec) = backend.requested {
            self.requested_at_ms = Some(sec.saturating_mul(1_000));
        }
        if let Some(sec) = backend.running.or(backend.allocated) {
            self.provisioned_at_ms = Some(sec.saturating_mul(1_000));
        }
        if let Some(sec) = backend.enrolled {
            self.enrolled_at_ms = Some(sec.saturating_mul(1_000));
        }
        if let Some(sec) = backend.peer_ready {
            self.peer_ready_at_ms = Some(sec.saturating_mul(1_000));
        }
        if let Some(sec) = backend.seed_started {
            self.seed_started_at_ms = Some(sec.saturating_mul(1_000));
        }
        if let Some(sec) = backend.warm_console_ready {
            self.warm_console_ready_at_ms = Some(sec.saturating_mul(1_000));
        } else {
            self.warm_console_ready_at_ms = None;
        }
        let delta = backend.warm_bootstrap_delta_secs.or_else(|| {
            match (backend.seed_started, backend.warm_console_ready) {
                (Some(start), Some(ready)) if ready >= start => Some((ready - start) as f64),
                _ => None,
            }
        });
        self.warm_bootstrap_delta_secs = delta;
        self.within_budget = delta.map(|secs| secs <= WARM_BOOTSTRAP_BUDGET_SECS);
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CloudWarmBootstrapSummary {
    pub schema_version: u8,
    pub status: WarmBootstrapStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reject_reason: Option<WarmBootstrapRejectReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery_revision: Option<String>,
    pub creation_generation: String,
    pub readable_owner_context: bool,
    pub wake_up_verified: bool,
    #[serde(default)]
    pub applied_counts: WarmBootstrapAppliedCounts,
    #[serde(default)]
    pub excluded: Vec<SanitizedExclusion>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub general_tab_id: Option<String>,
    #[serde(default)]
    pub providers: Vec<ProviderSignInCardStatus>,
}

impl From<&WarmBootstrapApplyResult> for CloudWarmBootstrapSummary {
    fn from(result: &WarmBootstrapApplyResult) -> Self {
        Self {
            schema_version: result.schema_version,
            status: result.status,
            reject_reason: result.reject_reason,
            recovery_revision: result.recovery_revision.clone(),
            creation_generation: result.creation_generation.clone(),
            readable_owner_context: result.readable_owner_context,
            wake_up_verified: result.wake_up_verified,
            applied_counts: result.applied_counts.clone(),
            excluded: result.excluded.clone(),
            warnings: result.warnings.clone(),
            general_tab_id: result.general_tab_id.clone(),
            providers: result.providers.clone(),
        }
    }
}

/// Authoritative backend warm-bootstrap milestone sent to `recordWarmBootstrapPhases`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WarmBootstrapPhaseEvent {
    SeedStarted,
    WarmConsoleReady,
}

/// Authoritative backend `last_create_phases` snapshot (Unix seconds) returned by Firebase.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BackendCreatePhases {
    pub requested: Option<u64>,
    pub allocated: Option<u64>,
    pub ipv4_bound: Option<u64>,
    pub running: Option<u64>,
    pub enrolled: Option<u64>,
    pub peer_ready: Option<u64>,
    pub seed_started: Option<u64>,
    pub warm_console_ready: Option<u64>,
    pub warm_bootstrap_delta_secs: Option<f64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrepareProviderSignInRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareProviderSignInItem {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub display_name: String,
    pub state: ProviderSignInState,
    #[serde(default)]
    pub supported_methods: Vec<String>,
    #[serde(default)]
    pub code_input: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_tab_id: Option<String>,
    pub card_ready: bool,
    pub requires_owner_input: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unsupported_reason: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrepareProviderSignInResponse {
    pub providers: Vec<PrepareProviderSignInItem>,
}
