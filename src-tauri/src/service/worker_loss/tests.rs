use std::path::Path;
use std::process::Command;

use super::*;
use crate::api::events::SystemEvent;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, TaskExecutionAttempt};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

struct BusyWorker {
    fx: ServiceFixture,
    orchestrator: String,
    worker: String,
    plan_id: String,
    task_id: String,
    worktree: String,
    _lane_dir: tempfile::TempDir,
}

fn git(cwd: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let status = command.current_dir(cwd).args(args).status().unwrap();
    assert!(status.success(), "git {args:?} failed");
}

fn dirty_lane() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init"]);
    std::fs::write(dir.path().join("uncommitted.txt"), "work in flight\n").unwrap();
    dir
}

fn seed_busy_worker() -> BusyWorker {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    let fx = ServiceFixture::new(mux);
    let lane_dir = dirty_lane();
    let worktree = lane_dir.path().display().to_string();
    let mut plan = crate::plan::Plan::new(
        "Worker loss".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&orchestrator),
    );
    fx.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fx
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Lost work".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fx.service
        .plan_store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.assigned_tab_id = Some(worker.clone());
            task.lane.tab_id = Some(worker.clone());
            task.lane.worktree_path = Some(worktree.clone());
            task.lane.branch = Some("lane/lost-work".into());
            task.execution_attempts.push(TaskExecutionAttempt {
                id: "attempt_lost".into(),
                executor_tab_id: worker.clone(),
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    BusyWorker {
        fx,
        orchestrator,
        worker,
        plan_id: plan.id,
        task_id,
        worktree,
        _lane_dir: lane_dir,
    }
}

fn worker_lost_events(
    receiver: &mut tokio::sync::broadcast::Receiver<crate::api::events::SeqEvent>,
) -> Vec<serde_json::Value> {
    let mut events = Vec::new();
    while let Ok(event) = receiver.try_recv() {
        if let SystemEvent::Generic { event_type, data } = event.event {
            if event_type == WORKER_LOST_EVENT {
                events.push(data);
            }
        }
    }
    events
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn worker_tab_death_pushes_orchestrator_event() {
    let busy = seed_busy_worker();
    let mut receiver = busy.fx.service.events.subscribe();

    busy.fx.service.close_tab(&busy.worker).unwrap();
    busy.fx
        .service
        .flag_bound_tasks_for_gone_tab(&busy.worker, 10);

    let events = worker_lost_events(&mut receiver);
    assert_eq!(events.len(), 1, "one event per lost assignment: {events:?}");
    let event = &events[0];
    assert_eq!(event["plan_id"], busy.plan_id.as_str());
    assert_eq!(event["task_id"], busy.task_id.as_str());
    assert_eq!(event["worker_tab_id"], busy.worker.as_str());
    assert_eq!(event["orchestrator_tab_id"], busy.orchestrator.as_str());
    assert_eq!(event["worktree_path"], busy.worktree.as_str());
    assert_eq!(event["branch"], "lane/lost-work");
    assert_eq!(event["lane"]["state"], "dirty");
    assert_eq!(event["lane"]["changes"], 1);
    assert_eq!(event["cause"], "tab_gone");
    assert_eq!(event["delivery"], "delivered");
    assert_eq!(
        event["recovery"],
        recovery_command(&busy.plan_id, &busy.task_id, Some("attempt_lost")).as_str()
    );
    let task = busy
        .fx
        .service
        .plan_store
        .show_task(&busy.task_id)
        .unwrap()
        .task;
    assert_eq!(task.assigned_tab_id, None);
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
)]
fn agent_close_of_busy_worker_needs_force() {
    let busy = seed_busy_worker();
    let mut receiver = busy.fx.service.events.subscribe();

    let error = busy
        .fx
        .service
        .close_tab_guarded(Some(&busy.orchestrator), &busy.worker, false)
        .unwrap_err();
    assert!(error.starts_with(BUSY_WORKER_CLOSE_REFUSAL_PREFIX));
    assert!(error.contains(&busy.task_id));
    assert!(busy.fx.service.mux.read().get_tab(&busy.worker).is_some());
    assert!(worker_lost_events(&mut receiver).is_empty());

    let warnings = busy
        .fx
        .service
        .close_tab_guarded(Some(&busy.orchestrator), &busy.worker, true)
        .unwrap();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains(&busy.task_id));
    assert!(busy.fx.service.mux.read().get_tab(&busy.worker).is_none());
    assert_eq!(worker_lost_events(&mut receiver).len(), 1);
}

