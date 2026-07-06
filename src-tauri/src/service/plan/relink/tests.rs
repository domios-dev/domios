use super::*;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Task};

fn task(id: &str, status: &str, pane: Option<&str>) -> Task {
    let pane_json = pane.map(|p| format!("\"{p}\"")).unwrap_or("null".into());
    let raw = format!(
        r#"{{"id":"{id}","name":"n","type":"implement","model":null,
            "model_rationale":null,"status":"{status}","status_label":null,
            "section_ids":[],"body":"","intent":null,
            "assigned_pane_id":{pane_json},"peer":null}}"#
    );
    serde_json::from_str(&raw).expect("task fixture")
}

fn worker_pane(mux: &mut MultiplexerState, parent: &str) -> String {
    let p = mux.create_pane();
    mux.set_pane_role(&p, AgentRole::Worker, Some(parent))
        .expect("attach worker");
    p
}

fn plan_with(tasks: Vec<Task>, planner: Option<PlannerRef>) -> Plan {
    let mut plan = Plan::new("t".into(), "/repo".into(), OrchestratorRef::from_pane("old"));
    plan.tasks = tasks;
    plan.planner = planner;
    plan
}

fn planner_ref(pane: &str) -> PlannerRef {
    PlannerRef::Pane {
        pane_id: pane.into(),
        session_id: None,
        title: None,
    }
}

#[test]
fn repoints_planner_and_in_progress_workers() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let new_owner = mux.create_pane();
    let w_active = worker_pane(&mut mux, &old_owner);
    let w_pending = worker_pane(&mut mux, &old_owner);
    let planner = worker_pane(&mut mux, &old_owner);

    let plan = plan_with(
        vec![
            task("t1", "in_progress", Some(&w_active)),
            task("t2", "pending", Some(&w_pending)),
        ],
        Some(planner_ref(&planner)),
    );

    let n = relink_to_owner(&mut mux, &plan, &new_owner);
    assert_eq!(n, 2, "active worker + planner relinked");

    assert_eq!(
        mux.get_pane(&w_active).unwrap().parent_pane_id.as_deref(),
        Some(new_owner.as_str()),
        "in_progress worker repointed to new owner"
    );
    assert_eq!(
        mux.get_pane(&planner).unwrap().parent_pane_id.as_deref(),
        Some(new_owner.as_str()),
        "planner repointed to new owner"
    );
    assert_eq!(
        mux.get_pane(&w_pending).unwrap().parent_pane_id.as_deref(),
        Some(old_owner.as_str()),
        "pending worker left on old owner"
    );
}

#[test]
fn unrelated_panes_untouched() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let new_owner = mux.create_pane();
    let other_orch = mux.create_pane();
    let w_active = worker_pane(&mut mux, &old_owner);
    let stranger = worker_pane(&mut mux, &other_orch);

    let plan = plan_with(vec![task("t1", "in_progress", Some(&w_active))], None);
    relink_to_owner(&mut mux, &plan, &new_owner);

    assert_eq!(
        mux.get_pane(&stranger).unwrap().parent_pane_id.as_deref(),
        Some(other_orch.as_str()),
        "unrelated pane keeps its parent"
    );
}

#[test]
fn absent_target_pane_is_noop() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let new_owner = mux.create_pane();
    let plan = plan_with(
        vec![task("t1", "in_progress", Some("gone_pane"))],
        Some(planner_ref("also_gone")),
    );
    // old_owner unused by relink; asserts no panic on absent targets.
    let _ = &old_owner;
    let n = relink_to_owner(&mut mux, &plan, &new_owner);
    assert_eq!(n, 0, "no live target panes to relink");
}

#[test]
fn new_owner_without_pane_is_noop() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let w_active = worker_pane(&mut mux, &old_owner);
    let plan = plan_with(vec![task("t1", "in_progress", Some(&w_active))], None);

    let n = relink_to_owner(&mut mux, &plan, "");
    assert_eq!(n, 0, "empty new owner pane is a clean no-op");
    assert_eq!(
        mux.get_pane(&w_active).unwrap().parent_pane_id.as_deref(),
        Some(old_owner.as_str()),
        "worker untouched when new owner has no pane"
    );
}

#[test]
fn idempotent_second_relink() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let new_owner = mux.create_pane();
    let w_active = worker_pane(&mut mux, &old_owner);
    let plan = plan_with(vec![task("t1", "in_progress", Some(&w_active))], None);

    assert_eq!(relink_to_owner(&mut mux, &plan, &new_owner), 1);
    // Second pass still succeeds (pane exists) but edge is already correct.
    assert_eq!(relink_to_owner(&mut mux, &plan, &new_owner), 1);
    assert_eq!(
        mux.get_pane(&w_active).unwrap().parent_pane_id.as_deref(),
        Some(new_owner.as_str())
    );
}

#[test]
fn target_equal_to_new_owner_skipped() {
    let (mut mux, _root) = MultiplexerState::new();
    let old_owner = mux.create_pane();
    let new_owner = worker_pane(&mut mux, &old_owner);
    let plan = plan_with(vec![task("t1", "in_progress", Some(&new_owner))], None);

    let n = relink_to_owner(&mut mux, &plan, &new_owner);
    assert_eq!(n, 0, "new owner is not made its own parent");
}
