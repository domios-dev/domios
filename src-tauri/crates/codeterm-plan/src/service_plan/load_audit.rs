//! Fixtures for the `*_never_loads_unmatched_plans` consumer tests: seed plans
//! straight into the store, then prove which ones a consumer loaded from SQL.

use super::PlanStore;
use crate::plan::{
    OrchestratorRef, Plan, PlanMode, PlanStatus, PlannerRef, Task, TaskExecutionAttempt, TaskStatus,
};
use std::path::Path;

/// A task the store accepts, with its status and optional assignment.
pub fn task(id: &str, status: TaskStatus, assigned_tab: Option<&str>) -> Task {
    let mut task: Task = serde_json::from_value(serde_json::json!({
        "id": id, "name": id, "type": "implement", "status": "pending"
    }))
    .expect("load audit task fixture");
    task.status = status;
    task.assigned_tab_id = assigned_tab.map(str::to_string);
    task
}

/// An unclosed attempt executed by `tab_id`.
pub fn attempt(id: &str, tab_id: &str) -> TaskExecutionAttempt {
    TaskExecutionAttempt {
        id: id.to_string(),
        executor_tab_id: tab_id.to_string(),
        ..Default::default()
    }
}

/// An active execution plan owned by `owner_tab`, holding `tasks`.
pub fn execution_plan(project: &Path, title: &str, owner_tab: &str, tasks: Vec<Task>) -> Plan {
    let mut plan = Plan::new(
        title.to_string(),
        project.display().to_string(),
        OrchestratorRef::from_tab(owner_tab),
    );
    plan.status = PlanStatus::Active;
    plan.mode = PlanMode::Execution;
    plan.tasks = tasks;
    plan
}

pub fn planner(tab_id: &str) -> Option<PlannerRef> {
    Some(PlannerRef::Tab {
        tab_id: tab_id.to_string(),
        session_id: Some(format!("{tab_id}-session")),
        title: None,
    })
}

/// Stores the plan and returns its id.
pub fn seed(store: &PlanStore, mut plan: Plan) -> String {
    store.save(&mut plan, 1).expect("seed load audit plan");
    plan.id
}

/// Starts from an empty cache, so every plan the consumer reads is a recorded SQL load.
pub fn start(store: &PlanStore) {
    store.clear_plan_cache_for_test();
    store.reset_plan_loads_for_test();
}

pub fn assert_never_loaded(store: &PlanStore, unmatched: &[&str]) {
    let loads = store.plan_loads_for_test();
    for id in unmatched {
        assert!(
            !loads.iter().any(|loaded| loaded == id),
            "consumer loaded unmatched plan {id}; loads: {loads:?}"
        );
    }
}
