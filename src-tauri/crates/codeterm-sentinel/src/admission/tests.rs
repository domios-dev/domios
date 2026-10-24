use super::*;

#[test]
fn plan_lock_admission_holds_and_requires_a_fresh_watch_baseline_on_unlock() {
    let mut admission = Admission::default();
    assert!(admission.permits_steps());
    assert!(admission.reconcile(true, true));
    assert!(!admission.permits_steps());
    assert_eq!(admission.held_reason, Some(HeldReason::PlanLimitLocked));
    assert!(!admission.reconcile(true, true));
    let mut restored: Admission = serde_json::from_value(serde_json::json!(admission)).unwrap();
    assert!(restored.reconcile(false, true));
    assert!(restored.permits_steps());
    assert!(restored.baseline_pending);
    assert!(!restored.reconcile(false, true));
}

#[test]
fn plan_lock_admission_retains_pipeline_cursor_without_replaying_completed_steps() {
    let cursor = serde_json::json!({"run_id":"run-1","current_step":3});
    let mut admission = Admission {
        continuation: Some(cursor.clone()),
        ..Default::default()
    };
    admission.reconcile(true, false);
    admission.reconcile(false, false);
    assert!(!admission.baseline_pending);
    assert_eq!(admission.continuation, Some(cursor));
    assert_eq!(
        serde_json::json!(HeldReason::PlanLimitLocked),
        "plan_limit_locked"
    );
}

#[test]
fn plan_lock_watch_reobserves_immediately_when_unlocked_before_its_next_deadline() {
    use crate::store::{Owner, Store};
    use serde_json::json;
    let temp = tempfile::tempdir().unwrap();
    let clock = codeterm_cron::clock::MockClock::starting_at(1000);
    let mut store = Store::new(temp.path().join("watch.json"), clock.clone());
    let job = store
        .create_document(
            crate::test_support::watch(
                json!("resume-baseline"),
                json!({"uses":"probe","with":{"command":{"lit":"opaque"}},"every":{"lit":"10s"}}),
                json!({"with":{"question":{"lit":"opaque"}},"buckets":["ready"]}),
                json!(null),
                json!(null),
            ),
            Owner {
                creator_tab_id: "creator".into(),
                team_id: None,
                workspace_id: "workspace".into(),
                cwd: ".".into(),
            },
            false,
        )
        .unwrap();
    assert_eq!(store.due().unwrap().len(), 1);
    store.release(&job.id);
    assert!(store.due().unwrap().is_empty());
    let mut admission = Admission::default();
    admission.reconcile(true, true);
    store
        .locked(|store| store.stage_admission(&job.id, admission.clone()))
        .unwrap();
    clock.set_ms(1001);
    assert!(store.due().unwrap().is_empty());
    admission.reconcile(false, true);
    store
        .locked(|store| store.stage_admission(&job.id, admission))
        .unwrap();
    let baseline = store.due().unwrap().pop().unwrap();
    assert!(baseline.job.admission.baseline_pending);
    assert_eq!(baseline.job.next_run, 11_001);
}
