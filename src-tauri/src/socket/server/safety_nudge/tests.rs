use std::collections::HashSet;

use super::*;
use crate::plan::{AgentRef, OrchestratorRef, Plan};
use crate::service::plan::PlanStore;

#[test]
fn nudge_ids_are_stable_and_unique() {
    let unique: HashSet<&&str> = NUDGE_IDS.iter().collect();
    assert_eq!(unique.len(), NUDGE_IDS.len());
    assert!(NUDGE_IDS.iter().all(|id| !id.trim().is_empty()));
    assert!(NUDGE_IDS.contains(&NUDGE_RETROSPECTIVE_UNRECORDED));
}

#[test]
fn nudges_ride_a_successful_result_and_merge_into_existing_hints() {
    let mut result = serde_json::json!({ "ok": true, "_hints": ["pre-existing"] });
    let nudge = SafetyNudge::new(
        NUDGE_RETROSPECTIVE_UNRECORDED,
        "reason".into(),
        "codeterm plan retrospective pl_x".into(),
    );
    apply_safety_nudges(&mut result, std::slice::from_ref(&nudge));

    assert_eq!(result["ok"], true);
    assert_eq!(result["safety_nudges"][0]["id"], NUDGE_RETROSPECTIVE_UNRECORDED);
    assert!(!result["safety_nudges"][0]["safe_alternative"]
        .as_str()
        .unwrap()
        .is_empty());
    let hints = result["_hints"].as_array().unwrap();
    assert_eq!(hints.len(), 2, "existing hints must survive");
    assert_eq!(hints[0], "pre-existing");
}

#[test]
fn empty_nudges_leave_the_result_untouched() {
    let mut result = serde_json::json!({ "ok": true });
    apply_safety_nudges(&mut result, &[]);
    assert!(result.get("safety_nudges").is_none());
    assert!(result.get("_hints").is_none());
}

fn plan_without_retrospective() -> (tempfile::TempDir, PlanStore, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "retro".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    (dir, store, plan.id)
}

fn incidents(store: &PlanStore, plan_id: &str) -> usize {
    store
        .get(plan_id)
        .unwrap()
        .strides
        .iter()
        .filter(|stride| stride.kind == SAFETY_INCIDENT_KIND)
        .count()
}

#[test]
fn archive_without_retrospective_emits_nudge_and_durable_evidence() {
    let (_dir, store, plan_id) = plan_without_retrospective();

    let nudge =
        super::super::plan_archive::archive_with_evidence(&store, &plan_id, || store.archive(&plan_id, 3), 3)
            .unwrap()
            .expect("a plan with no retrospective must raise the advisory");

    assert_eq!(nudge.id, NUDGE_RETROSPECTIVE_UNRECORDED);
    assert!(!nudge.safe_alternative.is_empty());
    let reloaded = store.get(&plan_id).unwrap();
    assert_eq!(reloaded.status, crate::plan::PlanStatus::Archived);
    assert_eq!(incidents(&store, &plan_id), 1, "exactly one incident");
    assert!(reloaded
        .strides
        .iter()
        .find(|stride| stride.kind == SAFETY_INCIDENT_KIND)
        .unwrap()
        .text
        .contains(NUDGE_RETROSPECTIVE_UNRECORDED));
}

/// Failure injection: a durable incident must never claim an archive that failed.
#[test]
fn a_failed_archive_records_no_retrospective_incident() {
    let (_dir, store, plan_id) = plan_without_retrospective();

    let error = super::super::plan_archive::archive_with_evidence(
        &store,
        &plan_id,
        || Err(crate::service::plan::PlanError::Io("disk full".into())),
        3,
    )
    .unwrap_err();

    assert!(matches!(error, crate::service::plan::PlanError::Io(_)));
    assert_eq!(incidents(&store, &plan_id), 0, "no evidence without archive");
    assert_eq!(
        store.get(&plan_id).unwrap().status,
        crate::plan::PlanStatus::Active
    );
}

#[test]
fn a_recorded_retrospective_archives_with_no_incident_at_all() {
    let (_dir, store, plan_id) = plan_without_retrospective();
    store
        .add_retrospective(&plan_id, "what worked", AgentRef::from_tab("orch"), 2)
        .unwrap();

    let nudge = super::super::plan_archive::archive_with_evidence(
        &store,
        &plan_id,
        || store.archive(&plan_id, 3),
        3,
    )
    .unwrap();

    assert!(nudge.is_none());
    assert_eq!(incidents(&store, &plan_id), 0);
    assert_eq!(
        store.get(&plan_id).unwrap().status,
        crate::plan::PlanStatus::Archived
    );
}

#[test]
fn a_recorded_retrospective_raises_no_advisory() {
    let (_dir, store, plan_id) = plan_without_retrospective();
    store
        .add_retrospective(&plan_id, "what worked", AgentRef::from_tab("orch"), 2)
        .unwrap();

    assert!(super::super::plan_archive::retrospective_nudge(&store.get(&plan_id).unwrap()).is_none());
}
