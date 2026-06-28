use super::PlanStore;
use crate::plan::{PlanStatus, TaskStatus};

/// Compact, self-sufficient restatement of the three Worker rules. Mirrors the
/// orchestrator-heartbeat reinjection so worker role survives context drift.
pub fn render_worker_charter(task_id: &str) -> String {
    format!(
        "<codeterm-worker-charter task=\"{task_id}\">\n\
You are a Worker on task {task_id}. Three rules:\n\
- Do this ONE task, then run `codeterm agent report` and exit.\n\
- NEVER spawn agents.\n\
- Decomposition needed \u{2192} report blocked; the Orchestrator splits it.\n\
</codeterm-worker-charter>"
    )
}

fn is_open(status: &TaskStatus) -> bool {
    !matches!(status, TaskStatus::Done | TaskStatus::Cancelled)
}

impl PlanStore {
    /// Resolve the open task assigned to this pane across active plans. Newest
    /// matching active plan wins; unknown panes resolve to None.
    fn open_task_for_pane(&self, pane_id: &str) -> Option<String> {
        if pane_id.trim().is_empty() {
            return None;
        }
        for entry in self.list_index() {
            let Some(plan) = self.get(&entry.plan_id) else {
                continue;
            };
            if plan.status != PlanStatus::Active {
                continue;
            }
            if let Some(task) = plan.tasks.iter().find(|task| {
                is_open(&task.status)
                    && (task.assigned_pane_id.as_deref() == Some(pane_id)
                        || task.lane.pane_id.as_deref() == Some(pane_id))
            }) {
                return Some(task.id.clone());
            }
        }
        None
    }

    /// Prepend the worker charter for a pane currently assigned to an open task.
    /// No-op (returns `text` unchanged) for panes with no such assignment —
    /// the orchestrator-heartbeat prepend is the symmetric counterpart.
    pub fn prepend_worker_charter_for_pane(&self, pane_id: &str, text: &str) -> String {
        let Some(task_id) = self.open_task_for_pane(pane_id) else {
            return text.to_string();
        };
        let charter = render_worker_charter(&task_id);
        if text.trim().is_empty() {
            charter
        } else {
            format!("{charter}\n\n{text}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};
    use crate::service::plan::NewTask;

    fn store_with_assigned_task() -> (tempfile::TempDir, PlanStore, String) {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "Charter".into(),
            dir.path().display().to_string(),
            OrchestratorRef::default(),
        );
        plan.id = "pl_charter".into();
        store.save(&mut plan, 1).unwrap();
        let task_id = store
            .add_task(
                &plan.id,
                NewTask {
                    name: "Do the thing".into(),
                    kind: "task".into(),
                    assigned_pane_id: Some("worker-pane".into()),
                    ..Default::default()
                },
                2,
            )
            .unwrap();
        (dir, store, task_id)
    }

    #[test]
    fn prepends_charter_for_assigned_pane() {
        let (_dir, store, task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_pane("worker-pane", "continue");

        assert!(out.contains("<codeterm-worker-charter"));
        assert!(out.contains(&task_id));
        assert!(out.contains("NEVER spawn agents"));
        assert!(out.trim_end().ends_with("continue"));
    }

    #[test]
    fn noop_for_unassigned_pane() {
        let (_dir, store, _task_id) = store_with_assigned_task();

        let out = store.prepend_worker_charter_for_pane("some-other-pane", "continue");

        assert_eq!(out, "continue");
    }
}
