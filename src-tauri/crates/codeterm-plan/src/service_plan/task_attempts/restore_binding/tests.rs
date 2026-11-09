use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanSkillManifestEntry};
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;
use crate::service_plan::NewTask;
use std::sync::{Arc, Barrier};

const OLD_NONCE: &str = "nonce-before-restore";
const CURRENT_NONCE: &str = "nonce-after-restore";
const SESSION_ID: &str = "restore-session";
const TAB_ID: &str = "restore-worker";
const PROVIDER: &str = "codex";
const TEAM_ID: &str = "restore-team";

struct Fixture {
    _dir: tempfile::TempDir,
    store: PlanStore,
    plan_id: String,
    task_id: String,
    attempt_id: String,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "Attempt binding restoration".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orchestrator"),
    );
    plan.mode = PlanMode::Execution;
    plan.team_id = Some(TEAM_ID.into());
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Preserve the acknowledged attempt".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            TAB_ID,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: SESSION_ID.into(),
                session_binding_nonce: OLD_NONCE.into(),
                provider: Some(PROVIDER.into()),
                model: None,
                session_model: None,
                reasoning_effort: None,
            }),
            None,
            3,
        )
        .unwrap();
    let attempt_id = store
        .inspect_task_for_test(&task_id)
        .unwrap()
        .task
        .execution_attempts[0]
        .id
        .clone();
    store
        .acknowledge_task_attempt(&plan.id, &task_id, TAB_ID, &attempt_id, 4)
        .unwrap();
    store
        .set_attempt_plan_skill_manifest(
            &plan.id,
            &task_id,
            &attempt_id,
            vec![PlanSkillManifestEntry {
                slug: "restore-proof".into(),
                title: "Restore proof".into(),
                purpose: "preserve loaded history".into(),
                content_digest: "sha256:skill".into(),
            }],
            5,
        )
        .unwrap();
    store
        .record_plan_skill_load(
            &plan.id,
            &task_id,
            TAB_ID,
            SESSION_ID,
            OLD_NONCE,
            Some(&attempt_id),
            "restore-proof",
            "sha256:skill",
            6,
        )
        .unwrap();
    Fixture {
        _dir: dir,
        store,
        plan_id: plan.id,
        task_id,
        attempt_id,
    }
}

fn request(fixture: &Fixture, now_ms: u64) -> RestoreCurrentAttemptBinding {
    let task = &fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    RestoreCurrentAttemptBinding {
        plan_id: fixture.plan_id.clone(),
        task_id: fixture.task_id.clone(),
        attempt_id: fixture.attempt_id.clone(),
        executor_tab_id: TAB_ID.into(),
        session_id: SESSION_ID.into(),
        provider: PROVIDER.into(),
        team_id: TEAM_ID.into(),
        owner_epoch: task.interruption.owner_epoch,
        expected_old_nonce: OLD_NONCE.into(),
        current_nonce: CURRENT_NONCE.into(),
        receipt: AttemptBindingRestorationReceipt {
            operation_id: "restore-op-1".into(),
            restored_at: now_ms,
            attempt_id: fixture.attempt_id.clone(),
            executor_tab_id: TAB_ID.into(),
            session_id: SESSION_ID.into(),
            provider: PROVIDER.into(),
            owner_epoch: task.interruption.owner_epoch,
            old_nonce_digest: nonce_digest(OLD_NONCE),
            current_nonce_digest: nonce_digest(CURRENT_NONCE),
            lineage_proof_kind: "prior_session_binding_nonce_suffix".into(),
            lineage_proof_digest: format!("sha256:{}", "a".repeat(64)),
        },
    }
}

