use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanningAuthor, PlanningState};
use crate::service_plan::{NewTask, PlanError, PlanStore};
use tempfile::TempDir;

fn store() -> (TempDir, PlanStore, String) {
    let dir = TempDir::new().unwrap();
    let s = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Gate".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    s.save(&mut plan, 1).unwrap();
    (dir, s, plan.id)
}

fn add(store: &PlanStore, pid: &str, kind: &str) -> Result<String, PlanError> {
    store.ensure_task_creation_allowed(pid, kind, false)?;
    store.add_task(
        pid,
        NewTask {
            name: "t".into(),
            kind: kind.into(),
            ..Default::default()
        },
        2,
    )
}

#[test]
fn drafting_records_execution_tasks_without_unlocking_them() {
    let (_d, s, pid) = store();
    assert!(add(&s, &pid, "implement").is_ok());
    assert!(add(&s, &pid, "planning").is_ok());
    assert!(!s.get(&pid).unwrap().execution_tasks_unlocked());
}

#[test]
fn approved_state_unlocks_execution_tasks_and_mirrors_mode() {
    let (_d, s, pid) = store();
    s.set_planning_state(&pid, PlanningState::Approved, PlanningAuthor::User, None, 3)
        .unwrap();
    let plan = s.get(&pid).unwrap();
    assert_eq!(plan.planning_state, PlanningState::Approved);
    assert_eq!(plan.mode, PlanMode::Approved);
    assert_eq!(plan.planning_log.len(), 1);
    assert!(add(&s, &pid, "implement").is_ok());
}

#[test]
fn advance_records_author_and_is_idempotent() {
    let (_d, s, pid) = store();
    s.set_planning_state(
        &pid,
        PlanningState::StructureReady,
        PlanningAuthor::Planner {
            tab_id: "tab_p".into(),
        },
        Some("skeleton done"),
        3,
    )
    .unwrap();
    // Same state again — no new log entry.
    s.set_planning_state(
        &pid,
        PlanningState::StructureReady,
        PlanningAuthor::User,
        None,
        4,
    )
    .unwrap();
    let plan = s.get(&pid).unwrap();
    assert_eq!(plan.planning_log.len(), 1);
    assert_eq!(
        plan.planning_log[0].by,
        PlanningAuthor::Planner {
            tab_id: "tab_p".into()
        }
    );
    // StructureReady still locks execution tasks.
    assert!(!plan.execution_tasks_unlocked());
}

#[test]
fn unapprove_returns_to_structure_ready_and_relocks() {
    let (_d, s, pid) = store();
    s.set_planning_state(&pid, PlanningState::Approved, PlanningAuthor::User, None, 3)
        .unwrap();
    s.set_planning_state(
        &pid,
        PlanningState::StructureReady,
        PlanningAuthor::User,
        None,
        4,
    )
    .unwrap();
    let plan = s.get(&pid).unwrap();
    assert_eq!(plan.mode, PlanMode::Planning);
    assert!(!plan.execution_tasks_unlocked());
}

#[test]
fn legacy_approved_mode_bridges_gate_without_planning_state() {
    let (_d, s, pid) = store();
    // Simulate a pre-planning_state plan: mode Approved, planning_state Drafting.
    s.set_mode(&pid, PlanMode::Approved, 3).unwrap();
    let plan = s.get(&pid).unwrap();
    assert_eq!(plan.planning_state, PlanningState::Drafting);
    assert!(plan.execution_tasks_unlocked());
    assert!(add(&s, &pid, "implement").is_ok());
}
