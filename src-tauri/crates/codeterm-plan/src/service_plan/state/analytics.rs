//! Machine-wide KPI analytics: the provider-neutral query a plugin sends, the
//! snapshot it receives, and the pure fold from bounded ledger aggregates.

mod project;

use std::collections::BTreeMap;
use std::sync::Arc;

use chrono::{Duration, NaiveDate};
use serde::{Deserialize, Serialize};

use super::mission_control::ProviderAllowance;

pub use project::{project_snapshot, ProjectionInput};

pub const MAX_RANGE_DAYS: i64 = 92;
pub const MAX_DAY_BUCKET_DAYS: i64 = 31;
pub const MAX_FILTER_KEYS: usize = 64;
pub const MAX_ID_CHARS: usize = 256;
pub const MACHINE_SCOPE: &str = "machine";
const WEEK_DAYS: i64 = 7;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsQuery {
    pub range: AnalyticsRange,
    pub bucket: AnalyticsBucket,
    #[serde(default)]
    pub filters: AnalyticsFilters,
    pub group_by: AnalyticsGroupBy,
    #[serde(default)]
    pub scope: Option<AnalyticsScopeRequest>,
}

/// Local calendar days, both ends inclusive, in the caller's IANA time zone.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsRange {
    pub from: String,
    pub to: String,
    pub tz: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsBucket {
    Day,
    Week,
}

/// Choices inside one list widen the selection; lists narrow each other. An empty
/// list selects nothing extra and reads as no filter. `null` selects the null key.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsFilters {
    #[serde(default)]
    pub providers: Option<Vec<String>>,
    #[serde(default)]
    pub models: Option<Vec<ModelKey>>,
    #[serde(default)]
    pub workspaces: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub teams: Option<Vec<Option<String>>>,
    #[serde(default)]
    pub plans: Option<Vec<Option<String>>>,
}

impl AnalyticsFilters {
    /// Tab creation carries only a workspace, so any other filter cannot apply to it.
    pub fn narrows_beyond_workspace(&self) -> bool {
        active(&self.providers)
            || active(&self.models)
            || active(&self.teams)
            || active(&self.plans)
    }
}

