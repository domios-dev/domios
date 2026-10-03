use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt, TaskLaneMetadata};
use crate::service_plan::NewTask;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Duration;

fn fixture() -> (tempfile::TempDir, PlanStore, VerifiedWorkerTransferBinding) {
    let directory = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(directory.path());
    let mut plan = Plan::new(
        "transfer fence".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("destination"),
    );
    plan.team_id = Some("destination-team".into());
    plan.mode = PlanMode::Execution;
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "assigned task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let worktree_path = directory.path().join("worker-lane").display().to_string();
    store
        .with_plan(&plan.id, 3, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.assigned_tab_id = Some("worker".into());
            task.lane = TaskLaneMetadata {
                tab_id: Some("worker".into()),
                workspace_id: Some("workspace".into()),
                worktree_path: Some(worktree_path.clone()),
                branch: Some("lane/transfer".into()),
                base: Some("base".into()),
                ..Default::default()
            };
            task.interruption.owner_epoch = 7;
            task.interruption.owner_tab_id = Some("worker".into());
            task.interruption.owner_session_id = Some("worker-session".into());
            task.execution_attempts.push(TaskExecutionAttempt {
                id: "attempt".into(),
                executor_tab_id: "worker".into(),
                provider: Some("codex".into()),
                agent_type: Some("codex".into()),
                session_id: Some("worker-session".into()),
                session_binding_nonce: Some("worker-nonce".into()),
                worktree_path: Some(worktree_path.clone()),
                branch: Some("lane/transfer".into()),
                base: Some("base".into()),
                acknowledged_at: Some(4),
                interruption: codeterm_types::plan::AttemptInterruptionState {
                    owner_epoch: 7,
                    ..Default::default()
                },
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let binding = VerifiedWorkerTransferBinding {
        plan_id: plan.id,
        team_id: "destination-team".into(),
        owner_tab_id: "destination".into(),
        owner_session_id: "destination-session".into(),
        task_id,
        attempt_id: "attempt".into(),
        worker_tab_id: "worker".into(),
        workspace_id: "workspace".into(),
        worktree_path,
        session_id: "worker-session".into(),
        binding_nonce: "worker-nonce".into(),
        provider: "codex".into(),
        owner_epoch: 7,
    };
    (directory, store, binding)
}

#[test]
fn transfer_fence_commits_only_mux_and_preserves_the_whole_plan_record() {
    let (_directory, store, binding) = fixture();
    let before = store.get(&binding.plan_id).unwrap();
    let committed = AtomicBool::new(false);

    store
        .with_verified_worker_transfer_binding(&binding, || {
            committed.store(true, Ordering::Release);
        })
        .unwrap();

    assert!(committed.load(Ordering::Acquire));
    assert_eq!(store.get(&binding.plan_id).unwrap(), before);
}

#[test]
fn transfer_fence_refuses_duplicate_assignment_and_lane_without_committing() {
    let (_directory, store, binding) = fixture();
    store
        .add_task(
            &binding.plan_id,
            NewTask {
                name: "duplicate lane".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            5,
        )
        .unwrap();
    store
        .with_plan(&binding.plan_id, 6, |plan| {
            let duplicate = plan.tasks.last_mut().unwrap();
            duplicate.status = TaskStatus::InProgress;
            duplicate.assigned_tab_id = Some(binding.worker_tab_id.clone());
            duplicate.lane.tab_id = Some(binding.worker_tab_id.clone());
            duplicate.lane.workspace_id = Some(binding.workspace_id.clone());
            duplicate.lane.worktree_path = Some(binding.worktree_path.clone());
            duplicate.execution_attempts.push(TaskExecutionAttempt {
                id: "duplicate-attempt".into(),
                executor_tab_id: binding.worker_tab_id.clone(),
                closed_at: None,
                ..Default::default()
            });
            Ok(())
        })
        .unwrap();
    let before = store.get(&binding.plan_id).unwrap();
    let committed = AtomicBool::new(false);

    let result = store.with_verified_worker_transfer_binding(&binding, || {
        committed.store(true, Ordering::Release);
    });

    assert!(result.is_err());
    assert!(!committed.load(Ordering::Acquire));
    assert_eq!(store.get(&binding.plan_id).unwrap(), before);
}

#[test]
fn transfer_fence_refuses_a_stale_worker_nonce_without_plan_changes() {
    let (_directory, store, binding) = fixture();
    let before = store.get(&binding.plan_id).unwrap();
    let mut stale = binding;
    stale.binding_nonce = "stale-nonce".into();
    let committed = AtomicBool::new(false);

    assert!(store
        .with_verified_worker_transfer_binding(&stale, || {
            committed.store(true, Ordering::Release);
        })
        .is_err());

    assert!(!committed.load(Ordering::Acquire));
    assert_eq!(store.get(&stale.plan_id).unwrap(), before);
}

#[test]
fn transfer_fence_serializes_plan_writers_behind_its_mux_commit_callback() {
    let (_directory, store, binding) = fixture();
    let store = Arc::new(store);
    let (callback_entered_tx, callback_entered_rx) = mpsc::sync_channel(0);
    let (callback_release_tx, callback_release_rx) = mpsc::sync_channel(0);
    let transfer_store = store.clone();
    let transfer = std::thread::spawn(move || {
        transfer_store.with_verified_worker_transfer_binding(&binding, || {
            callback_entered_tx.send(()).unwrap();
            callback_release_rx.recv().unwrap();
        })
    });
    callback_entered_rx.recv().unwrap();

    let (writer_started_tx, writer_started_rx) = mpsc::sync_channel(0);
    let (writer_done_tx, writer_done_rx) = mpsc::sync_channel(0);
    let writer_store = store.clone();
    let plan_id = store.list_full()[0].id.clone();
    let writer = std::thread::spawn(move || {
        writer_started_tx.send(()).unwrap();
        writer_store
            .with_plan(&plan_id, 8, |plan| {
                plan.title = "after transfer fence".into();
                Ok(())
            })
            .unwrap();
        writer_done_tx.send(()).unwrap();
    });
    writer_started_rx.recv().unwrap();
    let writer_was_blocked = writer_done_rx
        .recv_timeout(Duration::from_millis(50))
        .is_err();
    callback_release_tx.send(()).unwrap();

    transfer.join().unwrap().unwrap();
    writer.join().unwrap();
    assert!(writer_was_blocked);
    assert_eq!(
        store.get(&store.list_full()[0].id).unwrap().title,
        "after transfer fence"
    );
}
