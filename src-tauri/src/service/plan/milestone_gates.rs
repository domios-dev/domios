use serde::Serialize;

use crate::plan::{AgentRef, Milestone, Plan};
use super::prompts_plan::REGRESSION_MILESTONE_KIND;

use super::{PlanError, PlanStore};

pub const GROUP_REGRESSION_LINK_PREFIX: &str = "group:";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RegressionGate {
    pub key: String,
    pub label: String,
}

pub fn regression_link(group_key: &str) -> String {
    format!("{GROUP_REGRESSION_LINK_PREFIX}{group_key}")
}

fn group_label(plan: &Plan, key: &str) -> String {
    plan.task_groups
        .iter()
        .find(|g| g.key == key)
        .map(|g| g.label.clone())
        .unwrap_or_else(|| key.to_string())
}

// >=1 Done required so an all-cancelled group (shipped nothing) never gates.
pub fn group_completion(plan: &Plan, key: &str) -> Option<u64> {
    let members: Vec<&_> = plan
        .tasks
        .iter()
        .filter(|t| t.group.as_deref() == Some(key))
        .collect();
    if members.is_empty() {
        return None;
    }
    if !members.iter().all(|t| t.status.is_terminal()) {
        return None;
    }
    if !members
        .iter()
        .any(|t| t.status == crate::plan::TaskStatus::Done)
    {
        return None;
    }
    members.iter().map(|t| t.status_changed_at).max()
}

fn latest_regression_ts(plan: &Plan, key: &str) -> Option<u64> {
    let link = regression_link(key);
    plan.strides
        .iter()
        .filter(|m| m.kind == REGRESSION_MILESTONE_KIND && m.links.iter().any(|l| l == &link))
        .map(|m| m.ts)
        .max()
}

// Reopening a task bumps the close past the old record, re-arming the gate.
pub fn regression_satisfied(plan: &Plan, key: &str, completed_at: u64) -> bool {
    latest_regression_ts(plan, key).is_some_and(|ts| ts >= completed_at)
}

// Advisory only — complete groups whose full-gate regression is missing or stale.
pub fn pending_regression_gates(plan: &Plan) -> Vec<RegressionGate> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for task in &plan.tasks {
        let Some(key) = task.group.as_deref() else {
            continue;
        };
        if !seen.insert(key.to_string()) {
            continue;
        }
        let Some(completed_at) = group_completion(plan, key) else {
            continue;
        };
        if regression_satisfied(plan, key, completed_at) {
            continue;
        }
        out.push(RegressionGate {
            key: key.to_string(),
            label: group_label(plan, key),
        });
    }
    out.sort_by(|a, b| a.key.cmp(&b.key));
    out
}

