use super::lane_liveness::{derive_lane_liveness, LaneInputs, LaneLiveness, LaneThresholds};
use super::{Task, TaskStatus};
use crate::service_plan::state::input::{StateSource, StateTaskSource};
use crate::service_plan::state::{task_projection, DependencyWait, TaskPresentation};
use crate::service_plan::{LaneWatchdogRead, LaneWatchdogTaskRead};
use serde::{Deserialize, Serialize};

pub const PENDING_UNSTAFFED_STALE_MS: u64 = 4 * 60 * 60 * 1000;
pub const IN_REVIEW_UNJUDGED_STALE_MS: u64 = 2 * 60 * 60 * 1000;
pub const DEFAULT_STALE_REVIEW_MS: u64 = IN_REVIEW_UNJUDGED_STALE_MS;
pub const NEEDS_REASSIGN_LABEL: &str = "needs-reassign";
pub const STALE_REVIEW_LABEL: &str = "stale-review";
pub const STALE_UNSTAFFED_LABEL: &str = "stale-unstaffed";
pub const TAB_GONE_LABEL: &str = "tab-gone";
pub const TEAM_CLOSED_LABEL: &str = "team-closed";
pub const REPORT_TARGET_WRITE_GRACE_MS: u64 = 30_000;

#[derive(Debug, Clone, PartialEq)]
pub struct AgedOpenTask {
    pub task_id: String,
    pub name: String,
    pub status: TaskStatus,
    pub presentation: TaskPresentation,
    pub dependency_waits: Vec<DependencyWait>,
    pub age_ms: u64,
}

// non-open (terminal/paused) statuses.
pub fn stale_threshold_ms(status: &TaskStatus) -> Option<u64> {
    match status {
        TaskStatus::InReview => Some(IN_REVIEW_UNJUDGED_STALE_MS),
        TaskStatus::Pending | TaskStatus::InProgress | TaskStatus::Blocked => {
            Some(PENDING_UNSTAFFED_STALE_MS)
        }
        _ => None,
    }
}

pub fn active_review_hold_note(task: &Task) -> Option<&str> {
    active_review_hold_note_for(
        &task.status,
        task.status_changed_at,
        task.review_evidence.last(),
    )
}

fn active_review_hold_note_for<'a>(
    status: &TaskStatus,
    status_changed_at: u64,
    latest: Option<&'a crate::plan::ReviewEvidence>,
) -> Option<&'a str> {
    if *status != TaskStatus::InReview {
        return None;
    }
    let latest = latest?;
    (latest.verdict == crate::plan::ReviewVerdict::Hold
        && latest.at >= status_changed_at
        && !latest.note.trim().is_empty())
    .then_some(latest.note.as_str())
}

pub fn has_active_review_hold(task: &impl StateTaskSource) -> bool {
    let task = task.state_task_input();
    active_review_hold_note_for(
        &task.status,
        task.status_changed_at,
        task.review_evidence.last(),
    )
    .is_some()
}

pub fn aged_open_tasks(plan: &impl StateSource, now_ms: u64) -> Vec<AgedOpenTask> {
    let input = plan.state_input();
    let plan = input.as_ref();
    let mut aged: Vec<AgedOpenTask> = plan
        .tasks
        .iter()
        .filter(|task| task.status.counts_as_open_work() && !has_active_review_hold(task))
        .filter_map(|task| {
            let projection = task_projection(plan, task);
            if !projection.can_age() {
                return None;
            }
            let threshold = stale_threshold_ms(&task.status)?;
            if task.status_changed_at == 0 {
                return None;
            }
            let age = now_ms.saturating_sub(task.status_changed_at);
            (age > threshold).then(|| AgedOpenTask {
                task_id: task.id.clone(),
                name: task.name.clone(),
                status: task.status.clone(),
                presentation: projection.presentation,
                dependency_waits: projection.dependency_waits,
                age_ms: age,
            })
        })
        .collect();
    aged.sort_by(|a, b| b.age_ms.cmp(&a.age_ms));
    aged
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabRole {
    Orchestrator,
    Worker,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TabProbe {
    pub exists: bool,
    pub live_agent: bool,
    /// The session which proved it owns this tab through the launch nonce.
    pub agent_session_id: Option<String>,
    /// A nonce-verified session binding exists for the reported session.
    pub session_binding_valid: bool,
    /// Delivery is directed to an agent chat transport, never a bare shell.
    pub chat_delivery_target: bool,
    /// The agent is ready, working, or idle; questions and unknown states do not route work.
    pub agent_accepting_work: bool,
    pub role: Option<TabRole>,
    pub parent_tab_id: Option<String>,
}

impl TabProbe {
    pub fn planner_routable_for(&self, expected_session_id: Option<&str>) -> bool {
        self.live_agent
            && self.session_binding_valid
            && self.chat_delivery_target
            && self.agent_accepting_work
            && expected_session_id
                .is_some_and(|expected| self.agent_session_id.as_deref() == Some(expected))
    }
}

#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    pub now_ms: u64,
    pub stale_review_ms: u64,
    pub current_orchestrator_tab_id: Option<String>,
}

impl ReconcileConfig {
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms,
            stale_review_ms: DEFAULT_STALE_REVIEW_MS,
            current_orchestrator_tab_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    InProgressTabGone,
    StaleInReview,
    ReportTargetWriteMissing,
    StalePendingUnstaffed,
    BoundToClosedTab,
    AcceptedButNotDone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, ts_rs::TS)]
