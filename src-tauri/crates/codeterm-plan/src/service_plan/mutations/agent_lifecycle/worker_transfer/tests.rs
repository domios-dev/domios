use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt, TaskLaneMetadata};
use crate::service_plan::NewTask;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

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

fn second_plan_with_unrelated_lane(
    directory: &tempfile::TempDir,
    store: &PlanStore,
) -> (String, String) {
    let mut plan = Plan::new(
        "unrelated transfer plan".into(),
        directory.path().display().to_string(),
        OrchestratorRef::from_tab("other-owner"),
    );
    plan.team_id = Some("other-team".into());
    store.save(&mut plan, 5).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "unrelated assigned task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            6,
        )
        .unwrap();
    let plan_id = plan.id;
    let worktree_path = directory.path().join("other-lane").display().to_string();
    store
        .with_plan(&plan_id, 7, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::InProgress;
            task.lane = TaskLaneMetadata {
                tab_id: Some("other-worker".into()),
                workspace_id: Some("other-workspace".into()),
                worktree_path: Some(worktree_path),
                branch: Some("lane/other-worker".into()),
                base: Some("other-base".into()),
                ..Default::default()
            };
            Ok(())
        })
        .unwrap();
    (plan_id, task_id)
}

fn assert_cross_plan_ownership_duplicate_refused(
    mutate_second_task: impl FnOnce(&mut Plan, &VerifiedWorkerTransferBinding, &str),
) {
    let (directory, store, binding) = fixture();
    let (second_plan_id, second_task_id) = second_plan_with_unrelated_lane(&directory, &store);
    let first_positive_record = store.get(&binding.plan_id).unwrap();
    let second_positive_record = store.get(&second_plan_id).unwrap();
    let positive_callback = AtomicBool::new(false);

    store
        .with_verified_worker_transfer_binding(&binding, || {
            positive_callback.store(true, Ordering::Release);
        })
        .unwrap();

    assert!(positive_callback.load(Ordering::Acquire));
    assert_eq!(store.get(&binding.plan_id).unwrap(), first_positive_record);
    assert_eq!(store.get(&second_plan_id).unwrap(), second_positive_record);
    store
        .with_plan(&second_plan_id, 8, |plan| {
            mutate_second_task(plan, &binding, &second_task_id);
            Ok(())
        })
        .unwrap();

    let first_before_refusal = store.get(&binding.plan_id).unwrap();
    let second_before_refusal = store.get(&second_plan_id).unwrap();
    let refusal_callback = AtomicBool::new(false);
    assert!(store
        .with_verified_worker_transfer_binding(&binding, || {
            refusal_callback.store(true, Ordering::Release);
        })
        .is_err());

    assert!(!refusal_callback.load(Ordering::Acquire));
    assert_eq!(store.get(&binding.plan_id).unwrap(), first_before_refusal);
    assert_eq!(store.get(&second_plan_id).unwrap(), second_before_refusal);
}

fn assert_fence_refusal_preserves_plan(
    mutate_plan: impl FnOnce(&mut Plan, &VerifiedWorkerTransferBinding),
) {
    let (_directory, store, binding) = fixture();
    store
        .with_plan(&binding.plan_id, 8, |plan| {
            mutate_plan(plan, &binding);
            Ok(())
        })
        .unwrap();
    let before = store.get(&binding.plan_id).unwrap();
    let callback_called = AtomicBool::new(false);

    assert!(store
        .with_verified_worker_transfer_binding(&binding, || {
            callback_called.store(true, Ordering::Release);
        })
        .is_err());

    assert!(!callback_called.load(Ordering::Acquire));
    assert_eq!(store.get(&binding.plan_id).unwrap(), before);
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
fn transfer_fence_refuses_duplicate_assignment_in_another_plan() {
    assert_cross_plan_ownership_duplicate_refused(|plan, binding, task_id| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap()
            .assigned_tab_id = Some(binding.worker_tab_id.clone());
    });
}