impl PlanStore {
    // One plan-level working point per group, not a stride scattered per task.
    pub fn record_group_regression(
        &self,
        plan_id: &str,
        group_key: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<RegressionGate, PlanError> {
        let note = note.trim();
        if note.is_empty() {
            return Err(PlanError::QaNoteRequired);
        }
        let mid = self.mint_id("ms");
        let mut gate = None;
        self.with_plan(plan_id, now_ms, |p| {
            let group = p
                .task_groups
                .iter()
                .find(|g| g.key == group_key)
                .ok_or(PlanError::GroupNotFound)?;
            gate = Some(RegressionGate {
                key: group.key.clone(),
                label: group.label.clone(),
            });
            p.strides.push(Milestone {
                id: mid.clone(),
                from,
                kind: REGRESSION_MILESTONE_KIND.into(),
                text: note.into(),
                ts: now_ms,
                links: vec![regression_link(group_key)],
                loop_id: None,
                iteration: None,
                canvas: Default::default(),
                attachments: vec![],
            });
            Ok(())
        })?;
        gate.ok_or(PlanError::GroupNotFound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, TaskGroup, TaskStatus};
    use crate::service::plan::NewTask;
    use tempfile::TempDir;

    fn store_with_two_groups() -> (TempDir, PlanStore, String, Vec<String>) {
        let dir = TempDir::new().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Gate plan".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane("orch"),
        );
        plan.task_groups = vec![
            TaskGroup {
                key: "phase-1".into(),
                label: "Phase 1".into(),
                color: None,
            },
            TaskGroup {
                key: "phase-2".into(),
                label: "Phase 2".into(),
                color: None,
            },
        ];
        store.save(&mut plan, 1).unwrap();
        let ids = ["a", "b", "c"]
            .iter()
            .map(|n| {
                store
                    .add_task(
                        &plan.id,
                        NewTask {
                            name: (*n).into(),
                            kind: "implement".into(),
                            group: Some(if *n == "c" {
                                "phase-2".into()
                            } else {
                                "phase-1".into()
                            }),
                            ..Default::default()
                        },
                        2,
                    )
                    .unwrap()
            })
            .collect();
        (dir, store, plan.id, ids)
    }

    fn set_status(store: &PlanStore, pid: &str, tid: &str, status: TaskStatus, now: u64) {
        store
            .with_plan(pid, now, |p| {
                let t = p.tasks.iter_mut().find(|t| t.id == tid).unwrap();
                t.status = status.clone();
                t.status_changed_at = now;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn incomplete_group_never_fires() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        // ids[1] still pending in phase-1.
        let plan = store.get(&pid).unwrap();
        assert!(pending_regression_gates(&plan).is_empty());
    }

    #[test]
    fn last_task_close_fires_once_and_record_silences() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);

        let plan = store.get(&pid).unwrap();
        let gates = pending_regression_gates(&plan);
        assert_eq!(gates.len(), 1);
        assert_eq!(gates[0].key, "phase-1");
        assert_eq!(gates[0].label, "Phase 1");

        store
            .record_group_regression(
                &pid,
                "phase-1",
                "full suite green",
                AgentRef::from_pane("qa"),
                30,
            )
            .unwrap();
        let plan = store.get(&pid).unwrap();
        assert!(
            pending_regression_gates(&plan).is_empty(),
            "recording the regression run silences the gate"
        );
    }

    #[test]
    fn reopening_a_task_rearms_the_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Done, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 20);
        store
            .record_group_regression(&pid, "phase-1", "green", AgentRef::from_pane("qa"), 30)
            .unwrap();
        assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());

        // Reopen then re-close a member after the regression run — new work, stale gate.
        set_status(&store, &pid, &ids[1], TaskStatus::InProgress, 40);
        assert!(
            pending_regression_gates(&store.get(&pid).unwrap()).is_empty(),
            "an open member means the group is incomplete, so no gate yet"
        );
        set_status(&store, &pid, &ids[1], TaskStatus::Done, 50);

        let gates = pending_regression_gates(&store.get(&pid).unwrap());
        assert_eq!(
            gates.len(),
            1,
            "reclosing after the old record re-arms the gate"
        );
        assert_eq!(gates[0].key, "phase-1");
    }

    #[test]
    fn all_cancelled_group_has_nothing_to_gate() {
        let (_d, store, pid, ids) = store_with_two_groups();
        set_status(&store, &pid, &ids[0], TaskStatus::Cancelled, 10);
        set_status(&store, &pid, &ids[1], TaskStatus::Cancelled, 20);
        assert!(pending_regression_gates(&store.get(&pid).unwrap()).is_empty());
    }

    #[test]
    fn record_rejects_unknown_group_and_empty_note() {
        let (_d, store, pid, _ids) = store_with_two_groups();
        assert!(matches!(
            store.record_group_regression(&pid, "phase-1", "  ", AgentRef::from_pane("qa"), 5),
            Err(PlanError::QaNoteRequired)
        ));
        assert!(matches!(
            store.record_group_regression(&pid, "nope", "green", AgentRef::from_pane("qa"), 5),
            Err(PlanError::GroupNotFound)
        ));
    }
}
