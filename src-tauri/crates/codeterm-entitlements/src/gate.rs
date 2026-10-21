use crate::activation::{Activation, Phase};
use crate::payload::Limits;
use crate::verdict::{compiled_free, EffectiveAccess};
use std::fmt;

/// Stable refusal code agents match on instead of retrying.
pub const FREE_LIMIT_REACHED: &str = "free_limit_reached";

/// Limits enforced at creation points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LimitKey {
    Workspaces,
    Teams,
    AgentTabs,
}

impl LimitKey {
    pub fn key(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Teams => "teams",
            Self::AgentTabs => "agent.tabs",
        }
    }

    pub fn max(self, limits: &Limits) -> Option<u64> {
        match self {
            Self::Workspaces => limits.workspaces.max,
            Self::Teams => limits.teams.max,
            Self::AgentTabs => limits.agent_tabs.max,
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Self::Workspaces => "workspaces",
            Self::Teams => "Teams across all workspaces",
            Self::AgentTabs => "open tabs at once",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LimitRefusal {
    pub limit: LimitKey,
    pub count: u64,
    pub max: u64,
}

impl fmt::Display for LimitRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{FREE_LIMIT_REACHED}: Sorry, the free version allows at most {} {} (you have {}). Upgrade to remove this limit. (limit: {})",
            self.max,
            self.limit.noun(),
            self.count,
            self.limit.key()
        )
    }
}

impl std::error::Error for LimitRefusal {}

/// Only a Phase B activation with enforcement on makes limits binding.
pub fn enforcing(activation: Option<&Activation>) -> bool {
    activation
        .is_some_and(|activation| activation.phase == Phase::B && activation.enforcement.enabled)
}

/// What a client needs to pre-check creation; the gate's own refusal stays the authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct LimitSummary {
    pub enforcing: bool,
    pub workspaces: Option<u64>,
    pub teams: Option<u64>,
    #[serde(rename = "agent.tabs")]
    pub agent_tabs: Option<u64>,
}

/// One admission decision source, captured before callers take their own locks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LimitGate {
    access: EffectiveAccess,
    enforcing: bool,
}

impl LimitGate {
    pub fn new(access: EffectiveAccess, enforcing: bool) -> Self {
        Self { access, enforcing }
    }

    pub fn dormant() -> Self {
        Self::new(compiled_free(), false)
    }

    pub fn admit(&self, limit: LimitKey, count: usize) -> Result<(), LimitRefusal> {
        admit(limit, count as u64, &self.access, self.enforcing)
    }

    pub fn summary(&self) -> LimitSummary {
        let limits = &self.access.limits;
        LimitSummary {
            enforcing: self.enforcing,
            workspaces: LimitKey::Workspaces.max(limits),
            teams: LimitKey::Teams.max(limits),
            agent_tabs: LimitKey::AgentTabs.max(limits),
        }
    }
}

/// Admits one more item when enforcement is off, the limit is unlimited, or `count` is below it.
pub fn admit(
    limit: LimitKey,
    count: u64,
    access: &EffectiveAccess,
    enforcing: bool,
) -> Result<(), LimitRefusal> {
    match limit.max(&access.limits) {
        Some(max) if enforcing && count >= max => Err(LimitRefusal { limit, count, max }),
        _ => Ok(()),
    }
}
