use super::{Plan, Task, TaskStatus};
use serde::{Deserialize, Serialize};

pub const DEFAULT_STALE_REVIEW_MS: u64 = 30 * 60 * 1000;
pub const NEEDS_REASSIGN_LABEL: &str = "needs-reassign";
pub const STALE_REVIEW_LABEL: &str = "stale-review";
pub const PANE_GONE_LABEL: &str = "pane-gone";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaneRole {
    Orchestrator,
    Worker,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneProbe {
    pub exists: bool,
    pub live_agent: bool,
    pub role: Option<PaneRole>,
    pub parent_pane_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ReconcileConfig {
    pub now_ms: u64,
    pub stale_review_ms: u64,
    pub current_orchestrator_pane_id: Option<String>,
}

impl ReconcileConfig {
    pub fn new(now_ms: u64) -> Self {
        Self {
            now_ms,
            stale_review_ms: DEFAULT_STALE_REVIEW_MS,
            current_orchestrator_pane_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    InProgressPaneGone,
    StaleInReview,
    BoundToClosedPane,
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

pub fn detect_plan_drift<F>(plan: &Plan, config: &ReconcileConfig, pane_probe: F) -> Vec<DriftItem>
where
    F: Fn(&str) -> PaneProbe,
{
    let mut items = Vec::new();
    for task in &plan.tasks {
        let pane = task
            .assigned_pane_id
            .as_deref()
            .map(|pane_id| (pane_id, pane_probe(pane_id)));
        let not_mine = pane
            .as_ref()
            .is_some_and(|(_, probe)| assigned_to_other_orchestrator(probe, config));

        if task.status == TaskStatus::InProgress {
            match pane.as_ref() {
                None => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressPaneGone,
                    "task is in_progress but has no assigned pane",
                    NEEDS_REASSIGN_LABEL,
                    not_mine,
                )),
                Some((pane_id, probe)) if !probe.exists => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressPaneGone,
                    format!("task is in_progress but assigned pane {pane_id} is closed"),
                    NEEDS_REASSIGN_LABEL,
                    not_mine,
                )),
                Some((pane_id, probe)) if !probe.live_agent => items.push(item(
                    plan,
                    task,
                    DriftKind::InProgressPaneGone,
                    format!("task is in_progress but pane {pane_id} has no live agent"),
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

        if let Some((pane_id, probe)) = pane.as_ref() {
            if !probe.exists {
                items.push(item(
                    plan,
                    task,
                    DriftKind::BoundToClosedPane,
                    format!("task is assigned to closed pane {pane_id}"),
                    "clear dead pane binding",
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

pub fn detect_pane_gone(plan: &Plan, pane_id: &str) -> Vec<DriftItem> {
    plan.tasks
        .iter()
        .filter(|task| task.assigned_pane_id.as_deref() == Some(pane_id))
        .map(|task| {
            item(
                plan,
                task,
                DriftKind::BoundToClosedPane,
                format!("assigned pane {pane_id} is gone"),
                PANE_GONE_LABEL,
                false,
            )
        })
        .collect()
}

fn assigned_to_other_orchestrator(probe: &PaneProbe, config: &ReconcileConfig) -> bool {
    if probe.role != Some(PaneRole::Worker) {
        return false;
    }
    let Some(parent) = probe.parent_pane_id.as_deref() else {
        return false;
    };
    config
        .current_orchestrator_pane_id
        .as_deref()
        .is_some_and(|mine| parent != mine)
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
            test_plan: None,
            correctness: None,
            status,
            status_label: None,
            priority: TaskPriority::P2,
            created_at: 0,
            updated_at: 0,
            status_changed_at: 0,
            handle: None,
            tags: vec![],
            section_ids: vec![],
            body: String::new(),
            intent: None,
            parent_task_id: None,
            assigned_pane_id: None,
            lane: Default::default(),
            taken_by_role: None,
            peer: None,
            group: None,
            canvas: Default::default(),
            milestones: vec![],
        }
    }

    fn plan(tasks: Vec<Task>) -> Plan {
        let mut plan = Plan::new(
            "p".into(),
            "/repo".into(),
            OrchestratorRef::from_pane("orch"),
        );
        plan.id = "plan".into();
        plan.tasks = tasks;
        plan
    }

    fn cfg(now_ms: u64) -> ReconcileConfig {
        ReconcileConfig {
            now_ms,
            stale_review_ms: 1_000,
            current_orchestrator_pane_id: Some("orch".into()),
        }
    }

    #[test]
    fn detects_in_progress_without_live_pane() {
        let mut t = task("t1", TaskStatus::InProgress);
        t.assigned_pane_id = Some("dead".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| PaneProbe {
            exists: false,
            ..Default::default()
        });
        assert!(items
            .iter()
            .any(|item| item.kind == DriftKind::InProgressPaneGone));
    }

    #[test]
    fn detects_stale_in_review() {
        let mut t = task("t1", TaskStatus::InReview);
        t.status_changed_at = 1;
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_500), |_| PaneProbe::default());
        assert_eq!(items[0].kind, DriftKind::StaleInReview);
        assert_eq!(items[0].suggested_fix, STALE_REVIEW_LABEL);
    }

    #[test]
    fn detects_bound_to_closed_pane() {
        let mut t = task("t1", TaskStatus::Pending);
        t.assigned_pane_id = Some("closed".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| PaneProbe {
            exists: false,
            ..Default::default()
        });
        assert_eq!(items[0].kind, DriftKind::BoundToClosedPane);
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
        });
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2), |_| PaneProbe::default());
        assert!(!items
            .iter()
            .any(|item| item.kind == DriftKind::AcceptedButNotDone));
    }

    #[test]
    fn flags_not_mine_for_other_orchestrator_worker() {
        let mut t = task("t1", TaskStatus::InProgress);
        t.assigned_pane_id = Some("worker".into());
        let items = detect_plan_drift(&plan(vec![t]), &cfg(2_000), |_| PaneProbe {
            exists: true,
            live_agent: false,
            role: Some(PaneRole::Worker),
            parent_pane_id: Some("other-orch".into()),
        });
        assert!(items[0].not_mine);
    }

    #[test]
    fn pane_gone_detector_is_bound_to_requested_pane() {
        let mut t1 = task("t1", TaskStatus::InProgress);
        t1.assigned_pane_id = Some("gone".into());
        let mut t2 = task("t2", TaskStatus::InProgress);
        t2.assigned_pane_id = Some("live".into());
        let items = detect_pane_gone(&plan(vec![t1, t2]), "gone");
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].task_id, "t1");
    }
}
