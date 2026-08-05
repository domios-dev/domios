use crate::plan::{ExecutionIntent, OrchestratorRef, TaskLaneMetadata};
use crate::service::plan::{NewTask, PlanStore};

use super::super::lane_safety::{
    atomic_lane_commitment, dispatch_safety_nudges, refuse_unsafe_mutating_write,
    REFUSAL_MUTATING_WRITE_IN_CANONICAL, REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE,
};
use crate::socket::server::safety_nudge::{
    apply_safety_nudges, safety_incident_text, NUDGE_IDS, SAFETY_INCIDENT_KIND,
};

fn violation_id(error: &crate::socket::server::JsonRpcError) -> String {
    error.data.as_ref().unwrap()["violation_id"]
        .as_str()
        .unwrap()
        .to_string()
}

fn plan_with_two_tasks() -> (tempfile::TempDir, PlanStore, crate::plan::Plan, String, String) {
    let root = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(root.path());
    let created = store
        .create_plan_for_team(
            "Lane safety".into(),
            root.path().display().to_string(),
            OrchestratorRef::from_tab("orch"),
            "team".into(),
            vec![],
            1,
        )
        .unwrap();
    let mine = store
        .add_task(
            &created.id,
            NewTask {
                name: "mine".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let theirs = store
        .add_task(
            &created.id,
            NewTask {
                name: "theirs".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    let mut plan = store.get(&created.id).unwrap();
    let foreign_lane = root.path().join("foreign-lane").display().to_string();
    plan.tasks
        .iter_mut()
        .find(|task| task.id == theirs)
        .unwrap()
        .lane
        .worktree_path = Some(foreign_lane);
    (root, store, plan, mine, theirs)
}

#[test]
fn mutating_dispatch_into_canonical_is_refused() {
    let (root, _store, plan, mine, _theirs) = plan_with_two_tasks();
    let canonical = root.path().display().to_string();

    let error = refuse_unsafe_mutating_write(&plan, &mine, &canonical, ExecutionIntent::Mutating)
        .unwrap_err();

    assert_eq!(violation_id(&error), REFUSAL_MUTATING_WRITE_IN_CANONICAL);
    assert_eq!(error.data.as_ref().unwrap()["stage"], "lane_safety");
    assert!(!error.data.as_ref().unwrap()["safe_alternative"]
        .as_str()
        .unwrap()
        .is_empty());
}

#[test]
fn mutating_dispatch_into_foreign_lane_is_refused() {
    let (root, _store, plan, mine, theirs) = plan_with_two_tasks();
    let foreign = root.path().join("foreign-lane").display().to_string();

    let error =
        refuse_unsafe_mutating_write(&plan, &mine, &foreign, ExecutionIntent::Mutating).unwrap_err();

    assert_eq!(violation_id(&error), REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE);
    assert!(error.message.contains(&theirs), "{}", error.message);
}

#[test]
fn a_task_dispatched_into_its_own_isolated_lane_is_not_refused() {
    let (root, _store, plan, mine, _theirs) = plan_with_two_tasks();
    let own_lane = root.path().join("my-lane").display().to_string();

    refuse_unsafe_mutating_write(&plan, &mine, &own_lane, ExecutionIntent::Mutating).unwrap();
}

#[test]
fn read_only_dispatch_is_unobstructed() {
    let (root, _store, plan, mine, _theirs) = plan_with_two_tasks();
    let canonical = root.path().display().to_string();
    let foreign = root.path().join("foreign-lane").display().to_string();

    // Negative control: the same two checkouts that a mutating Worker may never
    // touch stay legal for a read-only one, and for an unspecified intent.
    for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Unspecified] {
        refuse_unsafe_mutating_write(&plan, &mine, &canonical, intent.clone()).unwrap();
        refuse_unsafe_mutating_write(&plan, &mine, &foreign, intent).unwrap();
    }
}

#[test]
fn advisory_findings_emit_nudge_and_do_not_block() {
    let lane = TaskLaneMetadata {
        worktree_path: Some("/lanes/alpha".into()),
        ..Default::default()
    };
    let nudges = dispatch_safety_nudges("pl_x", "task_y", &lane);
    assert!(!nudges.is_empty());

    let mut result = serde_json::json!({ "ok": true, "accepted": true });
    apply_safety_nudges(&mut result, &nudges);

    // The operation still succeeds; the finding rides alongside it.
    assert_eq!(result["ok"], true);
    assert_eq!(result["accepted"], true);
    let emitted = result["safety_nudges"].as_array().unwrap();
    assert_eq!(emitted.len(), nudges.len());
    for nudge in emitted {
        assert!(NUDGE_IDS.contains(&nudge["id"].as_str().unwrap()));
        assert!(!nudge["reason"].as_str().unwrap().is_empty());
        assert!(!nudge["safe_alternative"].as_str().unwrap().is_empty());
    }
    assert_eq!(result["_hints"].as_array().unwrap().len(), nudges.len());
}

#[test]
fn a_fully_recorded_lane_raises_no_nudge_at_all() {
    let lane = TaskLaneMetadata {
        worktree_path: Some("/lanes/alpha".into()),
        branch: Some("lane/alpha".into()),
        base: Some("abc123".into()),
        workspace_id: Some("ws_1".into()),
        workspace_name: Some("codeterm".into()),
        destination: Some("develop".into()),
        ..Default::default()
    };
    assert!(dispatch_safety_nudges("pl_x", "task_y", &lane).is_empty());

    let mut result = serde_json::json!({ "ok": true });
    apply_safety_nudges(&mut result, &[]);
    assert!(result.get("safety_nudges").is_none());
    assert!(result.get("_hints").is_none());
}

/// The refusal ids are a separate, disjoint set from the advisory ids.
#[test]
fn refusal_ids_are_distinct_and_never_advisory() {
    assert_ne!(
        REFUSAL_MUTATING_WRITE_IN_CANONICAL,
        REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE
    );
    assert!(!NUDGE_IDS.contains(&REFUSAL_MUTATING_WRITE_IN_CANONICAL));
    assert!(!NUDGE_IDS.contains(&REFUSAL_MUTATING_WRITE_IN_FOREIGN_LANE));
}

#[test]
fn atomic_lane_provisioning_supplies_team_and_isolation() {
    let isolated = TaskLaneMetadata {
        worktree_path: Some("/lanes/alpha".into()),
        branch: Some("lane/alpha".into()),
        base: Some("abc123".into()),
        workspace_id: Some("ws_1".into()),
        workspace_name: Some("codeterm".into()),
        ..Default::default()
    };
    let committed = atomic_lane_commitment(&isolated);
    assert_eq!(committed.workspace_id.as_deref(), Some("ws_1"));
    assert_eq!(committed.worktree_path.as_deref(), Some("/lanes/alpha"));

    // No persisted state may carry Team identity without the lane isolating it.
    let identity_only = TaskLaneMetadata {
        workspace_id: Some("ws_1".into()),
        workspace_name: Some("codeterm".into()),
        ..Default::default()
    };
    let committed = atomic_lane_commitment(&identity_only);
    assert!(committed.workspace_id.is_none());
    assert!(committed.workspace_name.is_none());
    assert!(committed.worktree_path.is_none());
}

#[test]
fn incident_evidence_is_recorded_durably() {
    let (root, store, plan, mine, _theirs) = plan_with_two_tasks();
    let canonical = root.path().display().to_string();
    let error = refuse_unsafe_mutating_write(&plan, &mine, &canonical, ExecutionIntent::Mutating)
        .unwrap_err();
    let data = error.data.as_ref().unwrap();
    let text = safety_incident_text(
        &mine,
        data["violation_id"].as_str().unwrap(),
        data["reason"].as_str().unwrap(),
    );
    store
        .add_plan_stride(
            &plan.id,
            SAFETY_INCIDENT_KIND,
            &text,
            crate::plan::AgentRef::from_tab("dispatcher"),
            9,
        )
        .unwrap();

    // Re-read from the store (not the in-memory value) so this proves durability.
    let reloaded = store.get(&plan.id).unwrap();
    let incident = reloaded
        .strides
        .iter()
        .find(|stride| stride.kind == SAFETY_INCIDENT_KIND)
        .expect("the refusal must leave durable incident evidence");
    assert!(incident.text.contains(REFUSAL_MUTATING_WRITE_IN_CANONICAL));
    assert!(incident.text.contains(&mine));
}
