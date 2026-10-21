use super::*;
use crate::service::plan::task_attempts::TaskRespawnRequest;
use crate::service::plan::{PlanError, PlanStore};
use std::sync::Barrier;

fn close_and_read_advised_source(busy: &BusyWorker) -> String {
    let mut receiver = busy.fx.service.events.subscribe();
    busy.fx.service.close_tab(&busy.worker).unwrap();
    assert!(busy.fx.service.mux.read().get_tab(&busy.worker).is_none());
    let events = worker_lost_events(&mut receiver);
    assert_eq!(events.len(), 1);
    let held = pending_worker_losses()
        .lock()
        .pending_for(&busy.orchestrator);
    assert_eq!(held.len(), 1);
    let notice = held[0].notice_text();
    let command = events[0]["recovery"].as_str().unwrap();
    assert!(notice.contains(command));
    let arguments: Vec<_> = command.split_whitespace().collect();
    assert_eq!(&arguments[..4], &["codeterm", "plan", "task", "respawn"]);
    assert_eq!(arguments[4], busy.plan_id);
    assert_eq!(arguments[5], busy.task_id);
    assert_eq!(arguments[6], "--attempt");
    assert_eq!(arguments.len(), 8);
    let source = arguments[7].to_string();
    let task = busy
        .fx
        .service
        .plan_store
        .show_task(&busy.task_id)
        .unwrap()
        .task;
    let lost = task.execution_attempts.last().unwrap();
    assert_eq!(lost.id, source);
    assert!(lost.closed_at.is_some());
    assert!(lost.retired_at.is_some());
    source
}

fn advised_respawn(
    store: &PlanStore,
    plan: &str,
    task: &str,
    source: &str,
    tab: &str,
    barrier: Option<&Barrier>,
) -> Result<String, PlanError> {
    let draft = store.build_task_respawn_draft(
        plan,
        task,
        TaskRespawnRequest {
            attempt_id: source.into(),
            ..Default::default()
        },
    )?;
    if let Some(barrier) = barrier {
        barrier.wait();
    }
    let _reservation = store.reserve_task_respawn_startup(plan, task, source, &draft.attempt_id)?;
    let created = store.record_task_respawn_attempt(plan, task, tab, &draft, Some("worker"), 20)?;
    Ok(created.attempt.id)
}

#[test]
fn tab_gone_notice_advised_respawn_succeeds_once_and_refuses_replay() {
    let busy = seed_busy_worker();
    let source = close_and_read_advised_source(&busy);
    let store = &busy.fx.service.plan_store;
    let replacement = advised_respawn(store, &busy.plan_id, &busy.task_id, &source, "next", None)
        .expect("the command printed after tab loss must accept the finalized source");
    assert!(matches!(
        advised_respawn(store, &busy.plan_id, &busy.task_id, &source, "duplicate", None),
        Err(PlanError::RespawnAttemptNotCurrent { current_attempt_id, tab_id })
            if current_attempt_id == replacement && tab_id == "next"
    ));
    let task = store.show_task(&busy.task_id).unwrap().task;
    assert_eq!(task.execution_attempts.len(), 2);
    assert_eq!(task.execution_attempts.last().unwrap().id, replacement);
    assert_eq!(task.assigned_tab_id.as_deref(), Some("next"));
    let loss = busy
        .fx
        .service
        .worker_loss_for_task(
            &busy.plan_id,
            &busy.task_id,
            &busy.worker,
            WorkerLossCause::TabGone,
        )
        .unwrap();
    assert!(loss.recovery.contains(&replacement));
    assert!(loss.recovery.contains("next"));
    assert!(!loss.recovery.contains("codeterm plan task respawn"));
}

#[test]
fn tab_gone_notice_concurrent_advised_respawns_create_one_replacement() {
    let busy = seed_busy_worker();
    let source = close_and_read_advised_source(&busy);
    let barrier = Barrier::new(2);
    let outcomes = std::thread::scope(|scope| {
        let workers = ["next-a", "next-b"].map(|tab| {
            let busy = &busy;
            let source = &source;
            let barrier = &barrier;
            scope.spawn(move || {
                advised_respawn(
                    &busy.fx.service.plan_store,
                    &busy.plan_id,
                    &busy.task_id,
                    source,
                    tab,
                    Some(barrier),
                )
            })
        });
        workers.map(|worker| worker.join().unwrap())
    });
    assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
    assert!(outcomes.iter().any(|outcome| matches!(
        outcome,
        Err(PlanError::DispatchInFlight { .. }) | Err(PlanError::RespawnAttemptNotCurrent { .. })
    )));
    let task = busy
        .fx
        .service
        .plan_store
        .show_task(&busy.task_id)
        .unwrap()
        .task;
    assert_eq!(task.execution_attempts.len(), 2);
    assert_eq!(
        task.execution_attempts
            .iter()
            .filter(|attempt| attempt.closed_at.is_none())
            .count(),
        1
    );
    assert!(outcomes.iter().any(|outcome| outcome
        .as_ref()
        .is_ok_and(|id| id == &task.execution_attempts.last().unwrap().id)));
}

#[test]
fn delayed_loss_notice_uses_latest_closed_replacement_as_recovery_source() {
    let busy = seed_busy_worker();
    let source = close_and_read_advised_source(&busy);
    let store = &busy.fx.service.plan_store;
    let replacement =
        advised_respawn(store, &busy.plan_id, &busy.task_id, &source, "next", None).unwrap();
    store
        .flag_tab_gone(&busy.plan_id, &busy.task_id, "next", 30)
        .unwrap();
    let gateway = MessageGateway::new();
    let seam = NoticeSeam::default();
    let replayed = replay_losses(
        &gateway,
        &seam,
        pending_worker_losses(),
        &busy.orchestrator,
        |loss| busy.fx.service.refresh_worker_loss_recovery(loss),
    );
    assert_eq!(replayed.len(), 1);
    let (loss, route) = &replayed[0];
    assert_eq!(*route, LossRoute::Delivered);
    assert_eq!(
        loss.recovery,
        recovery_command(&busy.plan_id, &busy.task_id, Some(&replacement))
    );
    advised_respawn(
        store,
        &busy.plan_id,
        &busy.task_id,
        &replacement,
        "recovered",
        None,
    )
    .expect("a delayed loss notice must name a source that still accepts recovery");
}

#[test]
fn delayed_loss_notice_names_live_replacement_without_advising_respawn() {
    let busy = seed_busy_worker();
    let source = close_and_read_advised_source(&busy);
    let replacement = advised_respawn(
        &busy.fx.service.plan_store,
        &busy.plan_id,
        &busy.task_id,
        &source,
        "next",
        None,
    )
    .unwrap();
    let replayed = replay_losses(
        &MessageGateway::new(),
        &NoticeSeam::default(),
        pending_worker_losses(),
        &busy.orchestrator,
        |loss| busy.fx.service.refresh_worker_loss_recovery(loss),
    );
    assert_eq!(replayed.len(), 1);
    let (loss, route) = &replayed[0];
    assert_eq!(*route, LossRoute::Delivered);
    assert!(loss.notice_text().contains(&replacement));
    assert!(loss.notice_text().contains("next"));
    assert!(!loss.notice_text().contains("codeterm plan task respawn"));
}
