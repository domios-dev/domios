use crate::payload::Limits;
use crate::verdict::{compiled_free, EffectiveAccess};
use std::collections::BTreeSet;
use std::fmt;

/// Stable refusal code agents match on instead of retrying.
pub const FREE_LIMIT_REACHED: &str = "free_limit_reached";
/// Stable refusal code for acting on an item locked above the plan limit.
pub const PLAN_LIMIT_LOCKED: &str = "plan_limit_locked";

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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockedRefusal {
    pub limit: LimitKey,
    pub item_id: String,
}

impl fmt::Display for LockedRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{PLAN_LIMIT_LOCKED}: {} is locked because it is above your plan's limit of {}. Upgrade or close newer items to unlock it. (limit: {})",
            self.item_id,
            self.limit.noun(),
            self.limit.key()
        )
    }
}

impl std::error::Error for LockedRefusal {}

/// One item competing for a limited slot; `exempt` items (General) never count nor lock.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LockCandidate {
    pub id: String,
    pub created_at: u64,
    pub exempt: bool,
}

/// Ids marked locked; marking never removes an item from any listing.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(transparent)]
pub struct LockSet(BTreeSet<String>);

impl LockSet {
    pub fn contains(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
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
    valid_until: Option<u64>,
    clock: Option<GateClock>,
}

#[derive(Clone)]
struct GateClock(
    std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
    std::sync::Arc<std::sync::atomic::AtomicU64>,
);

impl GateClock {
    fn now(&self) -> u64 {
        let now = (self.0)();
        self.1
            .fetch_max(now, std::sync::atomic::Ordering::SeqCst)
            .max(now)
    }
}

impl std::fmt::Debug for GateClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GateClock")
    }
}

impl PartialEq for GateClock {
    fn eq(&self, other: &Self) -> bool {
        std::sync::Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for GateClock {}

impl LimitGate {
    pub fn new(access: EffectiveAccess, enforcing: bool) -> Self {
        Self {
            access,
            enforcing,
            valid_until: None,
            clock: None,
        }
    }

    pub fn with_deadline(
        mut self,
        valid_until: Option<u64>,
        clock: std::sync::Arc<dyn Fn() -> u64 + Send + Sync>,
    ) -> Self {
        self.clock = Some(GateClock(
            clock,
            std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
        ));
        self.valid_until = valid_until;
        self
    }

    fn access_at(&self, now: u64) -> std::borrow::Cow<'_, EffectiveAccess> {
        if self.valid_until.is_some_and(|end| now >= end) {
            std::borrow::Cow::Owned(compiled_free())
        } else {
            std::borrow::Cow::Borrowed(&self.access)
        }
    }

    fn current_access(&self) -> std::borrow::Cow<'_, EffectiveAccess> {
        self.access_at(self.clock.as_ref().map_or_else(
            || {
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            },
            GateClock::now,
        ))
    }

    pub fn dormant() -> Self {
        Self::new(compiled_free(), false)
    }

    pub fn admit(&self, limit: LimitKey, count: usize) -> Result<(), LimitRefusal> {
        admit(limit, count as u64, &self.current_access(), self.enforcing)
    }

    /// Oldest `max` non-exempt items stay usable (created_at, then id); the rest are locked.
    pub fn over_limit(&self, limit: LimitKey, items: &[LockCandidate]) -> LockSet {
        let Some(max) = limit
            .max(&self.current_access().limits)
            .filter(|_| self.enforcing)
        else {
            return LockSet::default();
        };
        let mut counted: Vec<&LockCandidate> = items.iter().filter(|item| !item.exempt).collect();
        counted.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
        let keep = usize::try_from(max).unwrap_or(usize::MAX);
        LockSet(
            counted
                .into_iter()
                .skip(keep)
                .map(|item| item.id.clone())
                .collect(),
        )
    }

    pub fn summary(&self) -> LimitSummary {
        let limits = &self.current_access().limits;
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