#[test]
fn attempt_binding_restore_changes_only_nonce_and_receipt() {
    let fixture = fixture();
    let before = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    let request = request(&fixture, 7);
    let result = fixture
        .store
        .restore_current_attempt_binding(request.clone(), 7)
        .unwrap();
    assert_eq!(
        result,
        RestoreCurrentAttemptBindingResult::Restored(request.receipt.clone())
    );

    let mut expected = before.clone();
    let expected_attempt = expected
        .execution_attempts
        .iter_mut()
        .find(|attempt| attempt.id == fixture.attempt_id)
        .unwrap();
    expected_attempt.session_binding_nonce = Some(CURRENT_NONCE.into());
    expected_attempt
        .binding_restorations
        .push(request.receipt.clone());
    let actual = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    assert_eq!(actual, expected);
    assert_eq!(actual.execution_attempts[0].acknowledged_at, Some(4));
    assert_eq!(
        actual.execution_attempts[0].loaded_plan_skills[0].loaded_at,
        6
    );
    assert!(fixture
        .store
        .record_plan_skill_load(
            &fixture.plan_id,
            &fixture.task_id,
            TAB_ID,
            SESSION_ID,
            OLD_NONCE,
            Some(&fixture.attempt_id),
            "restore-proof",
            "sha256:skill",
            8,
        )
        .is_err());
    fixture
        .store
        .record_plan_skill_load(
            &fixture.plan_id,
            &fixture.task_id,
            TAB_ID,
            SESSION_ID,
            CURRENT_NONCE,
            Some(&fixture.attempt_id),
            "restore-proof",
            "sha256:skill",
            9,
        )
        .unwrap();
    assert_eq!(
        fixture
            .store
            .inspect_task_for_test(&fixture.task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .loaded_plan_skills[0]
            .loaded_at,
        6
    );
}

#[test]
fn attempt_binding_restore_current_replay_is_a_noop() {
    let fixture = fixture();
    let initial_request = request(&fixture, 7);
    fixture
        .store
        .restore_current_attempt_binding(initial_request, 7)
        .unwrap();
    let before = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    let operation_count = fixture
        .store
        .operation_count_for_test(Some(&fixture.plan_id));
    assert_eq!(
        fixture
            .store
            .restore_current_attempt_binding(request(&fixture, 8), 8)
            .unwrap(),
        RestoreCurrentAttemptBindingResult::AlreadyCurrent
    );
    assert_eq!(
        fixture
            .store
            .inspect_task_for_test(&fixture.task_id)
            .unwrap()
            .task,
        before
    );
    assert_eq!(
        fixture
            .store
            .operation_count_for_test(Some(&fixture.plan_id)),
        operation_count
    );
}

#[test]
fn attempt_binding_restore_concurrent_duplicate_rotates_once() {
    let fixture = fixture();
    let request = request(&fixture, 7);
    let task_id = fixture.task_id.clone();
    let store = Arc::new(fixture.store);
    let barrier = Arc::new(Barrier::new(3));
    let workers = (0..2)
        .map(|_| {
            let store = store.clone();
            let barrier = barrier.clone();
            let request = request.clone();
            std::thread::spawn(move || {
                barrier.wait();
                store.restore_current_attempt_binding(request, 7).unwrap()
            })
        })
        .collect::<Vec<_>>();
    barrier.wait();
    let results = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, RestoreCurrentAttemptBindingResult::Restored(_)))
            .count(),
        1
    );
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, RestoreCurrentAttemptBindingResult::AlreadyCurrent))
            .count(),
        1
    );
    let task = store.inspect_task_for_test(&task_id).unwrap().task;
    assert_eq!(
        task.execution_attempts[0].session_binding_nonce.as_deref(),
        Some(CURRENT_NONCE)
    );
    assert_eq!(task.execution_attempts[0].binding_restorations.len(), 1);
}

#[test]
fn attempt_binding_restore_close_race_serializes_without_resurrecting_a_closed_attempt() {
    let fixture = fixture();
    let task_id = fixture.task_id.clone();
    let plan_id = fixture.plan_id.clone();
    let restore_request = request(&fixture, 7);
    let store = Arc::new(fixture.store);
    let barrier = Arc::new(Barrier::new(3));
    let restore_store = store.clone();
    let restore_barrier = barrier.clone();
    let restore = std::thread::spawn(move || {
        restore_barrier.wait();
        restore_store.restore_current_attempt_binding(restore_request, 7)
    });
    let close_store = store.clone();
    let close_barrier = barrier.clone();
    let close = std::thread::spawn(move || {
        close_barrier.wait();
        close_store.edit_plan_fixture(&plan_id, 8, |plan| {
            plan.tasks[0].execution_attempts[0].closed_at = Some(8);
            Ok(())
        })
    });
    barrier.wait();
    let restore_result = restore.join().unwrap();
    close.join().unwrap().unwrap();
    let task = store.inspect_task_for_test(&task_id).unwrap().task;
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.closed_at, Some(8));
    match restore_result {
        Ok(RestoreCurrentAttemptBindingResult::Restored(_)) => {
            assert_eq!(
                attempt.session_binding_nonce.as_deref(),
                Some(CURRENT_NONCE)
            );
            assert_eq!(attempt.binding_restorations.len(), 1);
        }
        Err(_) => {
            assert_eq!(attempt.session_binding_nonce.as_deref(), Some(OLD_NONCE));
            assert!(attempt.binding_restorations.is_empty());
        }
        other => panic!("unexpected restoration result: {other:?}"),
    }
}

