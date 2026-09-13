//! Mission Control projection over stored plan fields.
//!
//! Every value here is read from a field the plan store already writes: no note
//! prose, no session-log scan, and no provider jsonl. A held review is the stored
//! `review held` label, a bounce is an `execution_attempts` count, an author is a
//! title recorded on the plan record. The ledger-backed KPI and machine-spend blocks
//! stay in their explicit not-recording states for the service layer to fill from
//! the KPI ledger and the usage store.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::plan::{
    AgentRef, DecisionGate, GateStatus, Plan, PlannerRef, PlanningAuthor, Task, TaskStatus,
};

/// The stored facet whose value groups a task into an area.
pub const AREA_FACET_KEY: &str = "area";
/// Written by `review_gate.rs` when a reviewer holds a task for a decision.
pub const HELD_STATUS_LABEL: &str = "review held";
/// Written by `review_gate.rs` when a bounce continuation is delivered.
pub const BOUNCED_STATUS_LABEL: &str = "review bounced";
const REVIEW_MILESTONE_KIND: &str = "review";
const WORKER_REPORT_KIND: &str = "worker_report";
/// Newest collapsed lines kept in the snapshot; `recorded_events` stays exact.
pub const EVENT_LOG_LIMIT: usize = 200;

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct MissionControl {
    pub held: HeldForDecision,
    pub bounces: BounceSummary,
    pub event_log: EventLog,
    pub areas: AreaGrouping,
    pub kpi: MissionControlKpi,
    pub spend: MissionControlSpend,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct HeldForDecision {
    pub held_reviews: Vec<HeldReview>,
    pub open_gates: Vec<OpenDecisionGate>,
    /// Age of the oldest held review, measured from its newest review milestone.
    pub oldest_hold_age_ms: Option<u64>,
    pub verdicts_on_held: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HeldReview {
    pub task_id: String,
    pub name: String,
    pub verdicts: usize,
    pub last_verdict_at: Option<u64>,
    pub age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OpenDecisionGate {
    pub id: String,
    pub question: String,
    pub status: GateStatus,
    pub asked_at: u64,
    pub age_ms: u64,
    pub blocking_task_count: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct BounceSummary {
    pub total_attempts: usize,
    pub tasks_on_second_attempt: usize,
    pub first_try_done: usize,
    pub labelled_bounced: usize,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct EventLog {
    /// Events before collapse and truncation; `entries` may be shorter on both counts.
    pub recorded_events: usize,
    pub truncated: bool,
    pub entries: Vec<PlanEvent>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PlanEvent {
    pub ts: u64,
    /// Last event folded into this line; equals `ts` when `count` is 1.
    pub last_ts: u64,
    pub count: usize,
    pub kind: String,
    pub author: String,
    pub author_tab_id: Option<String>,
    pub task_id: Option<String>,
    pub task_name: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AreaFacetState {
    Declared,
    #[default]
    NoFacet,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct AreaGrouping {
    pub state: AreaFacetState,
    pub facet_key: Option<String>,
    pub facet_label: Option<String>,
    pub groups: Vec<AreaGroup>,
    pub untagged_task_count: usize,
    /// Populated only in the `no_facet` state: grouping by the stored executor role.
    pub role_fallback: Vec<RoleGroup>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AreaGroup {
    pub value: String,
    pub label: String,
    pub task_count: usize,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RoleGroup {
    /// `None` is the stored absence of an executor role, never a guess.
    pub executor_role: Option<String>,
    pub task_count: usize,
    pub task_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KpiLedgerState {
    Recording,
    NotRecording,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum KpiNotRecordingReason {
    /// Someone opted out: `kpi.enabled` is false, so no ledger exists to read.
    Disabled,
    /// The ledger is on but holds no row attributed to this plan.
    NoRowsForPlan,
    /// The ledger could not be read; the snapshot never guesses a figure instead.
    Unreadable,
    /// Built without a ledger reader, which is every plan-crate caller.
    NotRead,
}

/// Carried in the snapshot so a not-recording surface names the real config key
/// rather than a string hand-copied into the UI.
pub use codeterm_types::config::schema::KPI_ENABLED_KEY as KPI_ENABLED_SETTING;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MissionControlKpi {
    pub state: KpiLedgerState,
    pub not_recording_reason: Option<KpiNotRecordingReason>,
    /// `None` whenever `state` is `not_recording` — a missing ledger shows no number.
    pub spend: Option<PlanKpiSpend>,
    /// `Some` exactly when recording is off because of the opt-out, and never for a
    /// state the setting would not fix.
    pub enable_setting: Option<&'static str>,
}

impl Default for MissionControlKpi {
    fn default() -> Self {
        Self::not_recording(KpiNotRecordingReason::NotRead)
    }
}

impl MissionControlKpi {
    pub fn not_recording(reason: KpiNotRecordingReason) -> Self {
        let enable_setting =
            matches!(reason, KpiNotRecordingReason::Disabled).then_some(KPI_ENABLED_SETTING);
        Self {
            state: KpiLedgerState::NotRecording,
            not_recording_reason: Some(reason),
            spend: None,
            enable_setting,
        }
    }

    pub fn recording(spend: PlanKpiSpend) -> Self {
        Self {
            state: KpiLedgerState::Recording,
            not_recording_reason: None,
            spend: Some(spend),
            enable_setting: None,
        }
    }
}

/// Every figure comes from the one `aggregate_by_plan` row for this plan.
#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
pub struct PlanKpiSpend {
    pub events: u64,
    pub bytes: u64,
    pub turns: u64,
    pub tokens: PlanKpiTokens,
}

#[derive(Debug, Clone, Copy, Serialize, Default, PartialEq, Eq)]
pub struct PlanKpiTokens {
    pub input: u64,
    pub uncached_input: u64,
    pub output: u64,
    pub total: u64,
}

/// The only scope this section can honestly claim today: plan attribution reaches
/// task, owner and planner bindings, so a ledger-wide total is a machine total and
/// is labelled one rather than being reported as a plan's spend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SpendScope {
    #[default]
    Machine,
}

/// Measured spend on this machine, grouped by provider and model. Every figure is a
/// sum over `token_usage` rows; the provider percentages beside them are a separate
/// provider/window allowance reading and are never apportioned across models.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MissionControlSpend {
    pub scope: SpendScope,
    pub state: KpiLedgerState,
    pub not_recording_reason: Option<KpiNotRecordingReason>,
    /// `Some` exactly when recording is off because of the opt-out.
    pub enable_setting: Option<&'static str>,
    /// Denominator behind every `token_share_pct`: uncached input over all rows, the
    /// part that costs; cache reads would otherwise swamp every share.
    pub measured_tokens: u64,
    pub turns: u64,
    pub providers: Vec<ProviderSpend>,
}

impl Default for MissionControlSpend {
    fn default() -> Self {
        Self::not_recording(KpiNotRecordingReason::NotRead)
    }
}

impl MissionControlSpend {
    pub fn not_recording(reason: KpiNotRecordingReason) -> Self {
        let enable_setting =
            matches!(reason, KpiNotRecordingReason::Disabled).then_some(KPI_ENABLED_SETTING);
        Self {
            scope: SpendScope::Machine,
            state: KpiLedgerState::NotRecording,
            not_recording_reason: Some(reason),
            enable_setting,
            measured_tokens: 0,
            turns: 0,
            providers: Vec::new(),
        }
    }

    /// Allowance readings survive a missing ledger: they are a real observation
    /// about this machine even when nothing measured is recorded.
    pub fn with_providers(mut self, providers: Vec<ProviderSpend>) -> Self {
        self.providers = providers;
        self
    }

    pub fn recording(measured_tokens: u64, turns: u64, providers: Vec<ProviderSpend>) -> Self {
        Self {
            scope: SpendScope::Machine,
            state: KpiLedgerState::Recording,
            not_recording_reason: None,
            enable_setting: None,
            measured_tokens,
            turns,
            providers,
        }
    }
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ProviderSpend {
    pub provider: String,
    pub label: String,
    /// `None` when this provider reports no usage snapshot at all; a provider that
    /// reports one window only keeps the other window null rather than zero.
    pub allowance: Option<ProviderAllowance>,
    pub turns: u64,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    pub models: Vec<ModelSpend>,
}

/// One provider's latest parsed usage row, read as allowance already consumed.
/// Both shipped parsers store a consumed figure — Claude's `five_hour.utilization`
/// and Codex's `used_percent`, with Codex's text path converting a "left" reading to
/// used before it is stored — so nothing here inverts a percentage. The granularity
/// is the provider and the window; no per-model denominator exists in either source.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ProviderAllowance {
    pub captured_at: i64,
    pub session_used_pct: Option<f32>,
    pub session_resets_at: Option<i64>,
    pub weekly_used_pct: Option<f32>,
    pub weekly_resets_at: Option<i64>,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct ModelSpend {
    /// `None` is the ledger's unresolved model identity, carried as its own visible
    /// row. It is never guessed from the provider and never dropped.
    pub model: Option<String>,
    pub turns: u64,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    /// Share of the machine's measured token total. Never a share of allowance.
    pub token_share_pct: f32,
}

pub fn build_mission_control(plan: &Plan, now_ms: u64) -> MissionControl {
    MissionControl {
        held: held_for_decision(plan, now_ms),
        bounces: bounce_summary(plan),
        event_log: event_log(plan),
        areas: area_grouping(plan),
        kpi: MissionControlKpi::default(),
        spend: MissionControlSpend::default(),
    }
}

fn review_verdicts(task: &Task) -> (usize, Option<u64>) {
    let stamps: Vec<u64> = task
        .milestones
        .iter()
        .filter(|milestone| milestone.kind == REVIEW_MILESTONE_KIND)
        .map(|milestone| milestone.ts)
        .collect();
    (stamps.len(), stamps.iter().copied().max())
}

fn held_for_decision(plan: &Plan, now_ms: u64) -> HeldForDecision {
    let held_reviews: Vec<HeldReview> = plan
        .tasks
        .iter()
        .filter(|task| task.status_label.as_deref() == Some(HELD_STATUS_LABEL))
        .map(|task| {
            let (verdicts, last_verdict_at) = review_verdicts(task);
            HeldReview {
                task_id: task.id.clone(),
                name: task.name.clone(),
                verdicts,
                last_verdict_at,
                age_ms: last_verdict_at.map(|at| now_ms.saturating_sub(at)),
            }
        })
        .collect();
    let open_gates: Vec<OpenDecisionGate> = plan
        .gates
        .iter()
        .filter(|gate: &&DecisionGate| gate.is_open())
        .map(|gate| OpenDecisionGate {
            id: gate.id.clone(),
            question: gate.question.clone(),
            status: gate.status.clone(),
            asked_at: gate.asked_at,
            age_ms: now_ms.saturating_sub(gate.asked_at),
            blocking_task_count: gate.blocking_task_ids.len(),
        })
        .collect();
    HeldForDecision {
        oldest_hold_age_ms: held_reviews.iter().filter_map(|held| held.age_ms).max(),
        verdicts_on_held: held_reviews.iter().map(|held| held.verdicts).sum(),
        held_reviews,
        open_gates,
    }
}

fn bounce_summary(plan: &Plan) -> BounceSummary {
    let mut summary = BounceSummary::default();
    for task in &plan.tasks {
        let attempts = task.execution_attempts.len();
        summary.total_attempts += attempts;
        if attempts >= 2 {
            summary.tasks_on_second_attempt += 1;
        }
        if task.status == TaskStatus::Done && attempts == 1 {
            summary.first_try_done += 1;
        }
        if task.status_label.as_deref() == Some(BOUNCED_STATUS_LABEL) {
            summary.labelled_bounced += 1;
        }
    }
    summary
}

/// Titles recorded on the plan record itself: the owner, the planner, and each
/// attempt's executor. A tab with no recorded title keeps its tab id.
fn author_titles(plan: &Plan) -> BTreeMap<String, String> {
    let mut titles: BTreeMap<String, String> = BTreeMap::new();
    if let (Some(tab_id), Some(title)) = (plan.owner.tab_id.as_ref(), plan.owner.title.as_ref()) {
        titles.insert(tab_id.clone(), title.clone());
    }
    if let Some(PlannerRef::Tab {
        tab_id,
        title: Some(title),
        ..
    }) = &plan.planner
    {
        titles.insert(tab_id.clone(), title.clone());
    }
    for task in &plan.tasks {
        for attempt in &task.execution_attempts {
            if let Some(title) = &attempt.executor_title {
                titles.insert(attempt.executor_tab_id.clone(), title.clone());
            }
        }
    }
    titles
}

fn first_line(text: &str) -> String {
    text.trim().lines().next().unwrap_or("").trim().to_string()
}

fn resolve_author(titles: &BTreeMap<String, String>, from: &AgentRef) -> (String, Option<String>) {
    let tab_id = from.tab_id.clone();
    let author = tab_id
        .as_ref()
        .and_then(|tab| titles.get(tab).cloned())
        .or_else(|| from.title.clone())
        .or_else(|| tab_id.clone())
        .unwrap_or_default();
    (author, tab_id)
}

fn planning_events(plan: &Plan, out: &mut Vec<PlanEvent>) {
    for transition in &plan.planning_log {
        out.push(PlanEvent {
            ts: transition.at,
            last_ts: transition.at,
            count: 1,
            kind: "planning".into(),
            author: transition.by.label(),
            author_tab_id: match &transition.by {
                PlanningAuthor::Planner { tab_id } => Some(tab_id.clone()),
                PlanningAuthor::User => None,
            },
            task_id: None,
            task_name: None,
            text: format!("{} to {}", transition.from.as_str(), transition.to.as_str()),
        });
    }
}

fn task_events(plan: &Plan, titles: &BTreeMap<String, String>, out: &mut Vec<PlanEvent>) {
    for task in &plan.tasks {
        out.push(PlanEvent {
            ts: task.created_at,
            last_ts: task.created_at,
            count: 1,
            kind: "task_created".into(),
            author: String::new(),
            author_tab_id: None,
            task_id: Some(task.id.clone()),
            task_name: Some(task.name.clone()),
            text: task.name.clone(),
        });
        for milestone in &task.milestones {
            let (author, author_tab_id) = resolve_author(titles, &milestone.from);
            out.push(PlanEvent {
                ts: milestone.ts,
                last_ts: milestone.ts,
                count: 1,
                kind: milestone.kind.clone(),
                author,
                author_tab_id,
                task_id: Some(task.id.clone()),
                task_name: Some(task.name.clone()),
                text: first_line(&milestone.text),
            });
        }
    }
}

fn stride_and_gate_events(
    plan: &Plan,
    titles: &BTreeMap<String, String>,
    out: &mut Vec<PlanEvent>,
) {
    for stride in &plan.strides {
        let (author, author_tab_id) = resolve_author(titles, &stride.from);
        out.push(PlanEvent {
            ts: stride.ts,
            last_ts: stride.ts,
            count: 1,
            kind: stride.kind.clone(),
            author,
            author_tab_id,
            task_id: None,
            task_name: None,
            text: first_line(&stride.text),
        });
    }
    for gate in &plan.gates {
        let author = gate
            .asked_by_tab_id
            .as_ref()
            .and_then(|tab| titles.get(tab).cloned())
            .or_else(|| gate.asked_by_tab_id.clone())
            .unwrap_or_default();
        out.push(PlanEvent {
            ts: gate.asked_at,
            last_ts: gate.asked_at,
            count: 1,
            kind: "gate".into(),
            author,
            author_tab_id: gate.asked_by_tab_id.clone(),
            task_id: None,
            task_name: None,
            text: first_line(&gate.question),
        });
    }
}

fn event_log(plan: &Plan) -> EventLog {
    let titles = author_titles(plan);
    let mut raw: Vec<PlanEvent> = Vec::new();
    planning_events(plan, &mut raw);
    task_events(plan, &titles, &mut raw);
    stride_and_gate_events(plan, &titles, &mut raw);

    let recorded_events = raw.len();
    raw.sort_by_key(|event| event.ts);

    let mut entries: Vec<PlanEvent> = Vec::with_capacity(raw.len());
    for event in raw {
        let folds_into_previous = entries.last().is_some_and(|previous| {
            previous.kind == WORKER_REPORT_KIND
                && event.kind == WORKER_REPORT_KIND
                && previous.author == event.author
                && previous.task_id == event.task_id
        });
        if folds_into_previous {
            if let Some(previous) = entries.last_mut() {
                previous.count += 1;
                previous.last_ts = event.ts;
            }
            continue;
        }
        entries.push(event);
    }

    let overflow = entries.len().saturating_sub(EVENT_LOG_LIMIT);
    let truncated = overflow > 0;
    if truncated {
        entries.drain(..overflow);
    }
    EventLog {
        recorded_events,
        truncated,
        entries,
    }
}

fn facet_value<'a>(tags: &'a [String], key: &str) -> Option<&'a str> {
    tags.iter().find_map(|tag| {
        let (tag_key, value) = tag.split_once(':')?;
        (tag_key == key && !value.is_empty()).then_some(value)
    })
}

fn area_grouping(plan: &Plan) -> AreaGrouping {
    let Some(facet) = plan
        .tag_facets
        .iter()
        .find(|facet| facet.key == AREA_FACET_KEY)
    else {
        return role_fallback_grouping(plan);
    };
    let mut groups: Vec<AreaGroup> = facet
        .values
        .iter()
        .map(|value| AreaGroup {
            value: value.v.clone(),
            label: value.label.clone().unwrap_or_else(|| value.v.clone()),
            task_count: 0,
            task_ids: Vec::new(),
        })
        .collect();
    let mut untagged_task_count = 0;
    for task in &plan.tasks {
        match facet_value(&task.tags, &facet.key)
            .and_then(|value| groups.iter_mut().find(|group| group.value == value))
        {
            Some(group) => {
                group.task_count += 1;
                group.task_ids.push(task.id.clone());
            }
            None => untagged_task_count += 1,
        }
    }
    AreaGrouping {
        state: AreaFacetState::Declared,
        facet_key: Some(facet.key.clone()),
        facet_label: facet.label.clone(),
        groups,
        untagged_task_count,
        role_fallback: Vec::new(),
    }
}

fn role_fallback_grouping(plan: &Plan) -> AreaGrouping {
    let mut by_role: BTreeMap<Option<String>, RoleGroup> = BTreeMap::new();
    for task in &plan.tasks {
        let entry = by_role
            .entry(task.executor_role.clone())
            .or_insert_with(|| RoleGroup {
                executor_role: task.executor_role.clone(),
                task_count: 0,
                task_ids: Vec::new(),
            });
        entry.task_count += 1;
        entry.task_ids.push(task.id.clone());
    }
    AreaGrouping {
        state: AreaFacetState::NoFacet,
        facet_key: None,
        facet_label: None,
        groups: Vec::new(),
        untagged_task_count: plan.tasks.len(),
        role_fallback: by_role.into_values().collect(),
    }
}
