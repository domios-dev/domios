use super::PlanStore;
use crate::plan::{OrchestratorRef, Plan};

fn store_with_plan(owner: &str) -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let plan = store
        .create_plan(
            "Heartbeat".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_pane(owner),
            vec![],
            10,
        )
        .unwrap();
    (dir, store, plan.id)
}

#[test]
fn heartbeat_learning_persists_reloads_and_dedups() {
    let (dir, store, plan_id) = store_with_plan("orch-a");
    let first = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Always verify delayed reseed text at send time.",
            Some("reseed"),
            20,
        )
        .unwrap();
    assert!(!first.deduped);

    let duplicate = store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "always verify delayed reseed text at send time",
            Some("reseed"),
            30,
        )
        .unwrap();
    assert!(duplicate.deduped);
    assert_eq!(duplicate.learning.id, first.learning.id);

    let raw = std::fs::read_to_string(
        dir.path()
            .join(".codeterm")
            .join("plans")
            .join(format!("{plan_id}.json")),
    )
    .unwrap();
    let plan: Plan = serde_json::from_str(&raw).unwrap();
    assert_eq!(plan.heartbeat.learnings.len(), 1);

    let reloaded = PlanStore::new_for_test(dir.path());
    reloaded.scan_project(dir.path()).unwrap();
    let shown = reloaded.show_heartbeat(Some(&plan_id), None, None).unwrap();
    assert_eq!(shown.heartbeat.learnings[0].text, first.learning.text);
    assert!(shown.reinjection.contains("Recent learnings"));
}

#[test]
fn heartbeat_resolves_for_owner_pane_at_send_time() {
    let (_dir, store, plan_id) = store_with_plan("orch-a");
    let before = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert_eq!(before, "continue");

    store
        .add_heartbeat_learning(
            Some(&plan_id),
            None,
            None,
            "Workers report in_review, orchestrator verifies done.",
            None,
            20,
        )
        .unwrap();
    let after = store.prepend_heartbeat_for_orchestrator_pane("orch-a", "continue");
    assert!(after.contains("<codeterm-orchestrator-heartbeat"));
    assert!(after.contains("Workers report in_review"));
    assert!(after.ends_with("continue"));

    let worker_text = store.prepend_heartbeat_for_orchestrator_pane("worker-a", "continue");
    assert_eq!(worker_text, "continue");
}