pub struct DriftItem {
    pub task_id: String,
    pub plan_id: String,
    pub kind: DriftKind,
    pub detail: String,
    pub suggested_fix: String,
    #[serde(default)]
    pub not_mine: bool,
}

pub fn detect_plan_drift<F>(
    plan: &impl StateSource,
    config: &ReconcileConfig,
    tab_probe: F,
) -> Vec<DriftItem>
where
    F: Fn(&str) -> TabProbe,
{
    let input = plan.state_input();
    let plan = input.as_ref();
    let mut items = Vec::new();
    for task in &plan.tasks {
        let projection = task_projection(plan, task);
        let assigned_tab = task
            .assigned_tab_id
            .as_deref()
            .map(str::trim)
            .filter(|tab_id| !tab_id.is_empty());
        let tab = if is_terminal_task_status(&task.status) {
            None
        } else {
            assigned_tab.map(|tab_id| (tab_id, tab_probe(tab_id)))
        };
        let not_mine = tab
            .as_ref()
            .is_some_and(|(_, probe)| assigned_to_other_orchestrator(probe, config));

        if task.status == TaskStatus::InProgress {
            match tab.as_ref() {
                None => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressTabGone,
                    "task is in_progress but has no assigned tab",
                    NEEDS_REASSIGN_LABEL,
                    not_mine,
                )),
                Some((tab_id, probe)) if !probe.exists => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressTabGone,
                    format!("task is in_progress but assigned tab {tab_id} is closed"),
                    NEEDS_REASSIGN_LABEL,
                    not_mine,
                )),
                Some((tab_id, probe)) if !probe.live_agent => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressTabGone,
                    format!("task is in_progress but tab {tab_id} has no live agent"),
                    NEEDS_REASSIGN_LABEL,
                    not_mine,
                )),
                _ => {}
            }
        }

        if task.status == TaskStatus::InReview
            && !has_active_review_hold(task)
            && task.status_changed_at > 0
            && config.now_ms.saturating_sub(task.status_changed_at) > config.stale_review_ms
        {
            items.push(item(
                plan,
                task,
                DriftKind::StaleInReview,
                format!(
                    "task has been in_review for {} ms",
                    config.now_ms.saturating_sub(task.status_changed_at)
                ),
                STALE_REVIEW_LABEL,
                not_mine,
            ));
        }

        if task.status == TaskStatus::InReview {
            for attempt in task.execution_attempts.iter().filter(|attempt| {
                attempt.closed_at.is_none()
                    && attempt.report_status.as_deref() == Some("in_review")
                    && attempt.report_target_written_at.is_none()
                    && attempt.reported_at.is_some_and(|reported_at| {
                        config.now_ms.saturating_sub(reported_at) > REPORT_TARGET_WRITE_GRACE_MS
                    })
            }) {
                let target_tab = [
                    attempt.parent_tab_id.as_deref(),
                    attempt.orchestrator_tab_id.as_deref(),
                    plan.owner.tab_id.as_deref(),
                ]
                .into_iter()
                .flatten()
                .map(str::trim)
                .find(|tab_id| !tab_id.is_empty());
                if let Some(target_tab) = target_tab {
                    let age_ms = config
                        .now_ms
                        .saturating_sub(attempt.reported_at.unwrap_or_default());
                    items.push(item(
                        plan,
                        task,
                        DriftKind::ReportTargetWriteMissing,
                        format!(
                            "in_review report for attempt {} was not written to target tab {} after {} ms",
                            attempt.id, target_tab, age_ms
                        ),
                        &format!("deliver the report to tab {target_tab}"),
                        not_mine,
                    ));
                }
            }
        }

        if task.status == TaskStatus::Pending
            && projection.presentation == TaskPresentation::ActionablePending
            && tab.is_none()
            && task.status_changed_at > 0
            && config.now_ms.saturating_sub(task.status_changed_at) > PENDING_UNSTAFFED_STALE_MS
        {
            items.push(item(
                plan,
                task,
                DriftKind::StalePendingUnstaffed,
                format!(
                    "task has been pending and unstaffed for {} ms",
                    config.now_ms.saturating_sub(task.status_changed_at)
                ),
                STALE_UNSTAFFED_LABEL,
                not_mine,
            ));
        }

        if let Some((tab_id, probe)) = tab.as_ref() {
            if !probe.exists {
                items.push(item(
                    plan,
                    task,
                    DriftKind::BoundToClosedTab,
                    format!("task is assigned to closed tab {tab_id}"),
                    "clear dead tab binding",
                    not_mine,
                ));
            }
        }

        if has_unambiguous_acceptance_signal(task) && task.status != TaskStatus::Done {
            items.push(item(
                plan,
                task,
                DriftKind::AcceptedButNotDone,
                "task has an unambiguous acceptance signal but is not done",
                "set status to done",
                not_mine,
            ));
        }
    }
    items
}