#[test]
fn transfer_fence_refuses_duplicate_lane_tab_in_another_plan() {
    assert_cross_plan_ownership_duplicate_refused(|plan, binding, task_id| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap()
            .lane
            .tab_id = Some(binding.worker_tab_id.clone());
    });
}

#[test]
fn transfer_fence_refuses_duplicate_worktree_in_another_plan() {
    assert_cross_plan_ownership_duplicate_refused(|plan, binding, task_id| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap()
            .lane
            .worktree_path = Some(binding.worktree_path.clone());
    });
}

#[test]
fn transfer_fence_refuses_duplicate_open_attempt_in_another_plan() {
    assert_cross_plan_ownership_duplicate_refused(|plan, binding, task_id| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == task_id)
            .unwrap()
            .execution_attempts
            .push(TaskExecutionAttempt {
                id: "other-open-attempt".into(),
                executor_tab_id: binding.worker_tab_id.clone(),
                ..Default::default()
            });
    });
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
fn transfer_fence_refuses_closed_attempt_without_plan_changes() {
    assert_fence_refusal_preserves_plan(|plan, binding| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == binding.task_id)
            .unwrap()
            .execution_attempts
            .iter_mut()
            .find(|attempt| attempt.id == binding.attempt_id)
            .unwrap()
            .closed_at = Some(9);
    });
}

#[test]
fn transfer_fence_refuses_retired_attempt_without_plan_changes() {
    assert_fence_refusal_preserves_plan(|plan, binding| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == binding.task_id)
            .unwrap()
            .execution_attempts
            .iter_mut()
            .find(|attempt| attempt.id == binding.attempt_id)
            .unwrap()
            .retired_at = Some(9);
    });
}

#[test]
fn transfer_fence_refuses_wrong_owner_epoch_without_plan_changes() {
    assert_fence_refusal_preserves_plan(|plan, binding| {
        plan.tasks
            .iter_mut()
            .find(|task| task.id == binding.task_id)
            .unwrap()
            .interruption
            .owner_epoch += 1;
    });
}

#[test]
fn transfer_fence_refuses_wrong_plan_owner_without_plan_changes() {
    assert_fence_refusal_preserves_plan(|plan, _binding| {
        plan.owner.tab_id = Some("other-owner".into());
    });
}

#[test]
fn transfer_fence_refuses_wrong_plan_mode_without_plan_changes() {
    assert_fence_refusal_preserves_plan(|plan, _binding| {
        plan.mode = PlanMode::Planning;
    });
}

#[test]
fn transfer_fence_serializes_plan_writers_behind_its_mux_commit_callback() {
    let (_directory, store, binding) = fixture();
    let store = Arc::new(store);
    let plan_id = binding.plan_id.clone();
    let before = store.get(&plan_id).unwrap();
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

    let writer_ticket = {
        let state = store.mutation_writer.state.lock();
        state.next_ticket
    };
    let (writer_entered_tx, writer_entered_rx) = mpsc::channel();
    let (writer_done_tx, writer_done_rx) = mpsc::channel();
    let writer_store = store.clone();
    let writer_plan_id = plan_id.clone();
    let writer = std::thread::spawn(move || {
        writer_store
            .with_plan(&writer_plan_id, 8, |plan| {
                writer_entered_tx.send(()).unwrap();
                plan.title = "after transfer fence".into();
                Ok(())
            })
            .unwrap();
        writer_done_tx.send(()).unwrap();
    });

    {
        let mut state = store.mutation_writer.state.lock();
        while state.next_ticket <= writer_ticket {
            store.mutation_writer.ready.wait(&mut state);
        }
        assert_eq!(state.next_ticket, writer_ticket + 1);
        assert!(state.serving_ticket < writer_ticket);
    }
    assert!(writer_entered_rx.try_recv().is_err());
    assert_eq!(store.get(&plan_id).unwrap(), before);
    callback_release_tx.send(()).unwrap();

    writer_entered_rx.recv().unwrap();
    writer_done_rx.recv().unwrap();
    transfer.join().unwrap().unwrap();
    writer.join().unwrap();
    assert_eq!(store.get(&plan_id).unwrap().title, "after transfer fence");
}
