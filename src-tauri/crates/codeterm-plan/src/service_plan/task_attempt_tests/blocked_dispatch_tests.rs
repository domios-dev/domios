use super::*;
use crate::plan::ExecutionIntent;

#[test]
fn blocked_respawn_draft_and_pending_attempt_agree_on_execution_intent() {
    use ExecutionIntent::{Mutating, ReadOnly, Unspecified};

    for (task_default, source_intent, effective, persisted) in [
        (Mutating, Some(ReadOnly), ReadOnly, Some(ReadOnly)),
        (ReadOnly, Some(Mutating), Mutating, Some(Mutating)),
        (Mutating, Some(Unspecified), Unspecified, Some(Unspecified)),
        (Mutating, None, Mutating, Some(Mutating)),
        (ReadOnly, None, ReadOnly, Some(ReadOnly)),
        (Unspecified, None, Unspecified, None),
    ] {
        let (_dir, store, plan_id, task_id) = bound_report_store();
        let source_id = store.list_task_attempts(&plan_id, &task_id).unwrap()[0]
            .attempt_id
            .clone();
        store
            .edit_plan_fixture(&plan_id, 4, |plan| {
                let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id).unwrap();
                task.status = TaskStatus::Blocked;
                task.status_label = Some("awaiting dependency evidence".into());
                task.interruption.execution_intent = task_default.clone();
                task.execution_attempts[0].execution_intent = source_intent.clone();
                Ok(())
            })
            .unwrap();
        let draft = store
            .build_task_respawn_draft(
                &plan_id,
                &task_id,
                task_attempts::TaskRespawnRequest {
                    attempt_id: source_id.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(draft.source_attempt.id, source_id);
        assert_eq!(draft.source_attempt.execution_intent, source_intent);
        assert_eq!(draft.execution_intent, effective);

        let receipt = store
            .record_task_respawn_attempt(
                &plan_id,
                &task_id,
                "replacement",
                &draft,
                Some("worker"),
                5,
            )
            .unwrap();
        let pending = store.inspect_task_for_test(&task_id).unwrap().task;
        assert_eq!(pending.status, TaskStatus::Pending);
        assert_eq!(pending.status_label.as_deref(), Some("starting"));
        assert_eq!(pending.execution_attempts[0].id, source_id);
        assert!(pending.execution_attempts[0].retired_at.is_some());
        assert_eq!(pending.execution_attempts[1].id, receipt.attempt.id);
        assert_eq!(pending.execution_attempts[1].execution_intent, persisted);
        assert_eq!(receipt.attempt.execution_intent, persisted);
    }
}

#[test]
fn failed_blocked_respawn_restores_reason_and_marks_unblock_stride_rolled_back() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let block_reason = "awaiting dependency evidence";
    store
        .edit_plan_fixture(&plan_id, 4, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id).unwrap();
            task.status = TaskStatus::Blocked;
            task.status_label = Some(block_reason.into());
            task.status_changed_at = 4;
            Ok(())
        })
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let snapshot = crate::plan::task_key::find_task(&plan.tasks, &task_id)
        .unwrap()
        .clone();
    let draft = store
        .build_task_respawn_draft(
            &plan_id,
            &task_id,
            task_attempts::TaskRespawnRequest {
                attempt_id: snapshot.execution_attempts[0].id.clone(),
                ..Default::default()
            },
        )
        .unwrap();
    let (replacement, undo) = store
        .record_task_respawn_attempt_with_undo(
            &plan_id,
            &task_id,
            "worker-failed",
            &draft,
            Some("worker"),
            5,
        )
        .unwrap();

    store
        .edit_plan_fixture(&plan_id, 5, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.body = "concurrent respawn edit".into();
            Ok(())
        })
        .unwrap();
    let rollback = store
        .roll_back_task_dispatch_attempt(&undo, "worker-failed", &replacement.attempt.id, 6)
        .unwrap();

    assert_eq!(rollback, super::super::DispatchRollback::Restored);
    let restored = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(restored.body, "concurrent respawn edit");
    assert_eq!(restored.status, TaskStatus::Blocked);
    assert_eq!(restored.status_label.as_deref(), Some(block_reason));
    assert!(store.get(&plan_id).unwrap().strides.iter().any(|stride| {
        stride.links.iter().any(|link| link == &task_id)
            && stride
                .text
                .contains("startup rolled back; task remains blocked")
            && stride.text.contains(block_reason)
    }));
}

#[test]
fn failed_blocked_dispatch_restores_reason_and_marks_unblock_stride_rolled_back() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let block_reason = "awaiting dependency evidence";
    store
        .edit_plan_fixture(&plan_id, 4, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = TaskStatus::Blocked;
            task.status_label = Some(block_reason.into());
            task.status_changed_at = 4;
            Ok(())
        })
        .unwrap();
    let plan = store.get(&plan_id).unwrap();
    let snapshot = crate::plan::task_key::find_task(&plan.tasks, &task_id)
        .unwrap()
        .clone();
    let started = store
        .begin_task_dispatch_with_undo(
            &plan_id,
            &task_id,
            "worker-failed",
            "Worker",
            snapshot.lane.clone(),
            5,
        )
        .unwrap();

    let outcome = store
        .roll_back_task_dispatch_attempt(&started.undo, "worker-failed", &started.attempt_id, 6)
        .unwrap();

    assert_eq!(outcome, super::super::DispatchRollback::Restored);
    let restored = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(restored.status, TaskStatus::Blocked);
    assert_eq!(restored.status_label.as_deref(), Some(block_reason));
    assert!(store.get(&plan_id).unwrap().strides.iter().any(|stride| {
        stride.links.iter().any(|link| link == &task_id)
            && stride
                .text
                .contains("startup rolled back; task remains blocked")
            && stride.text.contains(block_reason)
    }));
}

#[test]
fn reverted_blocked_dispatch_restores_reason_and_annotates_unblock_stride() {
    let (_dir, store, plan_id, task_id) = bound_report_store();
    let block_reason = "awaiting dependency evidence";
    store
        .edit_plan_fixture(&plan_id, 4, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, &task_id).unwrap();
            task.status = TaskStatus::Blocked;
            task.status_label = Some(block_reason.into());
            task.status_changed_at = 4;
            Ok(())
        })
        .unwrap();

    store
        .begin_task_dispatch(
            &plan_id,
            &task_id,
            "worker-failed",
            "Worker",
            TaskLaneMetadata::default(),
            5,
        )
        .unwrap();
    store.revert_task_dispatch(&plan_id, &task_id, 6).unwrap();

    let restored = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(restored.status, TaskStatus::Blocked);
    assert_eq!(restored.status_label.as_deref(), Some(block_reason));
    assert!(store.get(&plan_id).unwrap().strides.iter().any(|stride| {
        stride.links.iter().any(|link| link == &task_id)
            && stride
                .text
                .starts_with("dispatch rolled back; task remains blocked:")
            && stride.text.contains(block_reason)
    }));
}