#[test]
fn attempt_binding_restore_storage_failure_keeps_task_unmodified() {
    let fixture = fixture();
    let before = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    let database_path = fixture.store.database_path_for_test();
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_restore BEFORE UPDATE ON plans
             BEGIN SELECT RAISE(ABORT, 'injected plan write failure'); END;",
        )
        .unwrap();
    drop(connection);

    let error = fixture
        .store
        .restore_current_attempt_binding(request(&fixture, 7), 7)
        .unwrap_err();
    assert!(
        matches!(error, PlanError::Io(ref message) if message.contains("injected plan write failure"))
    );
    assert_eq!(
        fixture
            .store
            .inspect_task_for_test(&fixture.task_id)
            .unwrap()
            .task,
        before
    );
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch("DROP TRIGGER reject_restore;")
        .unwrap();
    drop(connection);
    let reloaded = PlanStore::open(database_path).unwrap();
    assert_eq!(
        reloaded
            .inspect_task_for_test(&fixture.task_id)
            .unwrap()
            .task,
        before
    );
}

#[test]
fn reported_attempt_restart_restoration_bounce_ack_preserves_identity_and_report() {
    let mut fixture = fixture();
    fixture
        .store
        .record_task_attempt_report(
            &fixture.plan_id,
            &fixture.task_id,
            &crate::plan::AgentRef::from_tab(TAB_ID),
            "durable report",
            Some(codeterm_types::agent::ReportStatus::InReview),
            7,
        )
        .unwrap();
    fixture
        .store
        .edit_plan_fixture(&fixture.plan_id, 7, |plan| {
            plan.tasks[0].status = TaskStatus::InReview;
            Ok(())
        })
        .unwrap();
    let before = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    let database = fixture.store.database_path_for_test();
    drop(fixture.store);
    fixture.store = PlanStore::open(database).unwrap();
    assert_eq!(
        fixture
            .store
            .inspect_task_for_test(&fixture.task_id)
            .unwrap()
            .task,
        before
    );
    let restore = request(&fixture, 8);
    fixture
        .store
        .restore_current_attempt_binding(restore, 8)
        .unwrap();
    let restored = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    assert_eq!(restored.status, TaskStatus::InReview);
    assert_eq!(restored.execution_attempts[0].reported_at, Some(7));
    assert_eq!(
        restored.execution_attempts[0].report,
        before.execution_attempts[0].report
    );
    let bounce = fixture
        .store
        .bounce_review(
            &fixture.plan_id,
            &fixture.task_id,
            "repair",
            true,
            true,
            crate::plan::AgentRef::from_tab("owner"),
            9,
        )
        .unwrap();
    let continuation = bounce.continuation.unwrap();
    assert_eq!(continuation.attempt_id, fixture.attempt_id);
    fixture
        .store
        .claim_review_continuation_delivery(
            &fixture.plan_id,
            &fixture.task_id,
            &fixture.attempt_id,
            continuation.epoch,
            10,
        )
        .unwrap();
    fixture
        .store
        .finish_review_continuation(
            &fixture.plan_id,
            &fixture.task_id,
            &fixture.attempt_id,
            continuation.epoch,
            true,
            11,
        )
        .unwrap();
    let ack = fixture
        .store
        .acknowledge_live_task_attempt_for_tab(TAB_ID, None, None, 12)
        .unwrap();
    assert!(ack.already_acknowledged);
    assert_eq!(ack.attempt.id, fixture.attempt_id);
    assert_eq!(ack.attempt.report, before.execution_attempts[0].report);
    assert_eq!(
        fixture
            .store
            .current_plan_skill_attempt(TAB_ID, SESSION_ID, CURRENT_NONCE)
            .unwrap(),
        Some((
            fixture.plan_id.clone(),
            fixture.task_id.clone(),
            fixture.attempt_id.clone()
        ))
    );
    fixture
        .store
        .announce_current_plan_skill_manifest(
            &fixture.plan_id,
            &fixture.task_id,
            &fixture.attempt_id,
            TAB_ID,
            SESSION_ID,
            CURRENT_NONCE,
            12,
        )
        .unwrap();
    let after = fixture
        .store
        .inspect_task_for_test(&fixture.task_id)
        .unwrap()
        .task;
    assert_eq!(after.status, TaskStatus::InProgress);
    assert_eq!(after.execution_attempts.len(), 1);
    assert!(after.execution_attempts[0].closed_at.is_none());
    assert!(after.execution_attempts[0].retired_at.is_none());
    assert!(fixture
        .store
        .current_plan_skill_attempt(TAB_ID, SESSION_ID, OLD_NONCE)
        .is_err());
}

