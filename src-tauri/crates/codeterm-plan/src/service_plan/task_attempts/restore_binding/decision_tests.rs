use super::*;
use crate::plan::{OrchestratorRef, Plan, TaskExecutionAttempt, TaskStatus};

const OLD_NONCE: &str = "nonce-before-restore";
const CURRENT_NONCE: &str = "nonce-after-restore";
const TAB_ID: &str = "restore-worker";
const SESSION_ID: &str = "restore-session";
const PROVIDER: &str = "codex";
const TEAM_ID: &str = "restore-team";

fn fixture() -> Plan {
    let mut plan = Plan::new(
        "Restore".into(),
        "project".into(),
        OrchestratorRef::default(),
    );
    plan.team_id = Some(TEAM_ID.into());
    let mut task = crate::service_plan::test_support::task("task");
    task.status = TaskStatus::InProgress;
    task.assigned_tab_id = Some(TAB_ID.into());
    task.lane.tab_id = Some(TAB_ID.into());
    task.interruption.owner_epoch = 1;
    task.interruption.owner_tab_id = Some(TAB_ID.into());
    task.interruption.owner_session_id = Some(SESSION_ID.into());
    let mut attempt = TaskExecutionAttempt {
        id: "attempt".into(),
        executor_tab_id: TAB_ID.into(),
        provider: Some(PROVIDER.into()),
        agent_type: Some(PROVIDER.into()),
        session_id: Some(SESSION_ID.into()),
        session_binding_nonce: Some(OLD_NONCE.into()),
        role: Some("worker".into()),
        assigned_at: Some(3),
        acknowledged_at: Some(4),
        ..Default::default()
    };
    attempt.interruption.owner_epoch = 1;
    task.execution_attempts.push(attempt);
    plan.tasks.push(task);
    plan
}

fn request(plan: &Plan, now_ms: u64) -> RestoreCurrentAttemptBinding {
    RestoreCurrentAttemptBinding {
        plan_id: plan.id.clone(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        executor_tab_id: TAB_ID.into(),
        session_id: SESSION_ID.into(),
        provider: PROVIDER.into(),
        team_id: TEAM_ID.into(),
        owner_epoch: 1,
        expected_old_nonce: OLD_NONCE.into(),
        current_nonce: CURRENT_NONCE.into(),
        receipt: AttemptBindingRestorationReceipt {
            operation_id: "restore-op-1".into(),
            restored_at: now_ms,
            attempt_id: "attempt".into(),
            executor_tab_id: TAB_ID.into(),
            session_id: SESSION_ID.into(),
            provider: PROVIDER.into(),
            owner_epoch: 1,
            old_nonce_digest: nonce_digest(OLD_NONCE),
            current_nonce_digest: nonce_digest(CURRENT_NONCE),
            lineage_proof_kind: "prior_session_binding_nonce_suffix".into(),
            lineage_proof_digest: format!("sha256:{}", "a".repeat(64)),
        },
    }
}

fn mutate_plan(
    plan: &mut Plan,
    mutate: impl FnOnce(&mut Plan) -> Result<(), PlanError>,
) -> Result<(), PlanError> {
    mutate(plan)
}

fn mutate_task(plan: &mut Plan, mutate: impl FnOnce(&mut crate::plan::Task)) {
    mutate(&mut plan.tasks[0]);
}

fn assert_refused_unchanged(
    name: &str,
    now_ms: u64,
    mutate: impl FnOnce(&mut Plan, &mut RestoreCurrentAttemptBinding),
) {
    let mut plan = fixture();
    let mut request = request(&plan, now_ms);
    assert!(matches!(
        restore_binding(&mut plan.clone(), &request, now_ms).unwrap(),
        Some(RestoreCurrentAttemptBindingResult::Restored(_))
    ));
    mutate(&mut plan, &mut request);
    let before = plan.clone();
    assert!(
        restore_binding(&mut plan, &request, now_ms).is_err(),
        "{name} must be refused"
    );
    assert_eq!(plan, before, "{name} must preserve the complete plan");
}

#[test]
fn binding_model_refuses_stale_epoch_assignment_and_team() {
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
        mutate_plan(fixture, |plan| {
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
fn binding_model_refuses_multiple_open_or_interrupted_attempts() {
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
fn binding_model_refuses_nonworker_remote_or_retired_authority() {
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
                mutate_plan(fixture, |plan| {
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
                    "provider" => attempt.provider = Some("other-provider".into()),
                    "agent_type" => attempt.agent_type = Some("other-provider".into()),
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
                request.provider = "other-provider".into();
                request.receipt.provider = "other-provider".into();
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
                mutate_plan(fixture, |plan| {
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
            "receipt_provider" => request.receipt.provider = "other-provider".into(),
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
