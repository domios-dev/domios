use super::*;
use crate::multiplexer::types::{AgentRole, PriorBindingNonce};
use crate::plan::{AgentRef, OrchestratorRef, Plan};
use crate::service::plan::task_assignment::AssignmentRuntimeBinding;
use crate::service::plan::NewTask;
use codeterm_types::agent::ReportStatus;

const SESSION: &str = "reported-session";
const OLD_NONCE: &str = "before-restart";
const CURRENT_NONCE: &str = "after-restart";

struct Fixture {
    dir: tempfile::TempDir,
    store: PlanStore,
    mux: MultiplexerState,
    plan_id: String,
    task_id: String,
    tab_id: String,
    attempt_id: String,
}

fn fixture(status: ReportStatus) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let (mut mux, owner) = MultiplexerState::new();
    mux.set_orchestrator_role(&owner).unwrap();
    let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let workspace = mux.list_workspaces()[0].id.clone();
    let tab_id = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&tab_id, AgentRole::Worker, Some(&owner))
        .unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.team_id = Some(team.clone());
    tab.agent_type = Some("codex".into());
    tab.agent_session_id = Some(SESSION.into());
    tab.session_binding_nonce = Some(CURRENT_NONCE.into());
    tab.reported_session_binding_nonce = Some(CURRENT_NONCE.into());
    tab.pty_id = Some("restored-pty".into());
    tab.chat_view = true;
    tab.prior_session_binding_nonces = vec![PriorBindingNonce {
        nonce: OLD_NONCE.into(),
        session_id: Some(SESSION.into()),
        resume_target_session_id: Some(SESSION.into()),
    }];
    let mut plan = Plan::new(
        "Reported worker restoration".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab(&owner),
    );
    plan.team_id = Some(team);
    store.save(&mut plan, 1).unwrap();
    let task_id = store
        .add_task(
            &plan.id,
            NewTask {
                name: "Continue the reported attempt".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    store
        .assign_task_with_runtime_binding(
            &plan.id,
            &task_id,
            &tab_id,
            None,
            Some("worker"),
            Some(&AssignmentRuntimeBinding {
                session_id: SESSION.into(),
                session_binding_nonce: OLD_NONCE.into(),
                provider: Some("codex".into()),
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
        .acknowledge_task_attempt(&plan.id, &task_id, &tab_id, &attempt_id, 4)
        .unwrap();
    store
        .record_task_attempt_report(
            &plan.id,
            &task_id,
            &AgentRef::from_tab(&tab_id),
            "persisted work",
            Some(status),
            5,
        )
        .unwrap();
    let mut plan = store.get(&plan.id).unwrap();
    plan.tasks[0].status = TaskStatus::InReview;
    store.save(&mut plan, 5).unwrap();
    Fixture {
        dir,
        store,
        mux,
        plan_id: plan.id,
        task_id,
        tab_id,
        attempt_id,
    }
}

#[test]
fn reported_restart_bounce_keeps_one_current_attempt_and_ack_skill_lookup_succeeds() {
    assert_reported_restart_bounce(false);
}

#[test]
fn live_chat_backend_worker_bounce_restores_binding_and_continues_same_attempt() {
    assert_reported_restart_bounce(true);
}

fn assert_reported_restart_bounce(runtime: bool) {
    for status in [ReportStatus::Partial, ReportStatus::InReview] {
        let mut f = fixture(status);
        let _live_runtime = if runtime {
            let tab = f.mux.get_tab_mut(&f.tab_id).unwrap();
            tab.pty_id = None;
            tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
                plugin_id: tab.agent_type.clone().unwrap(),
                session_id: SESSION.into(),
            };
            tab.agent_runtime_state = Some(serde_json::json!({"state":"loaded"}));
            Some(crate::service::agent_runtime::engine::test_support::live_runtime(tab))
        } else {
            None
        };
        let report = f
            .store
            .show_task(&f.task_id)
            .unwrap()
            .task
            .execution_attempts[0]
            .report
            .clone();
        drop(f.store);
        f.store = PlanStore::new_for_test(f.dir.path());
        assert!(f
            .store
            .current_plan_skill_attempt(&f.tab_id, SESSION, CURRENT_NONCE)
            .is_err());
        restore_review_attempt_binding_for_mux(&f.store, &f.mux, &f.plan_id, &f.task_id, 6)
            .unwrap();
        let bounce = f
            .store
            .bounce_review(
                &f.plan_id,
                &f.task_id,
                "repair",
                true,
                true,
                AgentRef::from_tab("owner"),
                7,
            )
            .unwrap();
        let continuation = bounce.continuation.unwrap();
        assert_eq!(continuation.attempt_id, f.attempt_id);
        f.store
            .claim_review_continuation_delivery(
                &f.plan_id,
                &f.task_id,
                &f.attempt_id,
                continuation.epoch,
                8,
            )
            .unwrap();
        assert!(f
            .store
            .finish_review_continuation(
                &f.plan_id,
                &f.task_id,
                &f.attempt_id,
                continuation.epoch,
                true,
                9,
            )
            .unwrap());
        let ack = f
            .store
            .acknowledge_live_task_attempt_for_tab(&f.tab_id, None, None, 10)
            .unwrap();
        assert!(ack.already_acknowledged);
        assert_eq!(ack.attempt.id, f.attempt_id);
        assert_eq!(ack.attempt.report, report);
        assert_eq!(ack.attempt.binding_restorations.len(), 1);
        f.store
            .announce_current_plan_skill_manifest(
                &f.plan_id,
                &f.task_id,
                &f.attempt_id,
                &f.tab_id,
                SESSION,
                CURRENT_NONCE,
                10,
            )
            .unwrap();
        let task = f.store.show_task(&f.task_id).unwrap().task;
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.execution_attempts.len(), 1);
        assert!(task.execution_attempts[0].closed_at.is_none());
        assert!(f
            .store
            .current_plan_skill_attempt(&f.tab_id, SESSION, OLD_NONCE)
            .is_err());
    }
}

#[test]
fn reported_second_round_restart_bounce_restores_the_idle_continuation_epoch() {
    let f = fixture(ReportStatus::InReview);
    restore_review_attempt_binding_for_mux(&f.store, &f.mux, &f.plan_id, &f.task_id, 6).unwrap();
    let bounce = f
        .store
        .bounce_review(
            &f.plan_id,
            &f.task_id,
            "first correction",
            true,
            true,
            AgentRef::from_tab("owner"),
            7,
        )
        .unwrap();
    let epoch = bounce.continuation.unwrap().epoch;
    f.store
        .claim_review_continuation_delivery(&f.plan_id, &f.task_id, &f.attempt_id, epoch, 8)
        .unwrap();
    f.store
        .finish_review_continuation(&f.plan_id, &f.task_id, &f.attempt_id, epoch, true, 9)
        .unwrap();
    let task = f.store.show_task(&f.task_id).unwrap().task;
    assert!(f
        .store
        .ingest_bound_worker_report(
            &f.plan_id,
            &f.task_id,
            &AgentRef::from_tab(&f.tab_id),
            crate::service::plan::task_attempts::BoundWorkerReport {
                message: "second report".into(),
                status: Some(ReportStatus::InReview),
                authority: Some(crate::service::plan::interruption::TaskMutationAuthority {
                    tab_id: f.tab_id.clone(),
                    session_id: SESSION.into(),
                    session_binding_nonce: CURRENT_NONCE.into(),
                    owner_epoch: task.interruption.owner_epoch,
                }),
                ..Default::default()
            },
            10,
        )
        .unwrap());
    let mut f = f;
    drop(f.store);
    f.store = PlanStore::new_for_test(f.dir.path());
    let tab = f.mux.get_tab_mut(&f.tab_id).unwrap();
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        nonce: CURRENT_NONCE.into(),
        session_id: Some(SESSION.into()),
        resume_target_session_id: Some(SESSION.into()),
    });
    tab.session_binding_nonce = Some("second-restart".into());
    tab.reported_session_binding_nonce = Some("second-restart".into());
    restore_review_attempt_binding_for_mux(&f.store, &f.mux, &f.plan_id, &f.task_id, 11).unwrap();
    let bounce = f
        .store
        .bounce_review(
            &f.plan_id,
            &f.task_id,
            "second correction",
            true,
            true,
            AgentRef::from_tab("owner"),
            12,
        )
        .unwrap();
    let continuation = bounce.continuation.unwrap();
    assert!(continuation.epoch > epoch);
    f.store
        .claim_review_continuation_delivery(
            &f.plan_id,
            &f.task_id,
            &f.attempt_id,
            continuation.epoch,
            13,
        )
        .unwrap();
    f.store
        .finish_review_continuation(
            &f.plan_id,
            &f.task_id,
            &f.attempt_id,
            continuation.epoch,
            true,
            14,
        )
        .unwrap();
    let ack = f
        .store
        .acknowledge_live_task_attempt_for_tab(&f.tab_id, None, None, 15)
        .unwrap();
    assert_eq!(ack.attempt.id, f.attempt_id);
    assert_eq!(ack.attempt.binding_restorations.len(), 2);
    f.store
        .announce_current_plan_skill_manifest(
            &f.plan_id,
            &f.task_id,
            &f.attempt_id,
            &f.tab_id,
            SESSION,
            "second-restart",
            15,
        )
        .unwrap();
    assert!(!f
        .store
        .finish_review_continuation(&f.plan_id, &f.task_id, &f.attempt_id, epoch, true, 16,)
        .unwrap());
}