pub fn detect_tab_gone(plan: &impl StateSource, tab_id: &str) -> Vec<DriftItem> {
    let input = plan.state_input();
    let plan = input.as_ref();
    let tab_id = tab_id.trim();
    if tab_id.is_empty() {
        return Vec::new();
    }
    plan.tasks
        .iter()
        .filter(|task| !is_terminal_task_status(&task.status))
        .filter(|task| task.assigned_tab_id.as_deref().map(str::trim) == Some(tab_id))
        .map(|task| {
            item(
                plan,
                task,
                DriftKind::BoundToClosedTab,
                format!("assigned tab {tab_id} is gone"),
                TAB_GONE_LABEL,
                false,
            )
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneDemotion {
    pub task_id: String,
    pub plan_id: String,
    pub reason: String,
}

// Freshest known activity for a lane: last stride/report, else when it entered
// in_progress. Zero when no baseline exists (never demote without a clock).
pub fn lane_last_activity_ms(task: &Task) -> u64 {
    task.milestones
        .iter()
        .map(|m| m.ts)
        .chain(task.execution_attempts.iter().filter_map(|a| a.reported_at))
        .max()
        .unwrap_or(0)
        .max(task.status_changed_at)
}

fn lane_inputs(task: &LaneWatchdogTaskRead, probe: Option<&TabProbe>, now_ms: u64) -> LaneInputs {
    LaneInputs {
        has_assigned_tab: probe.is_some(),
        tab_present_local: probe.is_some_and(|p| p.exists),
        has_peer: task.peer.as_deref().is_some_and(|p| !p.trim().is_empty()),
        // Mesh presence unknown here: derive stays conservative (a peer lane
        // absent locally reads Working, never Gone) so a sleeping laptop is safe.
        peer_online: None,
        tracker_active: probe.map(|p| p.live_agent),
        last_activity_ms: task.last_activity_ms,
        now_ms,
    }
}

/// Watchdog demotion pass, meant to run on the reminder-scheduler cadence.
/// Demotes an in_progress lane ONLY when its assigned tab is genuinely gone
/// (absent locally, no peer that could still host it — via derive_lane_liveness)
/// AND it has been silent past the grace window. Never touches ReportOverdue /
/// IdleStalled lanes (honest slow work), tab-less lanes (UI-owned), or lanes
/// owned by another orchestrator. The caller's probe keys `exists` off the
/// lane's OWN session, so a recycled tab id hosting a different agent reads gone.
pub fn detect_lane_demotions<F>(
    plan: &LaneWatchdogRead,
    config: &ReconcileConfig,
    thresholds: &LaneThresholds,
    tab_probe: F,
) -> Vec<LaneDemotion>
where
    F: Fn(&str) -> TabProbe,
{
    plan.tasks
        .iter()
        .filter(|task| task.status == TaskStatus::InProgress)
        .filter_map(|task| {
            let assigned_tab = task
                .assigned_tab_id
                .as_deref()
                .map(str::trim)
                .filter(|tab_id| !tab_id.is_empty());
            let probe = assigned_tab.map(&tab_probe);
            if probe
                .as_ref()
                .is_some_and(|p| assigned_to_other_orchestrator(p, config))
            {
                return None;
            }
            let inputs = lane_inputs(task, probe.as_ref(), config.now_ms);
            if derive_lane_liveness(&inputs, thresholds) != LaneLiveness::TabGone {
                return None;
            }
            if inputs.last_activity_ms == 0 {
                return None;
            }
            let idle = config.now_ms.saturating_sub(inputs.last_activity_ms);
            (idle > thresholds.t_overdue_ms).then(|| LaneDemotion {
                task_id: task.id.clone(),
                plan_id: plan.id.clone(),
                reason: format!(
                    "watchdog: assigned tab gone and silent for {idle}ms — demoted to pending"
                ),
            })
        })
        .collect()
}

fn assigned_to_other_orchestrator(probe: &TabProbe, config: &ReconcileConfig) -> bool {
    if probe.role != Some(TabRole::Worker) {
        return false;
    }
    let Some(parent) = probe.parent_tab_id.as_deref() else {
        return false;
    };
    config
        .current_orchestrator_tab_id
        .as_deref()
        .is_some_and(|mine| parent != mine)
}

fn is_terminal_task_status(status: &TaskStatus) -> bool {
    matches!(status, TaskStatus::Done | TaskStatus::Cancelled)
}

fn has_unambiguous_acceptance_signal(_task: &impl StateTaskSource) -> bool {
    false
}

fn item(
    plan: &impl StateSource,
    task: &impl StateTaskSource,
    kind: DriftKind,
    detail: impl Into<String>,
    suggested_fix: &str,
    not_mine: bool,
) -> DriftItem {
    let plan = plan.state_input();
    let task = task.state_task_input();
    DriftItem {
        task_id: task.id.clone(),
        plan_id: plan.id.clone(),
        kind,
        detail: detail.into(),
        suggested_fix: suggested_fix.to_string(),
        not_mine,
    }
}

#[cfg(test)]
mod tests;
