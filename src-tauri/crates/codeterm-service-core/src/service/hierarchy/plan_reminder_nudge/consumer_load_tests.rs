//! Reminder ticks read only Active execution plans with unfinished work.

use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::load_audit::{assert_never_loaded, execution_plan, seed, start, task};
use crate::service::test_fixture::ServiceFixture;

/// One reminder candidate, one archived plan and one plan with only finished work.
fn seeded() -> (ServiceFixture, String, Vec<String>) {
    let (mux, _) = MultiplexerState::new();
    let fx = ServiceFixture::new(mux);
    let store = &fx.service.plan_store;
    let project = fx.temp.path().to_path_buf();
    let working = seed(
        store,
        execution_plan(
            &project,
            "working",
            "owner-tab",
            vec![task("task_w", TaskStatus::InProgress, Some("worker-tab"))],
        ),
    );
    let mut archived = execution_plan(
        &project,
        "archived",
        "owner-tab",
        vec![task("task_a", TaskStatus::InProgress, Some("worker-tab"))],
    );
    archived.status = PlanStatus::Archived;
    let archived = seed(store, archived);
    let finished = seed(
        store,
        execution_plan(
            &project,
            "finished",
            "owner-tab",
            vec![task("task_f", TaskStatus::Done, None)],
        ),
    );
    start(store);
    (fx, working, vec![archived, finished])
}

#[test]
fn send_due_plan_time_reminders_never_loads_unmatched_plans() {
    let (fx, working, unmatched) = seeded();

    fx.service.send_due_plan_time_reminders();
    assert_never_loaded(&fx.service.plan_store, &[&working]);
    assert_never_loaded(&fx.service.plan_store, &[&unmatched[0], &unmatched[1]]);
}

#[test]
fn reconcile_time_reminder_eligibility_never_loads_unmatched_plans() {
    let (fx, working, unmatched) = seeded();

    fx.service.reconcile_time_reminder_eligibility();
    assert_never_loaded(&fx.service.plan_store, &[&working]);
    assert_never_loaded(&fx.service.plan_store, &[&unmatched[0], &unmatched[1]]);
}

#[test]
fn live_host_worker_bindings_never_loads_unmatched_plans() {
    let (fx, working, unmatched) = seeded();
    let host = LiveHost {
        svc: &fx.service,
        interval: 60,
    };

    let ids = match host.worker_bindings("worker-tab", None).unwrap() {
        WorkerBindingRead::Bound { plan, .. } => vec![plan.id],
        WorkerBindingRead::Missing => Vec::new(),
        WorkerBindingRead::Ambiguous => panic!("one eligible binding"),
    };
    assert_eq!(ids, vec![working]);
    assert_never_loaded(&fx.service.plan_store, &[&unmatched[0], &unmatched[1]]);
}
