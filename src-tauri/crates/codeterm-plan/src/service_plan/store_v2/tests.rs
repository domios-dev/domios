use super::*;
use crate::plan::{Goal, OrchestratorRef, PlanMode, TaskStatus};
use crate::service_plan::{NewTask, PlanStore};

// Build a real, fully-populated Plan+Task via the production store, then hand it to store_v2 — so the round-trip exercises the actual model, not a hand-rolled subset of fields.
fn sample_plan(dir: &Path) -> Plan {
    let store = PlanStore::new_for_test(dir);
    let mut plan = Plan::new(
        "Storage v2 roundtrip".into(),
        dir.display().to_string(),
        OrchestratorRef::default(),
    );
    plan.id = "pl_test".into();
    plan.mode = PlanMode::Execution;
    plan.goal = Some(Goal {
        vector: "prove the store round-trips".into(),
        vision: "multi\nline\nvision".into(),
        constraints: vec!["c1".into()],
        postulates: vec![],
    });
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "First task".into(),
                handle: Some("first-task".into()),
                body: "A body with\nmultiple lines\nas a block scalar.".into(),
                executor_role: Some("backend-dev".into()),
                role_guidance: Some("You are a careful Rust worker.".into()),
                model: Some("claude/opus".into()),
                model_rationale: Some("ordering risk".into()),
                acceptance_criteria: vec!["compiles".into(), "round-trips".into()],
                ..Default::default()
            },
            2,
        )
        .unwrap();
    // A tab must exist before in_progress (write-time invariant); set it,
    // then flip status — this also exercises the lane fields in the yaml.
    store
        .fill_task_lane_if_empty(
            &plan.id,
            &task_id,
            Some("tab-x"),
            Some("lane/first"),
            None,
            3,
        )
        .unwrap();
    store
        .update_task(
            &plan.id,
            &task_id,
            None,
            None,
            None,
            None,
            Some("in_progress"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            &[],
            None,
            None,
            4,
        )
        .unwrap();
    store.get(&plan.id).unwrap()
}

#[test]
fn plan_round_trips_through_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let plan = sample_plan(dir.path());
    write_plan(dir.path(), &plan).unwrap();
    let loaded = load_plan(dir.path()).unwrap();
    assert_eq!(loaded.id, plan.id);
    assert_eq!(loaded.title, plan.title);
    assert_eq!(loaded.tasks.len(), 1);
    let t = &loaded.tasks[0];
    assert_eq!(t.body, plan.tasks[0].body);
    assert_eq!(t.executor_role.as_deref(), Some("backend-dev"));
    assert_eq!(
        t.role_guidance.as_deref(),
        Some("You are a careful Rust worker.")
    );
    assert_eq!(t.model.as_deref(), Some("claude/opus"));
    assert_eq!(t.status, TaskStatus::InProgress);
    assert_eq!(loaded.goal.as_ref().unwrap().vision, "multi\nline\nvision");
}

#[test]
fn plan_yaml_roundtrip_team_id() {
    let dir = tempfile::tempdir().unwrap();
    let mut plan = sample_plan(dir.path());
    plan.team_id = Some("team_abcd1234".into());
    write_plan(dir.path(), &plan).unwrap();
    let meta = std::fs::read_to_string(plan_yaml_path(dir.path())).unwrap();
    assert!(
        meta.contains("team_abcd1234"),
        "plan.yaml must persist team_id"
    );
    let loaded = load_plan(dir.path()).unwrap();
    assert_eq!(loaded.team_id.as_deref(), Some("team_abcd1234"));
}

#[test]
fn plan_yaml_omits_team_id_when_teamless() {
    let dir = tempfile::tempdir().unwrap();
    let plan = sample_plan(dir.path());
    assert_eq!(plan.team_id, None);
    write_plan(dir.path(), &plan).unwrap();
    let loaded = load_plan(dir.path()).unwrap();
    assert_eq!(loaded.team_id, None);
}

#[test]
fn plan_yaml_holds_no_task_copies() {
    let dir = tempfile::tempdir().unwrap();
    write_plan(dir.path(), &sample_plan(dir.path())).unwrap();
    let meta = std::fs::read_to_string(plan_yaml_path(dir.path())).unwrap();
    assert!(
        !meta.contains("First task"),
        "plan.yaml must not copy task data"
    );
    assert!(
        !meta.contains("block scalar"),
        "plan.yaml must not copy task bodies"
    );
}

#[test]
fn single_task_write_touches_only_its_file() {
    let dir = tempfile::tempdir().unwrap();
    let plan = sample_plan(dir.path());
    write_plan(dir.path(), &plan).unwrap();
    let plan_mtime = std::fs::metadata(plan_yaml_path(dir.path()))
        .unwrap()
        .modified()
        .unwrap();
    let mut t = plan.tasks[0].clone();
    t.status = TaskStatus::Done;
    assert!(
        write_task(dir.path(), &t).unwrap(),
        "changed task must write"
    );
    let plan_mtime_after = std::fs::metadata(plan_yaml_path(dir.path()))
        .unwrap()
        .modified()
        .unwrap();
    assert_eq!(
        plan_mtime, plan_mtime_after,
        "plan.yaml must not move on a task write"
    );
}

#[test]
fn identical_write_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let plan = sample_plan(dir.path());
    write_plan(dir.path(), &plan).unwrap();
    assert!(
        !write_task(dir.path(), &plan.tasks[0]).unwrap(),
        "no-op write must skip"
    );
}

#[test]
fn renamed_handle_prunes_the_stale_file() {
    let dir = tempfile::tempdir().unwrap();
    let mut plan = sample_plan(dir.path());
    write_plan(dir.path(), &plan).unwrap();
    assert!(task_yaml_path(dir.path(), &plan.tasks[0]).exists());
    let old = task_yaml_path(dir.path(), &plan.tasks[0]);
    plan.tasks[0].handle = Some("renamed-task".into());
    write_plan(dir.path(), &plan).unwrap();
    assert!(!old.exists(), "stale handle file must be pruned");
    assert!(task_yaml_path(dir.path(), &plan.tasks[0]).exists());
}