#[test]
fn restored_bounce_refuses_unproven_or_foreign_session_without_mutating_the_plan() {
    for invalid in [
        "missing_lineage",
        "foreign_session",
        "wrong_team",
        "ambiguous_attempts",
    ] {
        let mut f = fixture(ReportStatus::InReview);
        let tab = f.mux.get_tab_mut(&f.tab_id).unwrap();
        match invalid {
            "missing_lineage" => tab.prior_session_binding_nonces.clear(),
            "foreign_session" => tab.agent_session_id = Some("foreign".into()),
            "wrong_team" => tab.team_id = Some("foreign-team".into()),
            "ambiguous_attempts" => {
                let mut plan = f.store.get(&f.plan_id).unwrap();
                let mut duplicate = plan.tasks[0].execution_attempts[0].clone();
                duplicate.id = "foreign-attempt".into();
                plan.tasks[0].execution_attempts.push(duplicate);
                f.store.save(&mut plan, 6).unwrap();
            }
            _ => unreachable!(),
        }
        let before = f.store.get(&f.plan_id).unwrap();
        assert!(
            restore_review_attempt_binding_for_mux(&f.store, &f.mux, &f.plan_id, &f.task_id, 7,)
                .is_err(),
            "{invalid}"
        );
        assert_eq!(f.store.get(&f.plan_id).unwrap(), before);
    }
}