#[test]
fn restoration_undo_preserves_metadata_and_refuses_a_newer_nonce_or_owner() {
    for conflict in [false, true] {
        let f = fixture();
        let request = request(&f, 10);
        f.store
            .restore_current_attempt_binding(request.clone(), 10)
            .unwrap();
        f.store
            .edit_plan_fixture(&f.plan_id, 11, |plan| {
                plan.title = "unrelated edit".into();
                if conflict {
                    let task = plan
                        .tasks
                        .iter_mut()
                        .find(|task| task.id == f.task_id)
                        .unwrap();
                    task.interruption.owner_epoch += 1;
                }
                Ok(())
            })
            .unwrap();
        let result = f.store.undo_restored_attempt_binding(&request, 12);
        let plan = f.store.get(&f.plan_id).unwrap();
        assert_eq!(plan.title, "unrelated edit");
        let attempt = plan
            .tasks
            .iter()
            .find(|task| task.id == f.task_id)
            .unwrap()
            .execution_attempts
            .iter()
            .find(|attempt| attempt.id == f.attempt_id)
            .unwrap();
        if conflict {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("rollback conflict"));
            assert_eq!(
                attempt.session_binding_nonce.as_deref(),
                Some(CURRENT_NONCE)
            );
            assert_eq!(attempt.binding_restorations.last(), Some(&request.receipt));
        } else {
            result.unwrap();
            assert_eq!(attempt.session_binding_nonce.as_deref(), Some(OLD_NONCE));
            assert!(!attempt
                .binding_restorations
                .iter()
                .any(|receipt| receipt.operation_id == request.receipt.operation_id));
        }
    }
}

#[test]
fn executor_identity_undo_preserves_other_attempt_fields_and_rejects_newer_identity() {
    for newer in [None, Some("nonce"), Some("profile")] {
        let f = fixture();
        let before = f
            .store
            .inspect_task_for_test(&f.task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .clone();
        let receipt = f
            .store
            .update_attempt_executor_identity_with_undo(
                &f.plan_id,
                &f.task_id,
                TAB_ID,
                Some("captured executor"),
                Some(SESSION_ID),
                Some(CURRENT_NONCE),
                Some("captured-profile"),
                10,
            )
            .unwrap();
        f.store
            .edit_plan_fixture(&f.plan_id, 11, |plan| {
                let attempt = plan
                    .tasks
                    .iter_mut()
                    .find(|task| task.id == f.task_id)
                    .unwrap()
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == f.attempt_id)
                    .unwrap();
                attempt.model = Some("unrelated model edit".into());
                if newer == Some("nonce") {
                    attempt.session_binding_nonce = Some("newer nonce".into());
                }
                if newer == Some("profile") {
                    attempt.launch_profile = Some("newer-profile".into());
                }
                Ok(())
            })
            .unwrap();
        let result = f.store.undo_attempt_executor_identity(receipt, 12);
        let current = f
            .store
            .inspect_task_for_test(&f.task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .clone();
        assert_eq!(current.model.as_deref(), Some("unrelated model edit"));
        if newer.is_some() {
            assert!(result
                .unwrap_err()
                .to_string()
                .contains("rollback conflict"));
            if newer == Some("nonce") {
                assert_eq!(
                    current.session_binding_nonce.as_deref(),
                    Some("newer nonce")
                );
                assert_eq!(current.launch_profile.as_deref(), Some("captured-profile"));
            } else {
                assert_eq!(current.launch_profile.as_deref(), Some("newer-profile"));
            }
        } else {
            result.unwrap();
            assert_eq!(current.launch_profile, before.launch_profile);
            assert_eq!(current.executor_title, before.executor_title);
            assert_eq!(current.session_id, before.session_id);
            assert_eq!(current.session_binding_nonce, before.session_binding_nonce);
        }
    }
}
