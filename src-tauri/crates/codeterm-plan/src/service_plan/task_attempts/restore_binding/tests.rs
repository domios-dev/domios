use super::*;
use crate::plan::{OrchestratorRef, Plan, PlanMode, PlanSkillManifestEntry, TaskLaneMetadata};
use crate::service_plan::task_assignment::AssignmentRuntimeBinding;
use crate::service_plan::NewTask;
use std::sync::{Arc, Barrier};

const OLD_NONCE: &str = "nonce-before-restore";
const CURRENT_NONCE: &str = "nonce-after-restore";
const SESSION_ID: &str = "restore-session";
const TAB_ID: &str = "restore-worker";
const PROVIDER: &str = "codex";

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
    let attempt_id = store.show_task(&task_id).unwrap().task.execution_attempts[0]
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

fn request(fixture: &Fixture) -> RestoreCurrentAttemptBinding {
    let task = &fixture.store.show_task(&fixture.task_id).unwrap().task;
    RestoreCurrentAttemptBinding {
        plan_id: fixture.plan_id.clone(),
        task_id: fixture.task_id.clone(),
        attempt_id: fixture.attempt_id.clone(),
        executor_tab_id: TAB_ID.into(),
        session_id: SESSION_ID.into(),
        provider: PROVIDER.into(),
        team_id: None,
        owner_epoch: task.interruption.owner_epoch,
        expected_old_nonce: OLD_NONCE.into(),
        current_nonce: CURRENT_NONCE.into(),
        receipt: AttemptBindingRestorationReceipt {
            operation_id: "restore-op-1".into(),
            restored_at: 7,
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
    let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
    let request = request(&fixture);
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
    let actual = fixture.store.show_task(&fixture.task_id).unwrap().task;
    assert_eq!(actual, expected);
    assert_eq!(actual.execution_attempts[0].acknowledged_at, Some(4));
    assert_eq!(actual.execution_attempts[0].loaded_plan_skills[0].loaded_at, 6);
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
        fixture.store.show_task(&fixture.task_id).unwrap().task.execution_attempts[0]
            .loaded_plan_skills[0]
            .loaded_at,
        6
    );
}

#[test]
fn attempt_binding_restore_refuses_stale_epoch_and_assignment_without_mutation() {
    for refusal in ["epoch", "assignment", "team"] {
        let fixture = fixture();
        let mut request = request(&fixture);
        fixture
            .store
            .with_plan(&fixture.plan_id, 7, |plan| {
                match refusal {
                    "assignment" => plan.tasks[0].assigned_tab_id = Some("other-tab".into()),
                    "team" => plan.team_id = Some("unrelated-team".into()),
                    "epoch" => request.owner_epoch += 1,
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
        assert!(fixture
            .store
            .restore_current_attempt_binding(request, 8)
            .is_err());
        assert_eq!(fixture.store.show_task(&fixture.task_id).unwrap().task, before);
    }
}

#[test]
fn attempt_binding_restore_refuses_multiple_open_or_interrupted_attempts() {
    for duplicate_attempt in [false, true] {
        let fixture = fixture();
        fixture
            .store
            .with_plan(&fixture.plan_id, 7, |plan| {
                let task = &mut plan.tasks[0];
                if duplicate_attempt {
                    let mut duplicate = task.execution_attempts[0].clone();
                    duplicate.id = "another-open-attempt".into();
                    task.execution_attempts.push(duplicate);
                } else {
                    task.interruption.handoff_required = true;
                }
                Ok(())
            })
            .unwrap();
        let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
        assert!(fixture
            .store
            .restore_current_attempt_binding(request(&fixture), 8)
            .is_err());
        assert_eq!(fixture.store.show_task(&fixture.task_id).unwrap().task, before);
    }
}

#[test]
fn attempt_binding_restore_refuses_nonworker_remote_or_retired_authority_without_mutation() {
    for refusal in [
        "terminal",
        "remote",
        "provider",
        "session",
        "role",
        "retired",
        "unacked",
        "old_nonce",
        "receipt",
    ] {
        let fixture = fixture();
        fixture
            .store
            .with_plan(&fixture.plan_id, 7, |plan| {
                let task = &mut plan.tasks[0];
                match refusal {
                    "terminal" => task.status = TaskStatus::Done,
                    "remote" => task.peer = Some("peer-origin".into()),
                    "provider" => task.execution_attempts[0].provider = Some("claude".into()),
                    "session" => task.execution_attempts[0].session_id = Some("other-session".into()),
                    "role" => task.execution_attempts[0].role = Some("orchestrator".into()),
                    "retired" => task.execution_attempts[0].retired_at = Some(7),
                    "unacked" => task.execution_attempts[0].acknowledged_at = None,
                    "old_nonce" => {
                        task.execution_attempts[0].session_binding_nonce = Some("other-old".into())
                    }
                    "receipt" => {}
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
        let mut request = request(&fixture);
        if refusal == "receipt" {
            request.receipt.lineage_proof_digest = "sha256:invalid".into();
        }
        assert!(fixture
            .store
            .restore_current_attempt_binding(request, 8)
            .is_err());
        assert_eq!(fixture.store.show_task(&fixture.task_id).unwrap().task, before);
    }
}

#[test]
fn attempt_binding_restore_accepts_an_acknowledged_in_review_attempt() {
    let fixture = fixture();
    fixture
        .store
        .with_plan(&fixture.plan_id, 7, |plan| {
            plan.tasks[0].status = TaskStatus::InReview;
            plan.tasks[0].execution_attempts[0].reported_at = Some(7);
            Ok(())
        })
        .unwrap();
    let mut request = request(&fixture);
    request.receipt.restored_at = 8;
    assert!(matches!(
        fixture
            .store
            .restore_current_attempt_binding(request, 8)
            .unwrap(),
        RestoreCurrentAttemptBindingResult::Restored(_)
    ));
    let task = fixture.store.show_task(&fixture.task_id).unwrap().task;
    assert_eq!(task.status, TaskStatus::InReview);
    assert_eq!(task.execution_attempts[0].reported_at, Some(7));
}

#[test]
fn attempt_binding_restore_current_replay_is_a_noop() {
    let fixture = fixture();
    let request = request(&fixture);
    fixture
        .store
        .restore_current_attempt_binding(request.clone(), 7)
        .unwrap();
    let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
    let operation_count = fixture
        .store
        .operation_count_for_test(Some(&fixture.plan_id));
    assert_eq!(
        fixture
            .store
            .restore_current_attempt_binding(request, 8)
            .unwrap(),
        RestoreCurrentAttemptBindingResult::AlreadyCurrent
    );
    assert_eq!(fixture.store.show_task(&fixture.task_id).unwrap().task, before);
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
    let request = request(&fixture);
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
    let task = store.show_task(&task_id).unwrap().task;
    assert_eq!(task.execution_attempts[0].session_binding_nonce.as_deref(), Some(CURRENT_NONCE));
    assert_eq!(task.execution_attempts[0].binding_restorations.len(), 1);
}

#[test]
fn attempt_binding_restore_close_race_serializes_without_resurrecting_a_closed_attempt() {
    let fixture = fixture();
    let task_id = fixture.task_id.clone();
    let plan_id = fixture.plan_id.clone();
    let restore_request = request(&fixture);
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
        close_store.with_plan(&plan_id, 8, |plan| {
            plan.tasks[0].execution_attempts[0].closed_at = Some(8);
            Ok(())
        })
    });
    barrier.wait();
    let restore_result = restore.join().unwrap();
    close.join().unwrap().unwrap();
    let task = store.show_task(&task_id).unwrap().task;
    let attempt = &task.execution_attempts[0];
    assert_eq!(attempt.closed_at, Some(8));
    match restore_result {
        Ok(RestoreCurrentAttemptBindingResult::Restored(_)) => {
            assert_eq!(attempt.session_binding_nonce.as_deref(), Some(CURRENT_NONCE));
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
    let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
    let database_path = fixture.store.database_path_for_test();
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute_batch(
            "CREATE TRIGGER reject_restore BEFORE UPDATE ON plans
             BEGIN SELECT RAISE(ABORT, 'injected plan write failure'); END;",
        )
        .unwrap();
    drop(connection);

    assert!(fixture
        .store
        .restore_current_attempt_binding(request(&fixture), 7)
        .is_err());
    assert_eq!(fixture.store.show_task(&fixture.task_id).unwrap().task, before);
    let reloaded = PlanStore::open(database_path).unwrap();
    assert_eq!(reloaded.show_task(&fixture.task_id).unwrap().task, before);
}