fn plan_with_task(status: TaskStatus, owner: &str, worker: &str) -> (Plan, String) {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::service::plan::PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Pure".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(owner),
    );
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Pure task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    let task = plan
        .tasks
        .iter_mut()
        .find(|task| task.id == task_id)
        .unwrap();
    task.status = status;
    task.assigned_tab_id = Some(worker.into());
    task.lane.worktree_path = Some("/lanes/pure".into());
    task.execution_attempts.push(TaskExecutionAttempt {
        id: "attempt_pure".into(),
        executor_tab_id: worker.into(),
        branch: Some("lane/pure".into()),
        ..Default::default()
    });
    (plan, task_id)
}

#[test]
fn in_progress_losses_name_task_lane_orchestrator_and_recovery() {
    let (plan, task_id) = plan_with_task(TaskStatus::InProgress, "orch-tab", "worker-tab");

    let losses = in_progress_losses(
        std::slice::from_ref(&plan),
        "worker-tab",
        WorkerLossCause::TabGone,
    );

    assert_eq!(losses.len(), 1);
    let loss = &losses[0];
    assert_eq!(loss.task_id, task_id);
    assert_eq!(loss.orchestrator_tab_id.as_deref(), Some("orch-tab"));
    assert_eq!(loss.worktree_path.as_deref(), Some("/lanes/pure"));
    assert_eq!(loss.branch.as_deref(), Some("lane/pure"));
    assert_eq!(loss.attempt_id.as_deref(), Some("attempt_pure"));
    assert_eq!(
        loss.recovery,
        recovery_command(&plan.id, &task_id, Some("attempt_pure"))
    );
    assert!(in_progress_losses(
        std::slice::from_ref(&plan),
        "other-tab",
        WorkerLossCause::TabGone
    )
    .is_empty());
    let (review, _) = plan_with_task(TaskStatus::InReview, "orch-tab", "worker-tab");
    assert!(in_progress_losses(&[review], "worker-tab", WorkerLossCause::TabGone).is_empty());
}

#[test]
fn loss_message_targets_the_orchestrator_once_per_attempt() {
    let (plan, task_id) = plan_with_task(TaskStatus::InProgress, "orch-tab", "worker-tab");
    let loss = in_progress_losses(&[plan], "worker-tab", WorkerLossCause::TabGone).remove(0);

    let message = loss.outbound_message("orch-tab");
    let again = loss.outbound_message("orch-tab");

    assert_eq!(message.target, "orch-tab");
    assert_eq!(message.class, DeliveryClass::SystemNotice);
    assert!(message.idempotency_key.is_some());
    assert_eq!(message.idempotency_key, again.idempotency_key);
    let MessageBody::Text { body } = message.body else {
        panic!("worker loss is a text notice");
    };
    for value in [
        task_id.as_str(),
        "worker-tab",
        "/lanes/pure",
        "lane/pure",
        loss.recovery.as_str(),
    ] {
        assert!(body.contains(value), "notice carries {value}");
    }
}

#[test]
fn lane_state_distinguishes_missing_and_unrecorded_lanes() {
    assert_eq!(probe_lane_state(None), LaneState::NoLane);
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone");
    assert_eq!(
        probe_lane_state(Some(missing.to_str().unwrap())),
        LaneState::Missing
    );
}
