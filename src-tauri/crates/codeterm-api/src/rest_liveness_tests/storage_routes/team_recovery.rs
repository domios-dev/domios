use super::*;
use codeterm_service_core::plan::{OrchestratorRef, PlanStatus};

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn team_takeover_unarchive_and_planner_lookup_release_mux_while_sql_is_held() {
    for operation_kind in ["takeover", "takeover_sql_failure", "unarchive", "planner"] {
        for change_context in [false, true] {
            codeterm_orchestration::register_orchestration();
            let (mut mux, owner) = MultiplexerState::new();
            let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
            let workspace = mux.find_workspace_for_tab(&owner).unwrap();
            let replacement = mux.create_tab(&workspace).unwrap();
            mux.set_orchestrator_role(&replacement).unwrap();
            let fixture = ServiceFixture::new(mux);
            let state = app_state(&fixture);
            let store = state.svc.plan_store.clone();
            let plan = store
                .create_plan_for_team(
                    "Recovery liveness".into(),
                    fixture.temp.path().display().to_string(),
                    OrchestratorRef::from_tab(&owner),
                    team.clone(),
                    vec![],
                    1,
                )
                .unwrap();
            if operation_kind.starts_with("takeover") {
                state.svc.mux.write().close_tab(&owner).unwrap();
            } else if operation_kind == "unarchive" {
                store.archive(&plan.id, 2).unwrap();
            }
            if operation_kind == "takeover_sql_failure" {
                rusqlite::Connection::open(store.database_path_for_test()).unwrap().execute_batch(
                    "CREATE TRIGGER refuse_takeover BEFORE UPDATE ON plans BEGIN SELECT RAISE(ABORT, 'injected takeover failure'); END;"
                ).unwrap();
            }
            let held_store = store.clone();
            let held = HeldDependency::new(move |entered, release| {
                held_store.with_database_held_for_test(|| {
                    entered.send(()).unwrap();
                    release.recv_timeout(Duration::from_secs(10)).unwrap();
                });
            })
            .await;
            let service = state.svc.clone();
            let (plan_id, team_id, tab_id) = (plan.id.clone(), team.clone(), replacement.clone());
            let (lookup_entered, lookup_started) = tokio::sync::oneshot::channel();
            let operation = tokio::task::spawn_blocking(move || match operation_kind {
                "takeover" | "takeover_sql_failure" => {
                    codeterm_orchestration::team::commit_team_member_prepared(
                        &service,
                        &team_id,
                        None,
                        &tab_id,
                        "Manager",
                        &Some("orchestrator".into()),
                        true,
                        3,
                    )
                    .map(|_| ())
                    .map_err(|error| format!("{error:?}"))
                }
                "unarchive" => {
                    codeterm_orchestration::team::unarchive_team_plan(&service, &plan_id, 3)
                        .map_err(|error| format!("{error:?}"))
                }
                _ => service
                    .resolve_planner_parent_for_spawn_observed(&plan_id, || {
                        lookup_entered.send(()).unwrap();
                    })
                    .map(|_| ()),
            });
            if operation_kind != "planner" {
                let observer = store.clone();
                assert!(tokio::task::spawn_blocking(
                    move || observer.wait_for_active_mutation_for_test(Duration::from_secs(2))
                )
                .await
                .unwrap());
            } else {
                lookup_started.await.unwrap();
            }
            assert!(!operation.is_finished());
            let available = state.svc.mux.try_write().is_some();
            if available && change_context {
                if operation_kind == "planner" {
                    state.svc.mux.write().close_tab(&owner).unwrap();
                } else {
                    state
                        .svc
                        .mux
                        .write()
                        .get_tab_mut(&replacement)
                        .unwrap()
                        .agent_session_id = Some("new-launch".into());
                }
            }
            crate::rest_liveness_tests::assert_health_list_send_respond(state.clone()).await;
            drop(held);
            let result = operation.await.unwrap();
            assert!(available, "SQL wait must not retain mux");
            if change_context {
                let error =
                    result.expect_err(&format!("{operation_kind} must refuse the changed context"));
                assert!(
                    error.contains(if operation_kind == "planner" {
                        "not live"
                    } else {
                        "conflict"
                    }),
                    "{operation_kind}: {error}"
                );
                let current = store.get(&plan.id).unwrap();
                assert_eq!(current.owner, plan.owner);
                assert_eq!(
                    current.status,
                    if operation_kind == "unarchive" {
                        PlanStatus::Archived
                    } else {
                        PlanStatus::Active
                    }
                );
                if operation_kind.starts_with("takeover") {
                    assert_eq!(
                        state
                            .svc
                            .mux
                            .read()
                            .get_tab(&replacement)
                            .unwrap()
                            .agent_session_id
                            .as_deref(),
                        Some("new-launch")
                    );
                }
            } else if operation_kind == "takeover_sql_failure" {
                assert!(result.unwrap_err().contains("injected takeover failure"));
                assert!(state.svc.mux.read().get_tab(&replacement).is_none());
                assert_eq!(store.get(&plan.id).unwrap(), plan);
            } else {
                result.unwrap();
                let current = store.get(&plan.id).unwrap();
                assert_eq!(current.status, PlanStatus::Active);
                if operation_kind.starts_with("takeover") {
                    assert_eq!(current.owner.tab_id.as_deref(), Some(replacement.as_str()));
                    assert_eq!(
                        state
                            .svc
                            .mux
                            .read()
                            .get_tab(&replacement)
                            .unwrap()
                            .team_id
                            .as_deref(),
                        Some(team.as_str())
                    );
                }
            }
        }
    }
}
