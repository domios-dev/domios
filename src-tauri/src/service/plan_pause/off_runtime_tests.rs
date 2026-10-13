use std::sync::Arc;

use tauri::Manager;

use super::{PlanPausePorts, ServicePlanPausePorts};
use crate::api::cron_routes::off_runtime_gate::StorageCron;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

/// One worker: the unrelated task runs only if the parked schedule write left the worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn cron_entry_off_runtime_plan_pause_schedule_write_frees_the_runtime_worker() {
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let storage_cron = StorageCron::open();
    let held = storage_cron.create("held", "ws-held");
    let svc = Arc::new(fixture.service.clone());
    svc.app_handle.manage(storage_cron.cron.clone());
    svc.app_handle.manage(storage_cron.wake.clone());
    let mut gate = storage_cron.gate_next_commit();

    let pausing = {
        let (svc, job_id) = (svc.clone(), held.id.clone());
        tokio::spawn(async move {
            ServicePlanPausePorts { svc: &svc }.set_schedule_enabled(&job_id, false)
        })
    };
    gate.parked().await;

    let unrelated = tokio::spawn(async { "unrelated" })
        .await
        .expect("an unrelated task runs while the schedule write is parked");
    assert_eq!(unrelated, "unrelated");
    assert_eq!(
        ServicePlanPausePorts { svc: &svc }.schedule_enabled(&held.id),
        Some(true),
        "the parked commit is not yet published"
    );

    assert!(gate.release(), "the commit stayed parked until released");
    pausing
        .await
        .expect("schedule write task")
        .expect("schedule write");
    assert_eq!(
        ServicePlanPausePorts { svc: &svc }.schedule_enabled(&held.id),
        Some(false)
    );
}
