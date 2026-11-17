//! Worker interruption asks the plan store for the plans it matches; each test proves a
//! non-matching plan is never loaded. Moved from the core's consumer_load_tests with its code.

use crate::plan::TaskStatus;
use crate::prelude::*;
use crate::service::agent_lifecycle::WorkerInterruptionEvent;
use crate::service::consumer_load_fixture::{fixture, project};
use crate::service::plan::load_audit::{
    assert_never_loaded, attempt, execution_plan, seed, start, task,
};

#[test]
fn signal_provider_exhaustion_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut exhausted = task("task_c", TaskStatus::InProgress, Some("worker-a"));
    let mut provider_a = attempt("att_c", "worker-a");
    provider_a.provider = Some("fixture-provider-a".into());
    exhausted.execution_attempts.push(provider_a);
    let exhausted = seed(
        store,
        execution_plan(
            &project(&fx),
            "fixture-provider-a",
            "owner",
            vec![exhausted],
        ),
    );
    let mut fine = task("task_x", TaskStatus::InProgress, Some("worker-b"));
    let mut provider_b = attempt("att_x", "worker-b");
    provider_b.provider = Some("fixture-provider-b".into());
    fine.execution_attempts.push(provider_b);
    let fine = seed(
        store,
        execution_plan(&project(&fx), "fixture-provider-b", "owner", vec![fine]),
    );
    start(store);

    assert_eq!(
        fx.service
            .signal_provider_exhaustion("fixture-provider-a")
            .unwrap(),
        0,
        "no task names an interruption owner"
    );
    assert!(
        store.plan_loads_for_test().is_empty(),
        "exhaustion must select {exhausted} without loading a Plan"
    );
    assert_never_loaded(store, &[&fine]);
}

#[test]
fn signal_provider_recovery_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut paused = task("task_p", TaskStatus::Paused, None);
    paused.status_label = Some(crate::service::plan::interruption::PROVIDER_EXHAUSTED_LABEL.into());
    paused.interruption.owner_tab_id = Some("worker-a".into());
    let mut provider_b = attempt("att_p", "worker-a");
    provider_b.provider = Some("fixture-provider-b".into());
    paused.execution_attempts.push(provider_b);
    let paused = seed(
        store,
        execution_plan(&project(&fx), "paused", "owner", vec![paused]),
    );
    let running = seed(
        store,
        execution_plan(
            &project(&fx),
            "running",
            "owner",
            vec![task("task_r", TaskStatus::InProgress, Some("worker-b"))],
        ),
    );
    start(store);

    assert_eq!(
        fx.service
            .signal_provider_recovery("fixture-provider-a")
            .unwrap(),
        0,
        "another provider paused it"
    );
    assert!(
        store.plan_loads_for_test().is_empty(),
        "recovery must select {paused} without loading a Plan"
    );
    assert_never_loaded(store, &[&running]);
}

#[test]
fn signal_worker_interruption_never_loads_unmatched_plans() {
    let (fx, _) = fixture();
    let store = &fx.service.plan_store;
    let mut owned = task("task_o", TaskStatus::InProgress, Some("worker-a"));
    owned.interruption.owner_tab_id = Some("worker-a".into());
    owned.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
    owned.execution_attempts.push(attempt("att_o", "worker-a"));
    let owned = seed(
        store,
        execution_plan(&project(&fx), "owned", "owner", vec![owned]),
    );
    let mut other = task("task_x", TaskStatus::InProgress, Some("worker-b"));
    other.interruption.owner_tab_id = Some("worker-b".into());
    other.interruption.execution_intent = crate::plan::ExecutionIntent::Mutating;
    other.execution_attempts.push(attempt("att_x", "worker-b"));
    let other = seed(
        store,
        execution_plan(&project(&fx), "other", "owner", vec![other]),
    );
    start(store);

    assert!(
        !fx.service
            .signal_worker_interruption("worker-a", WorkerInterruptionEvent::ProviderExhausted)
            .unwrap(),
        "an owner without a session cannot be checkpointed"
    );
    assert!(
        store.plan_loads_for_test().is_empty(),
        "interruption must select {owned} without loading a Plan"
    );
    assert_never_loaded(store, &[&other]);
}
