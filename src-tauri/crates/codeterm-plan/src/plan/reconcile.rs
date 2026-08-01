use super::lane_liveness::{derive_lane_liveness, LaneInputs, LaneLiveness, LaneThresholds};
use super::{Plan, Task, TaskStatus};
use crate::service_plan::state::{task_projection, DependencyWait, TaskPresentation};
use serde::{Deserialize, Serialize};

pub const PENDING_UNSTAFFED_STALE_MS: u64 = 4 * 60 * 60 * 1000;
pub const IN_REVIEW_UNJUDGED_STALE_MS: u64 = 2 * 60 * 60 * 1000;
pub const DEFAULT_STALE_REVIEW_MS: u64 = IN_REVIEW_UNJUDGED_STALE_MS;
pub const NEEDS_REASSIGN_LABEL: &str = "needs-reassign";
pub const STALE_REVIEW_LABEL: &str = "stale-review";
pub const STALE_UNSTAFFED_LABEL: &str = "stale-unstaffed";
pub const TAB_GONE_LABEL: &str = "tab-gone";

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

// Open tasks that have idled past their status threshold, newest drift first.
pub fn aged_open_tasks(plan: &Plan, now_ms: u64) -> Vec<AgedOpenTask> {
    let mut aged: Vec<AgedOpenTask> = plan
        .tasks
        .iter()
        .filter(|task| task.status.counts_as_open_work())
        .filter_map(|task| {
            let projection = task_projection(plan, task);
            if projection.presentation == TaskPresentation::WaitingOnDependencies {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    InProgressTabGone,
    StaleInReview,
    StalePendingUnstaffed,
    BoundToClosedTab,
    AcceptedButNotDone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DriftItem {
    pub task_id: String,
    pub plan_id: String,
    pub kind: DriftKind,
    pub detail: String,
    pub suggested_fix: String,
    #[serde(default)]
    pub not_mine: bool,
}

pub fn detect_plan_drift<F>(plan: &Plan, config: &ReconcileConfig, tab_probe: F) -> Vec<DriftItem>
where
    F: Fn(&str) -> TabProbe,
{
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

pub fn detect_tab_gone(plan: &Plan, tab_id: &str) -> Vec<DriftItem> {
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

fn lane_inputs(task: &Task, probe: Option<&TabProbe>, now_ms: u64) -> LaneInputs {
    LaneInputs {
        has_assigned_tab: probe.is_some(),
        tab_present_local: probe.is_some_and(|p| p.exists),
        has_peer: task.peer.as_deref().is_some_and(|p| !p.trim().is_empty()),
        // Mesh presence unknown here: derive stays conservative (a peer lane
        // absent locally reads Working, never Gone) so a sleeping laptop is safe.
        peer_online: None,
        tracker_active: probe.map(|p| p.live_agent),
        last_activity_ms: lane_last_activity_ms(task),
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
    plan: &Plan,
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

fn has_unambiguous_acceptance_signal(_task: &Task) -> bool {
    false
}

fn item(
    plan: &Plan,
    task: &Task,
    kind: DriftKind,
    detail: impl Into<String>,
    suggested_fix: &str,
    not_mine: bool,
) -> DriftItem {
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
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan, Task, TaskPriority, TaskStatus};

    fn task(id: &str, status: TaskStatus) -> Task {
        Task {
            id: id.into(),
            name: id.into(),
            kind: "implement".into(),
            model: None,
            model_rationale: None,
            acceptance_criteria: vec![],
            external_refs: vec![],
            test_plan: None,
            qa_plan: None,
            correctness: None,
            status,
            review_stage: Default::default(),
            qa_state: None,
            review_round: 0,
            review_oracle: None,
            review_evidence: vec![],
            review_override: None,
            review_audit: vec![],
            status_label: None,
            priority: TaskPriority::P2,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            role_guidance: None,
            executor_role: None,
            competition: None,
            intent: None,
            parent_task_id: None,
            depends_on: vec![],
            assigned_tab_id: None,
            lane: Default::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
            execution_attempts: vec![],
            pause_reason: None,
            personality: None,
            personality_provenance: None,
            attachments: vec![],
        }
    }

    fn plan(tasks: Vec<Task>) -> Plan {
        let mut plan = Plan::new(
            "p".into(),
            "/repo".into(),
            OrchestratorRef::from_tab("orch"),
        );
        plan.id = "plan".into();
        plan.tasks = tasks;
        plan
    }

    fn cfg(now_ms: u64) -> ReconcileConfig {
        ReconcileConfig {
            now_ms,
            stale_review_ms: 1_000,
            current_orchestrator_tab_id: Some("orch".into()),
        }
    }

    #[test]
    fn detects_in_progress_without_live_tab() {
        let mut t = task("t1", TaskStatus::InProgress);
        t.assigned_tab_id = Some("dead".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
            exists: false,
            ..Default::default()
        });
        assert!(items
            .iter()
            .any(|item| item.kind == DriftKind::InProgressTabGone));
    }

    #[test]
    fn detects_stale_in_review() {
        let mut t = task("t1", TaskStatus::InReview);
        t.status_changed_at = 1;
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_500), |_| TabProbe::default());
        assert_eq!(items[0].kind, DriftKind::StaleInReview);
        assert_eq!(items[0].suggested_fix, STALE_REVIEW_LABEL);
    }

    #[test]
    fn detects_stale_pending_unstaffed() {
        let mut t = task("t1", TaskStatus::Pending);
        t.status_changed_at = 1;
        let now = PENDING_UNSTAFFED_STALE_MS + 10;
        let items = detect_plan_drift(&plan(vec![t]), &cfg(now), |_| TabProbe::default());
        assert!(items
            .iter()
            .any(|item| item.kind == DriftKind::StalePendingUnstaffed));
    }

    #[test]
    fn dependency_waiting_pending_is_not_stale_unstaffed_or_aged() {
        let dependency = task("dependency", TaskStatus::InProgress);
        let mut waiting = task("waiting", TaskStatus::Pending);
        waiting.depends_on = vec![dependency.id.clone()];
        waiting.status_changed_at = 1;
        let plan = plan(vec![dependency, waiting]);
        let now = PENDING_UNSTAFFED_STALE_MS + 10;

        assert!(aged_open_tasks(&plan, now).is_empty());
        assert!(detect_plan_drift(&plan, &cfg(now), |_| TabProbe::default())
            .iter()
            .all(|item| item.kind != DriftKind::StalePendingUnstaffed));
    }

    #[test]
    fn missing_dependency_remains_waiting_and_is_not_stale_actionable() {
        let mut waiting = task("waiting", TaskStatus::Pending);
        waiting.depends_on = vec!["missing".into()];
        waiting.status_changed_at = 1;
        let plan = plan(vec![waiting]);
        let now = PENDING_UNSTAFFED_STALE_MS + 10;

        assert!(aged_open_tasks(&plan, now).is_empty());
        assert!(detect_plan_drift(&plan, &cfg(now), |_| TabProbe::default())
            .iter()
            .all(|item| item.kind != DriftKind::StalePendingUnstaffed));
    }

    #[test]
    fn aged_open_tasks_names_stale_work_and_skips_paused() {
        let mut pending = task("p1", TaskStatus::Pending);
        pending.status_changed_at = 1;
        let mut paused = task("k1", TaskStatus::Paused);
        paused.status_changed_at = 1;
        let mut fresh = task("f1", TaskStatus::InReview);
        fresh.status_changed_at = PENDING_UNSTAFFED_STALE_MS;
        let now = PENDING_UNSTAFFED_STALE_MS + 10;
        let aged = aged_open_tasks(&plan(vec![pending, paused, fresh]), now);
        assert_eq!(aged.len(), 1);
        assert_eq!(aged[0].task_id, "p1");
    }

    #[test]
    fn detects_bound_to_closed_tab() {
        let mut t = task("t1", TaskStatus::Pending);
        t.assigned_tab_id = Some("closed".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
            exists: false,
            ..Default::default()
        });
        assert_eq!(items[0].kind, DriftKind::BoundToClosedTab);
    }

    #[test]
    fn ignores_empty_and_terminal_closed_tab_bindings() {
        let mut empty = task("empty", TaskStatus::Pending);
        empty.assigned_tab_id = Some(" ".into());
        let mut done = task("done", TaskStatus::Done);
        done.assigned_tab_id = Some("closed".into());
        let mut cancelled = task("cancelled", TaskStatus::Cancelled);
        cancelled.assigned_tab_id = Some("closed".into());
        let items = detect_plan_drift(&plan(vec![empty, done, cancelled]), &cfg(2_000), |_| {
            TabProbe {
                exists: false,
                ..Default::default()
            }
        });
        assert!(items.is_empty());
        assert!(detect_tab_gone(&plan(vec![]), "").is_empty());
    }

    #[test]
    fn accepted_but_not_done_is_inert_without_acceptance_signal() {
        let mut t = task("t1", TaskStatus::InReview);
        t.milestones.push(crate::plan::Milestone {
            id: "m1".into(),
            from: Default::default(),
            kind: "review".into(),
            text: "looks good".into(),
            ts: 1,
            links: vec![],
            loop_id: None,
            iteration: None,
            canvas: Default::default(),
            attachments: vec![],
        });
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2), |_| TabProbe::default());
        assert!(!items
            .iter()
            .any(|item| item.kind == DriftKind::AcceptedButNotDone));
    }

    #[test]
    fn flags_not_mine_for_other_orchestrator_worker() {
        let mut t = task("t1", TaskStatus::InProgress);
        t.assigned_tab_id = Some("worker".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| TabProbe {
            exists: true,
            live_agent: false,
            role: Some(TabRole::Worker),
            parent_tab_id: Some("other-orch".into()),
            ..Default::default()
        });
        assert!(items[0].not_mine);
    }

    fn lane_thresholds() -> LaneThresholds {
        LaneThresholds {
            t_stall_ms: 100,
            t_overdue_ms: 1_000,
        }
    }

    fn in_progress_lane(id: &str, tab: &str, status_changed_at: u64) -> Task {
        let mut t = task(id, TaskStatus::InProgress);
        t.assigned_tab_id = Some(tab.into());
        t.status_changed_at = status_changed_at;
        t
    }

    #[test]
    fn demotes_dead_lane_past_grace() {
        let t = in_progress_lane("t1", "dead", 1);
        let demotions =
            detect_lane_demotions(&plan(vec![t]), &cfg(5_000), &lane_thresholds(), |_| {
                TabProbe {
                    exists: false,
                    ..Default::default()
                }
            });
        assert_eq!(demotions.len(), 1);
        assert_eq!(demotions[0].task_id, "t1");
    }

    #[test]
    fn leaves_live_lane_untouched() {
        let t = in_progress_lane("t1", "live", 4_900);
        let demotions =
            detect_lane_demotions(&plan(vec![t]), &cfg(5_000), &lane_thresholds(), |_| {
                TabProbe {
                    exists: true,
                    live_agent: true,
                    ..Default::default()
                }
            });
        assert!(demotions.is_empty());
    }

    #[test]
    fn never_demotes_report_overdue_lane() {
        // Tab present but silent past overdue: honest slow work, not gone.
        let t = in_progress_lane("t1", "live", 1);
        let demotions =
            detect_lane_demotions(&plan(vec![t]), &cfg(5_000), &lane_thresholds(), |_| {
                TabProbe {
                    exists: true,
                    live_agent: true,
                    ..Default::default()
                }
            });
        assert!(demotions.is_empty());
    }

    #[test]
    fn grace_window_spares_recently_active_dead_lane() {
        let t = in_progress_lane("t1", "dead", 4_900);
        let demotions =
            detect_lane_demotions(&plan(vec![t]), &cfg(5_000), &lane_thresholds(), |_| {
                TabProbe {
                    exists: false,
                    ..Default::default()
                }
            });
        assert!(demotions.is_empty());
    }

    #[test]
    fn never_demotes_other_orchestrator_lane() {
        let t = in_progress_lane("t1", "worker", 1);
        let demotions =
            detect_lane_demotions(&plan(vec![t]), &cfg(5_000), &lane_thresholds(), |_| {
                TabProbe {
                    exists: false,
                    live_agent: false,
                    role: Some(TabRole::Worker),
                    parent_tab_id: Some("other-orch".into()),
                    ..Default::default()
                }
            });
        assert!(demotions.is_empty());
    }

    #[test]
    fn tab_gone_detector_is_bound_to_requested_tab() {
        let mut t1 = task("t1", TaskStatus::InProgress);
        t1.assigned_tab_id = Some("gone".into());
        let mut t2 = task("t2", TaskStatus::InProgress);
        t2.assigned_tab_id = Some("live".into());
        let items = detect_tab_gone(&plan(vec![t1, t2]), "gone");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task_id, "t1");
    }
}
