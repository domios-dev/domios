use super::*;
use codeterm_service_core::multiplexer::types::{AgentRole, PriorBindingNonce};
use codeterm_service_core::plan::{OrchestratorRef, Plan, PlanMode};
use codeterm_service_core::service::plan::task_assignment::AssignmentRuntimeBinding;
use codeterm_service_core::service::{
    restore_attempt_binding_for_caller, AttemptBindingRestoreTarget, AuthenticatedPlanCaller,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn restoration_and_executor_identity_do_not_hold_mux_while_plan_writer_is_held() {
    for restore in [true, false] {
        for change_launch in [false, true] {
            let (mut mux, owner) = MultiplexerState::new();
            let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
            let workspace = mux.find_workspace_for_tab(&owner).unwrap();
            let worker = mux.create_tab(&workspace).unwrap();
            mux.set_tab_role(&worker, AgentRole::Worker, Some(&owner))
                .unwrap();
            mux.link_pty(&worker, "restoration-pty".into());
            let tab = mux.get_tab_mut(&worker).unwrap();
            tab.agent_type = Some("opaque-restore-provider".into());
            tab.agent_session_id = Some("same-session".into());
            tab.session_binding_nonce = Some("current-nonce".into());
            tab.reported_session_binding_nonce = Some("current-nonce".into());
            tab.chat_view = true;
            tab.prior_session_binding_nonces = vec![PriorBindingNonce {
                nonce: "old-nonce".into(),
                session_id: Some("same-session".into()),
                resume_target_session_id: Some("same-session".into()),
            }];
            let fixture = ServiceFixture::new(mux);
            let state = app_state(&fixture);
            let mut plan = Plan::new(
                "Restoration liveness".into(),
                fixture.temp.path().display().to_string(),
                OrchestratorRef::from_tab(&owner),
            );
            plan.mode = PlanMode::Execution;
            plan.team_id = Some(team);
            let store = state.svc.plan_store.clone();
            store.save(&mut plan, 1).unwrap();
            let task = store
                .add_task(
                    &plan.id,
                    codeterm_service_core::service::plan::NewTask {
                        name: "Restore exactly".into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap();
            store
                .assign_task_with_runtime_binding(
                    &plan.id,
                    &task,
                    &worker,
                    None,
                    Some("worker"),
                    Some(&AssignmentRuntimeBinding {
                        session_id: "same-session".into(),
                        session_binding_nonce: "old-nonce".into(),
                        provider: Some("opaque-restore-provider".into()),
                        model: None,
                        session_model: None,
                        reasoning_effort: None,
                    }),
                    None,
                    3,
                )
                .unwrap();
            let before = store.get(&plan.id).unwrap();
            let attempt = before
                .tasks
                .iter()
                .find(|item| item.id == task)
                .unwrap()
                .execution_attempts
                .last()
                .unwrap()
                .id
                .clone();
            let ack = store
                .acknowledge_task_attempt(&plan.id, &task, &worker, &attempt, 4)
                .unwrap();
            let owner_epoch = ack.attempt.interruption.owner_epoch;
            let held_store = store.clone();
            let held = HeldDependency::new(move |entered, release| {
                held_store.with_mutation_writer_held_for_test(|| {
                    entered.send(()).unwrap();
                    release.recv_timeout(Duration::from_secs(10)).unwrap();
                })
            })
            .await;
            let service = state.svc.clone();
            let (plan_id, task_id, attempt_id, tab_id) = (
                plan.id.clone(),
                task.clone(),
                attempt.clone(),
                worker.clone(),
            );
            let operation = tokio::task::spawn_blocking(move || {
                if restore {
                    restore_attempt_binding_for_caller(
                        &service,
                        AttemptBindingRestoreTarget {
                            plan_id: &plan_id,
                            task_id: &task_id,
                            attempt_id: &attempt_id,
                            owner_epoch,
                        },
                        &AuthenticatedPlanCaller {
                            tab_id: Some(tab_id),
                            session_id: Some("same-session".into()),
                            binding_nonce: Some("current-nonce".into()),
                        },
                        5,
                    )
                    .map(|_| ())
                } else {
                    codeterm_service_core::service::persist_attempt_executor_identity_for_mux(
                        &service.plan_store,
                        &service.mux,
                        &plan_id,
                        &task_id,
                        &tab_id,
                        "new executor title",
                        5,
                    )
                }
            });
            let observer = store.clone();
            assert!(tokio::task::spawn_blocking(
                move || observer.wait_for_queued_mutation_for_test(Duration::from_secs(2))
            )
            .await
            .unwrap());
            assert!(!operation.is_finished());
            let mux_available = state.svc.mux.try_write().is_some();
            if change_launch && mux_available {
                state
                    .svc
                    .mux
                    .write()
                    .get_tab_mut(&worker)
                    .unwrap()
                    .agent_session_id = Some("new-session".into());
            }
            crate::rest_liveness_tests::assert_health_list_send_respond(state.clone()).await;
            drop(held);
            let result = operation.await.unwrap();
            assert!(mux_available, "plan writer wait must not retain mux");
            let current = store.get(&plan.id).unwrap();
            let persisted = current
                .tasks
                .iter()
                .find(|item| item.id == task)
                .unwrap()
                .execution_attempts
                .iter()
                .find(|item| item.id == attempt)
                .unwrap();
            if change_launch {
                assert!(result.unwrap_err().to_string().contains("conflict"));
                assert_eq!(
                    persisted.session_binding_nonce.as_deref(),
                    Some("old-nonce")
                );
                assert!(persisted.binding_restorations.is_empty());
            } else {
                result.unwrap();
                assert_eq!(
                    persisted.session_binding_nonce.as_deref(),
                    Some("current-nonce")
                );
                assert_eq!(persisted.binding_restorations.len(), usize::from(restore));
            }
        }
    }
}