#[test]
fn runtime_bounce_refuses_unloaded_and_foreign_nonce_without_mutating_plan() {
    for invalid in ["unloaded", "wrong_nonce", "ended_process"] {
        let mut f = fixture(ReportStatus::InReview);
        let tab = f.mux.get_tab_mut(&f.tab_id).unwrap();
        tab.pty_id = None;
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: tab.agent_type.clone().unwrap(),
            session_id: SESSION.into(),
        };
        tab.agent_runtime_state = Some(serde_json::json!({"state":"loaded"}));
        let _live = crate::service::agent_runtime::engine::test_support::live_runtime(tab);
        let session = crate::service::agent_runtime::engine::for_tab(&f.tab_id).unwrap();
        match invalid {
            "unloaded" => session
                .thread_loaded
                .store(false, std::sync::atomic::Ordering::Release),
            "wrong_nonce" => session.context.lock().binding_nonce = "foreign".into(),
            "ended_process" => {
                session.process.lock().take();
            }
            _ => unreachable!(),
        }
        let before = f.store.get(&f.plan_id).unwrap();
        assert!(restore_review_attempt_binding_for_mux(
            &f.store, &f.mux, &f.plan_id, &f.task_id, 6
        )
        .is_err());
        assert_eq!(f.store.get(&f.plan_id).unwrap(), before);
    }
}
