//! Ordering fixtures for the worker report obligation. The obligation arms at
//! the turn-starting boundary (task dispatch) and clears on an accepted
//! explicit report; delivery of agent traffic must never re-arm it.

use super::DeliveryClass;
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, Task, TaskStatus};
use crate::service::messenger::MessageEnvelope;
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use crate::service::WorkerTurnSignal;

struct BoundWorker {
    fx: ServiceFixture,
    worker: String,
    orch: String,
    plan_id: String,
    task_id: String,
}

impl BoundWorker {
    fn task(&self) -> Task {
        self.fx
            .service
            .plan_store
            .get(&self.plan_id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.id == self.task_id)
            .unwrap()
    }

    fn report_count(&self) -> usize {
        self.fx.service.report_store.lock().count(&self.orch)
    }

    fn reports_json(&self) -> String {
        serde_json::to_string(&self.fx.service.report_store.lock().get(&self.orch, false))
            .unwrap()
    }
}

fn seed_bound_worker() -> BoundWorker {
    let (mut mux, orch) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orch).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orch))
        .expect("attach worker to orchestrator");
    mux.get_tab_mut(&worker).unwrap().agent_type = Some("claude".into());

    let fx = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "rearm ordering".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&orch),
    );
    fx.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fx
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "ordering fixture".into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["no fabricated partial".into()],
                test_plan: Some("focused regression".into()),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fx.service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker, None, 3)
        .unwrap();
    BoundWorker {
        fx,
        worker,
        orch,
        plan_id: plan.id,
        task_id,
    }
}

fn deliver_ack(bw: &BoundWorker) {
    let envelope = MessageEnvelope::tab("Orchestrator", bw.orch.clone());
    bw.fx
        .service
        .gateway_deliver(
            &bw.worker,
            envelope,
            "Report accepted — task stays in_review. No further action needed.",
            DeliveryClass::AgentTraffic,
        )
        .expect("acknowledgement delivery");
}

/// Report-accepted → ack-delivered → provider-stop, driven without timing.
/// The acknowledgement must not re-arm the obligation or fabricate a Partial.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn accepted_report_then_ack_then_provider_stop_fabricates_nothing() {
    let bw = seed_bound_worker();
    let svc = &bw.fx.service;

    // Dispatch is the turn-starting boundary: it arms the obligation explicitly.
    svc.gateway.require_worker_report(&bw.worker);
    assert!(svc.gateway.worker_report_required(&bw.worker));

    // Step (a): the worker's explicit report is accepted; the task moves to
    // in_review and the accepted report clears the obligation.
    svc.submit_report(
        &bw.worker,
        "STATUS: in_review\nCHANGES: fix landed\nTESTS RUN: focused",
        ReportDetails {
            status: Some(ReportStatus::InReview),
            ..Default::default()
        },
        None,
        None,
        None,
    )
    .expect("explicit report accepted");
    assert_eq!(bw.task().status, TaskStatus::InReview);
    assert!(
        !svc.gateway.worker_report_required(&bw.worker),
        "accepted explicit report must clear the obligation"
    );
    let reports_after_accept = bw.report_count();
    let accepted_reports = bw.reports_json();
    let task_after_accept = serde_json::to_string(&bw.task()).unwrap();

    // Step (b): a non-turn-starting acceptance ack is delivered to the bound
    // worker as agent traffic.
    deliver_ack(&bw);

    // Step (c): observe the requirement state directly, then (d) provider-stop.
    let rearmed_after_ack = svc.gateway.worker_report_required(&bw.worker);
    svc.complete_worker_turn_without_report(&bw.worker, WorkerTurnSignal::ProviderStop);
    let reports_after_stop = bw.report_count();
    let required_after_stop = svc.gateway.worker_report_required(&bw.worker);

    assert!(
        !rearmed_after_ack
            && reports_after_stop == reports_after_accept
            && !required_after_stop,
        "a non-turn-starting ack re-armed the obligation or provider-stop fabricated a \
         synthetic Partial; observed: rearmed_after_ack={rearmed_after_ack} \
         reports_after_accept={reports_after_accept} reports_after_stop={reports_after_stop} \
         required_after_stop={required_after_stop}"
    );
    // Immutable history: the accepted report and the task record (milestones,
    // review_audit) are byte-identical after the ack + provider-stop.
    assert_eq!(bw.reports_json(), accepted_reports);
    assert_eq!(serde_json::to_string(&bw.task()).unwrap(), task_after_accept);
    assert_eq!(bw.task().status, TaskStatus::InReview);
}

/// A genuinely unreported turn still produces one provider-stop handoff.
/// The resulting Partial must reach the orchestrator's report store.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn dispatched_brief_still_arms_and_provider_stop_still_fabricates_handoff() {
    let bw = seed_bound_worker();
    let svc = &bw.fx.service;

    svc.gateway.require_worker_report(&bw.worker);
    assert!(
        svc.gateway.worker_report_required(&bw.worker),
        "a dispatched brief must still arm the obligation"
    );
    let before = bw.report_count();

    svc.complete_worker_turn_without_report(&bw.worker, WorkerTurnSignal::ProviderStop);

    assert_eq!(
        bw.report_count(),
        before + 1,
        "an unreported finished turn must still produce the synthetic handoff"
    );
    assert!(!svc.gateway.worker_report_required(&bw.worker));
    let handoff = bw.task();
    assert!(
        handoff
            .milestones
            .iter()
            .any(|m| m.text.contains("automatic provider-stop handoff")),
        "the synthetic Partial must persist as a task milestone"
    );
}