fn active<T>(values: &Option<Vec<T>>) -> bool {
    values.as_ref().is_some_and(|values| !values.is_empty())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct ModelKey {
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsGroupBy {
    ProviderModel,
    Workspace,
    Team,
    Plan,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AnalyticsScopeRequest {
    pub kind: String,
    #[serde(default)]
    pub includes_mesh_peers: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnalyticsRefusalKind {
    InvalidQuery,
    /// The query names a scope this machine does not hold: a mesh rollup, or a
    /// workspace, team or plan id it never saw.
    ForeignScope,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnalyticsRefusal {
    pub error: AnalyticsRefusalKind,
    pub message: String,
}

impl AnalyticsRefusal {
    pub fn invalid(message: impl Into<String>) -> Self {
        Self {
            error: AnalyticsRefusalKind::InvalidQuery,
            message: message.into(),
        }
    }

    pub fn foreign(message: impl Into<String>) -> Self {
        Self {
            error: AnalyticsRefusalKind::ForeignScope,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalDays {
    pub from: NaiveDate,
    pub to: NaiveDate,
}

/// The time zone is resolved by the host, which owns the zone database.
pub fn validate_query(query: &AnalyticsQuery) -> Result<LocalDays, AnalyticsRefusal> {
    if let Some(scope) = &query.scope {
        if scope.kind != MACHINE_SCOPE || scope.includes_mesh_peers {
            return Err(AnalyticsRefusal::foreign(
                "analytics reads this machine only; other machines' ledgers are not read",
            ));
        }
    }
    let from = parse_day(&query.range.from, "range.from")?;
    let to = parse_day(&query.range.to, "range.to")?;
    if to < from {
        return Err(AnalyticsRefusal::invalid(
            "range.to must not be earlier than range.from",
        ));
    }
    let days = to.signed_duration_since(from).num_days() + 1;
    if days > MAX_RANGE_DAYS {
        return Err(AnalyticsRefusal::invalid(format!(
            "range must not exceed {MAX_RANGE_DAYS} days"
        )));
    }
    if query.bucket == AnalyticsBucket::Day && days > MAX_DAY_BUCKET_DAYS {
        return Err(AnalyticsRefusal::invalid(format!(
            "day buckets cover at most {MAX_DAY_BUCKET_DAYS} days; use week"
        )));
    }
    if query.range.tz.trim().is_empty() {
        return Err(AnalyticsRefusal::invalid("range.tz is required"));
    }
    validate_filters(&query.filters)?;
    Ok(LocalDays { from, to })
}

fn parse_day(value: &str, field: &str) -> Result<NaiveDate, AnalyticsRefusal> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| AnalyticsRefusal::invalid(format!("{field} must use YYYY-MM-DD format")))
}

fn validate_filters(filters: &AnalyticsFilters) -> Result<(), AnalyticsRefusal> {
    if let Some(providers) = &filters.providers {
        check_len(providers.len(), "filters.providers")?;
        for provider in providers {
            check_id(provider, "filters.providers")?;
        }
    }
    if let Some(models) = &filters.models {
        check_len(models.len(), "filters.models")?;
        for key in models {
            check_id(&key.provider, "filters.models.provider")?;
            if let Some(model) = &key.model {
                check_id(model, "filters.models.model")?;
            }
        }
    }
    for (values, field) in [
        (&filters.workspaces, "filters.workspaces"),
        (&filters.teams, "filters.teams"),
        (&filters.plans, "filters.plans"),
    ] {
        if let Some(values) = values {
            check_len(values.len(), field)?;
            for id in values.iter().flatten() {
                check_id(id, field)?;
            }
        }
    }
    Ok(())
}

fn check_len(len: usize, field: &str) -> Result<(), AnalyticsRefusal> {
    if len > MAX_FILTER_KEYS {
        return Err(AnalyticsRefusal::invalid(format!(
            "{field} must not list more than {MAX_FILTER_KEYS} keys"
        )));
    }
    Ok(())
}

fn check_id(value: &str, field: &str) -> Result<(), AnalyticsRefusal> {
    if value.trim().is_empty() || value != value.trim() {
        return Err(AnalyticsRefusal::invalid(format!(
            "{field} ids must be non-empty without surrounding whitespace"
        )));
    }
    if value.chars().count() > MAX_ID_CHARS {
        return Err(AnalyticsRefusal::invalid(format!(
            "{field} ids must not exceed {MAX_ID_CHARS} characters"
        )));
    }
    Ok(())
}

/// `end_day` is exclusive, so consecutive buckets share a boundary and never overlap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalBucket {
    pub first_day: NaiveDate,
    pub end_day: NaiveDate,
}

/// Weeks run in seven-day steps from `from`; the last one ends with the range.
pub fn local_buckets(days: LocalDays, bucket: AnalyticsBucket) -> Vec<LocalBucket> {
    let step = match bucket {
        AnalyticsBucket::Day => 1,
        AnalyticsBucket::Week => WEEK_DAYS,
    };
    let end = days.to + Duration::days(1);
    let mut out = Vec::new();
    let mut start = days.from;
    while start < end {
        let next = (start + Duration::days(step)).min(end);
        out.push(LocalBucket {
            first_day: start,
            end_day: next,
        });
        start = next;
    }
    out
}

/// One bucket in epoch seconds, `end` exclusive, labelled with its first local day.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BucketBound {
    pub start: u64,
    pub end: u64,
    pub label: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TabCreatedKind {
    Agent,
    Terminal,
}

impl TabCreatedKind {
    pub fn label(self) -> &'static str {
        match self {
            TabCreatedKind::Agent => "agent",
            TabCreatedKind::Terminal => "terminal",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "agent" => Some(TabCreatedKind::Agent),
            "terminal" => Some(TabCreatedKind::Terminal),
            _ => None,
        }
    }
}

/// One grouped `token_usage` sum. Every `Option` is the ledger's own stored absence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenAggregateRow {
    pub bucket: usize,
    pub provider: String,
    pub model: Option<String>,
    pub workspace_id: Option<String>,
    pub plan_id: Option<String>,
    pub turns: u64,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TabCreatedAggregateRow {
    pub bucket: usize,
    pub tab_kind: TabCreatedKind,
    pub workspace_id: Option<String>,
    pub count: u64,
}

/// `workspace_recorded_since` and `tabs_counted_since` date the ledger upgrade that
/// began recording each figure; nothing earlier is back-filled.
#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct LedgerCoverage {
    pub first_recorded_at: Option<u64>,
    pub last_recorded_at: Option<u64>,
    pub workspace_recorded_since: Option<u64>,
    pub tabs_counted_since: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LedgerAggregates {
    pub coverage: LedgerCoverage,
    pub tokens: Vec<TokenAggregateRow>,
    pub tabs: Vec<TabCreatedAggregateRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerRead {
    /// The owner opted out with `kpi.enabled`; no ledger is open.
    Disabled,
    /// Recording is on but the ledger could not be opened or read.
    Unavailable {
        reason: String,
    },
    Recorded(Arc<LedgerAggregates>),
}

/// Names the host already knows, keyed by the ids the ledger stores.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ScopeDirectory {
    pub providers: BTreeMap<String, String>,
    pub workspaces: BTreeMap<String, String>,
    pub teams: BTreeMap<String, Option<String>>,
    pub plans: BTreeMap<String, DirectoryPlan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPlan {
    pub title: String,
    pub team_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalyticsSnapshot {
    pub scope: AnalyticsScope,
    pub as_of: u64,
    pub range: AnalyticsRange,
    pub bucket: AnalyticsBucket,
    pub group_by: AnalyticsGroupBy,
    pub recording: RecordingBlock,
    pub tokens: TokenBlock,
    pub tabs_created: TabsCreatedBlock,
    pub allowance: Vec<AllowanceEntry>,
    pub money: MoneyBlock,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AnalyticsScope {
    pub kind: &'static str,
    pub includes_mesh_peers: bool,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RecordingState {
    Recording,
    Disabled,
    Unavailable,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RecordingBlock {
    pub state: RecordingState,
    /// Verbatim host text, present only when the ledger is unavailable.
    pub reason: Option<String>,
    /// `Some` exactly when recording is off because of the opt-out.
    pub enable_setting: Option<&'static str>,
    pub first_recorded_at: Option<u64>,
    pub last_recorded_at: Option<u64>,
    pub workspace_recorded_since: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct Totals {
    pub new_input: u64,
    pub generated: u64,
    pub read_from_cache: u64,
    pub turns: u64,
}

impl Totals {
    fn add_row(&mut self, row: &TokenAggregateRow) {
        self.new_input = self.new_input.saturating_add(row.uncached_input_tokens);
        self.generated = self.generated.saturating_add(row.output_tokens);
        self.read_from_cache = self
            .read_from_cache
            .saturating_add(row.input_tokens.saturating_sub(row.uncached_input_tokens));
        self.turns = self.turns.saturating_add(row.turns);
    }

    /// Cache reads would swamp every ordering, so rank by what was freshly processed.
    fn weight(&self) -> u64 {
        self.new_input.saturating_add(self.generated)
    }
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    Recorded,
    Partial,
    NotRecorded,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TokenBlock {
    /// `None` when no bucket in range was recorded; a missing figure is never zero.
    pub totals: Option<Totals>,
    pub series: Vec<TokenBucket>,
    pub groups: Vec<GroupRow>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TokenBucket {
    pub start: u64,
    pub end: u64,
    pub label: String,
    pub coverage: Coverage,
    pub totals: Option<Totals>,
    pub groups: Vec<GroupTotals>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GroupTotals {
    pub key: GroupKey,
    pub totals: Totals,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct GroupRow {
    pub key: GroupKey,
    /// The name the host holds for this id; `None` when it holds none.
    pub label: Option<String>,
    pub totals: Totals,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<GroupRow>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Dimension {
    Provider,
    Model,
    Workspace,
    Team,
    Plan,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum KeyReason {
    ProviderNotReported,
    ModelNotReported,
    WorkspaceNotRecorded,
    WorkspaceDeleted,
    NoPlan,
    PlanDeleted,
    NoTeam,
    TeamDeleted,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct GroupKey {
    pub dimension: Dimension,
    pub id: Option<String>,
    /// Set on model keys only: a model id is scoped to the provider that reported it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Why the id is null, or why a present id has no record behind it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<KeyReason>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TabsCreatedState {
    Counted,
    NotCounted,
    /// A filter other than workspace is active, and tab creation carries no such key.
    NotAttributed,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TabsCreatedBlock {
    pub state: TabsCreatedState,
    pub counted_since: Option<u64>,
    pub total: Option<u64>,
    pub by_kind: Option<TabKindCounts>,
    pub series: Vec<TabBucket>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, PartialEq, Eq)]
pub struct TabKindCounts {
    pub agent: u64,
    pub terminal: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TabBucket {
    pub start: u64,
    pub end: u64,
    pub label: String,
    pub coverage: Coverage,
    pub total: Option<u64>,
}

/// Allowance is reported per provider account and is never split across models,
/// workspaces, teams or plans.
#[derive(Debug, Clone, Serialize)]
pub struct AllowanceEntry {
    pub provider: String,
    pub allowance: ProviderAllowance,
    pub attribution: AllowanceAttribution,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct AllowanceAttribution {
    pub model: &'static str,
    pub workspace: &'static str,
    pub team: &'static str,
    pub plan: &'static str,
}

impl AllowanceAttribution {
    pub const UNAVAILABLE: Self = Self {
        model: "unavailable",
        workspace: "unavailable",
        team: "unavailable",
        plan: "unavailable",
    };
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub struct MoneyBlock {
    pub state: &'static str,
}

impl MoneyBlock {
    pub const NOT_RECORDED: Self = Self {
        state: "not_recorded",
    };
}

#[cfg(test)]
mod tests;
