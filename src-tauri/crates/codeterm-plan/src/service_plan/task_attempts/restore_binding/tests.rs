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

fn request(fixture: &Fixture, now_ms: u64) -> RestoreCurrentAttemptBinding {
    let task = &fixture.store.show_task(&fixture.task_id).unwrap().task;
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

fn assert_admissible_request(now_ms: u64) {
    let fixture = fixture();
    assert!(matches!(
        fixture
            .store
            .restore_current_attempt_binding(request(&fixture, now_ms), now_ms)
            .unwrap(),
        RestoreCurrentAttemptBindingResult::Restored(_)
    ));
}

fn assert_refused_unchanged(
    name: &str,
    now_ms: u64,
    mutate: impl FnOnce(&Fixture, &mut RestoreCurrentAttemptBinding),
) {
    assert_admissible_request(now_ms);
    let fixture = fixture();
    let mut request = request(&fixture, now_ms);
    mutate(&fixture, &mut request);
    let before = fixture.store.get(&fixture.plan_id).unwrap();
    assert!(
        fixture
            .store
            .restore_current_attempt_binding(request, now_ms)
            .is_err(),
        "{name} must be refused"
    );
    assert_eq!(
        fixture.store.get(&fixture.plan_id).unwrap(),
        before,
        "{name} must preserve the complete plan"
    );
}

fn mutate_task(fixture: &Fixture, mutate: impl FnOnce(&mut crate::plan::Task)) {
    fixture
        .store
        .with_plan(&fixture.plan_id, 7, |plan| {
            mutate(&mut plan.tasks[0]);
            Ok(())
        })
        .unwrap();
}

#[test]
fn attempt_binding_restore_changes_only_nonce_and_receipt() {
    let fixture = fixture();
    let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
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
    let actual = fixture.store.show_task(&fixture.task_id).unwrap().task;
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
            .show_task(&fixture.task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .loaded_plan_skills[0]
            .loaded_at,
        6
    );
}

#[test]
fn attempt_binding_restore_refuses_stale_epoch_assignment_and_team_without_mutation() {
    const NOW: u64 = 8;
    for refusal in ["task_epoch", "attempt_epoch", "owner_tab", "owner_session"] {
        assert_refused_unchanged(refusal, NOW, |fixture, _| {
            mutate_task(fixture, |task| match refusal {
                "task_epoch" => task.interruption.owner_epoch += 1,
                "attempt_epoch" => task.execution_attempts[0].interruption.owner_epoch += 1,
                "owner_tab" => task.interruption.owner_tab_id = Some("other-tab".into()),
                "owner_session" => {
                    task.interruption.owner_session_id = Some("other-session".into())
                }
                _ => unreachable!(),
            });
        });
    }
    assert_refused_unchanged("zero_epoch", NOW, |fixture, request| {
        mutate_task(fixture, |task| {
            task.interruption.owner_epoch = 0;
            task.execution_attempts[0].interruption.owner_epoch = 0;
        });
        request.owner_epoch = 0;
        request.receipt.owner_epoch = 0;
    });
    for refusal in ["assignment", "lane_assignment"] {
        assert_refused_unchanged(refusal, NOW, |fixture, _| {
            mutate_task(fixture, |task| match refusal {
                "assignment" => task.assigned_tab_id = Some("other-tab".into()),
                "lane_assignment" => task.lane.tab_id = Some("other-tab".into()),
                _ => unreachable!(),
            });
        });
    }
    assert_refused_unchanged("plan_team", NOW, |fixture, _| {
        fixture
            .store
            .with_plan(&fixture.plan_id, 7, |plan| {
                plan.team_id = Some("unrelated-team".into());
                Ok(())
            })
            .unwrap();
    });
    assert_refused_unchanged("request_team", NOW, |_, request| {
        request.team_id = "unrelated-team".into();
    });
}

#[test]
fn attempt_binding_restore_refuses_multiple_open_or_interrupted_attempts() {
    const NOW: u64 = 8;
    for refusal in [
        "handoff",
        "task_checkpoint",
        "multiple_open",
        "duplicate_id",
    ] {
        assert_refused_unchanged(refusal, NOW, |fixture, _| {
            mutate_task(fixture, |task| match refusal {
                "handoff" => task.interruption.handoff_required = true,
                "task_checkpoint" => task.interruption.checkpoint = Some(Default::default()),
                "multiple_open" => {
                    let mut duplicate = task.execution_attempts[0].clone();
                    duplicate.id = "another-open-attempt".into();
                    task.execution_attempts.push(duplicate);
                }
                "duplicate_id" => {
                    let mut duplicate = task.execution_attempts[0].clone();
                    duplicate.closed_at = Some(7);
                    task.execution_attempts.push(duplicate);
                }
                _ => unreachable!(),
            });
        });
    }
}

#[test]
fn attempt_binding_restore_refuses_nonworker_remote_or_retired_authority_without_mutation() {
    const NOW: u64 = 8;
    for refusal in [
        "terminal",
        "pending",
        "remote",
        "task_competition",
        "archived_plan",
    ] {
        assert_refused_unchanged(refusal, NOW, |fixture, _| {
            if refusal == "archived_plan" {
                fixture
                    .store
                    .with_plan(&fixture.plan_id, 7, |plan| {
                        plan.status = PlanStatus::Archived;
                        Ok(())
                    })
                    .unwrap();
            } else {
                mutate_task(fixture, |task| match refusal {
                    "terminal" => task.status = TaskStatus::Done,
                    "pending" => task.status = TaskStatus::Pending,
                    "remote" => task.peer = Some("peer-origin".into()),
                    "task_competition" => task.competition = Some(Default::default()),
                    _ => unreachable!(),
                });
            }
        });
    }
    for refusal in [
        "executor",
        "provider",
        "agent_type",
        "session",
        "role",
        "retired",
        "closed",
        "unacked",
        "attempt_epoch",
        "interrupted_at",
        "interruption_reason",
        "interruption_checkpoint",
        "continuation",
        "competition_group",
        "competition_lane",
        "mesh_peer",
        "remote_dispatch",
        "canonical_writer",
        "old_nonce",
        "missing_old_nonce",
    ] {
        assert_refused_unchanged(refusal, NOW, |fixture, _| {
            mutate_task(fixture, |task| {
                let attempt = &mut task.execution_attempts[0];
                match refusal {
                    "executor" => attempt.executor_tab_id = "other-tab".into(),
                    "provider" => attempt.provider = Some("claude".into()),
                    "agent_type" => attempt.agent_type = Some("claude".into()),
                    "session" => attempt.session_id = Some("other-session".into()),
                    "role" => attempt.role = Some("orchestrator".into()),
                    "retired" => attempt.retired_at = Some(7),
                    "closed" => attempt.closed_at = Some(7),
                    "unacked" => attempt.acknowledged_at = None,
                    "attempt_epoch" => attempt.interruption.owner_epoch += 1,
                    "interrupted_at" => attempt.interruption.interrupted_at = Some(7),
                    "interruption_reason" => attempt.interruption.reason = Some("reboot".into()),
                    "interruption_checkpoint" => {
                        attempt.interruption.checkpoint_digest = Some("sha256:checkpoint".into())
                    }
                    "continuation" => attempt.continuation.epoch = 1,
                    "competition_group" => attempt.competition_group_id = Some("group".into()),
                    "competition_lane" => attempt.competition_lane = Some(1),
                    "mesh_peer" => attempt.mesh_peer = Some("remote-peer".into()),
                    "remote_dispatch" => {
                        attempt.remote_dispatch_state =
                            Some(crate::plan::RemoteDispatchState::Acknowledged)
                    }
                    "canonical_writer" => {
                        attempt.placement = crate::plan::WorkerPlacement::CanonicalWriter
                    }
                    "old_nonce" => attempt.session_binding_nonce = Some("other-old".into()),
                    "missing_old_nonce" => attempt.session_binding_nonce = None,
                    _ => unreachable!(),
                }
            });
        });
    }
    for refusal in [
        "request_plan",
        "request_task",
        "request_attempt",
        "request_executor",
        "request_session",
        "request_provider",
        "request_old_nonce",
    ] {
        assert_refused_unchanged(refusal, NOW, |_, request| match refusal {
            "request_plan" => request.plan_id = "unknown-plan".into(),
            "request_task" => request.task_id = "unknown-task".into(),
            "request_attempt" => {
                request.attempt_id = "unknown-attempt".into();
                request.receipt.attempt_id = "unknown-attempt".into();
            }
            "request_executor" => {
                request.executor_tab_id = "other-tab".into();
                request.receipt.executor_tab_id = "other-tab".into();
            }
            "request_session" => {
                request.session_id = "other-session".into();
                request.receipt.session_id = "other-session".into();
            }
            "request_provider" => {
                request.provider = "claude".into();
                request.receipt.provider = "claude".into();
            }
            "request_old_nonce" => {
                request.expected_old_nonce = "other-old".into();
                request.receipt.old_nonce_digest = nonce_digest("other-old");
            }
            _ => unreachable!(),
        });
    }
    for refusal in [
        "empty_owner_epoch",
        "empty_plan",
        "empty_task",
        "empty_attempt",
        "empty_executor",
        "empty_session",
        "empty_provider",
        "empty_team",
        "empty_old_nonce",
        "empty_current_nonce",
    ] {
        assert_refused_unchanged(refusal, NOW, |fixture, request| match refusal {
            "empty_owner_epoch" => {
                request.owner_epoch = 0;
                request.receipt.owner_epoch = 0;
            }
            "empty_plan" => request.plan_id.clear(),
            "empty_task" => request.task_id.clear(),
            "empty_attempt" => {
                request.attempt_id.clear();
                mutate_task(fixture, |task| task.execution_attempts[0].id.clear());
                request.receipt.attempt_id.clear();
            }
            "empty_executor" => {
                request.executor_tab_id.clear();
                request.receipt.executor_tab_id.clear();
                mutate_task(fixture, |task| {
                    task.assigned_tab_id = Some(String::new());
                    task.lane.tab_id = Some(String::new());
                    task.execution_attempts[0].executor_tab_id.clear();
                    task.interruption.owner_tab_id = Some(String::new());
                });
            }
            "empty_session" => {
                request.session_id.clear();
                request.receipt.session_id.clear();
                mutate_task(fixture, |task| {
                    task.interruption.owner_session_id = Some(String::new());
                    task.execution_attempts[0].session_id = Some(String::new());
                });
            }
            "empty_provider" => {
                request.provider.clear();
                request.receipt.provider.clear();
                mutate_task(fixture, |task| {
                    task.execution_attempts[0].provider = Some(String::new());
                    task.execution_attempts[0].agent_type = Some(String::new());
                });
            }
            "empty_team" => {
                request.team_id.clear();
                fixture
                    .store
                    .with_plan(&fixture.plan_id, 7, |plan| {
                        plan.team_id = Some(String::new());
                        Ok(())
                    })
                    .unwrap();
            }
            "empty_old_nonce" => {
                request.expected_old_nonce.clear();
                request.receipt.old_nonce_digest = nonce_digest("");
                mutate_task(fixture, |task| {
                    task.execution_attempts[0].session_binding_nonce = Some(String::new())
                });
            }
            "empty_current_nonce" => {
                request.current_nonce.clear();
                request.receipt.current_nonce_digest = nonce_digest("");
            }
            _ => unreachable!(),
        });
    }
    for refusal in [
        "operation_id",
        "receipt_time",
        "receipt_attempt",
        "receipt_executor",
        "receipt_session",
        "receipt_provider",
        "receipt_epoch",
        "receipt_old_digest",
        "receipt_current_digest",
        "receipt_kind",
        "receipt_digest",
        "duplicate_operation",
    ] {
        assert_refused_unchanged(refusal, NOW, |fixture, request| match refusal {
            "operation_id" => request.receipt.operation_id.clear(),
            "receipt_time" => request.receipt.restored_at += 1,
            "receipt_attempt" => request.receipt.attempt_id = "other-attempt".into(),
            "receipt_executor" => request.receipt.executor_tab_id = "other-tab".into(),
            "receipt_session" => request.receipt.session_id = "other-session".into(),
            "receipt_provider" => request.receipt.provider = "claude".into(),
            "receipt_epoch" => request.receipt.owner_epoch += 1,
            "receipt_old_digest" => request.receipt.old_nonce_digest = "sha256:old".into(),
            "receipt_current_digest" => {
                request.receipt.current_nonce_digest = "sha256:current".into()
            }
            "receipt_kind" => request.receipt.lineage_proof_kind = "other-proof".into(),
            "receipt_digest" => request.receipt.lineage_proof_digest = "sha256:invalid".into(),
            "duplicate_operation" => mutate_task(fixture, |task| {
                task.execution_attempts[0]
                    .binding_restorations
                    .push(request.receipt.clone())
            }),
            _ => unreachable!(),
        });
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
    let request = request(&fixture, 8);
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
    let initial_request = request(&fixture, 7);
    fixture
        .store
        .restore_current_attempt_binding(initial_request, 7)
        .unwrap();
    let before = fixture.store.show_task(&fixture.task_id).unwrap().task;
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
        fixture.store.show_task(&fixture.task_id).unwrap().task,
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
    let task = store.show_task(&task_id).unwrap().task;
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
        .restore_current_attempt_binding(request(&fixture, 7), 7)
        .is_err());
    assert_eq!(
        fixture.store.show_task(&fixture.task_id).unwrap().task,
        before
    );
    let reloaded = PlanStore::open(database_path).unwrap();
    assert_eq!(reloaded.show_task(&fixture.task_id).unwrap().task, before);
}
