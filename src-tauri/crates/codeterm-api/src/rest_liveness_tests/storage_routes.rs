use super::*;
use axum::extract::{Path, Query, State};
use futures_util::FutureExt;

pub(super) struct HeldDependency {
    release: Option<std::sync::mpsc::Sender<()>>,
    holder: Option<std::thread::JoinHandle<()>>,
}

impl HeldDependency {
    pub(super) async fn new(
        hold: impl FnOnce(tokio::sync::oneshot::Sender<()>, std::sync::mpsc::Receiver<()>)
            + Send
            + 'static,
    ) -> Self {
        let (locked, ready) = tokio::sync::oneshot::channel();
        let (release, receive) = std::sync::mpsc::channel();
        let holder = std::thread::spawn(move || hold(locked, receive));
        ready.await.unwrap();
        Self {
            release: Some(release),
            holder: Some(holder),
        }
    }
}

impl Drop for HeldDependency {
    fn drop(&mut self) {
        self.release.take().unwrap().send(()).ok();
        self.holder.take().unwrap().join().unwrap();
    }
}

async fn health_responds(router: Router) {
    assert_eq!(tokio::spawn(async { 42 }).await.unwrap(), 42);
    let response = tokio::time::timeout(
        Duration::from_millis(250),
        router.oneshot(request("/api/health")),
    )
    .await
    .expect("health must answer while the dependency is held")
    .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), 16 * 1024)
        .await
        .unwrap();
    let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(body["status"], "ok");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_services_and_activity_wait_on_sql() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let connection = state.svc.services_store.conn();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = connection.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut activity = Box::pin(crate::services_routes::list_activity(
        State(state.clone()),
        Default::default(),
        Query(crate::services_routes::ActivityQuery {
            source: None,
            since: None,
            limit: Some(usize::MAX),
        }),
    ));
    let mut services = Box::pin(crate::services_routes::list_services(
        State(state.clone()),
        Default::default(),
        Query(crate::services_routes::ListQuery {
            kind: None,
            enabled: None,
        }),
    ));
    state.config.lock().api.token = Some("inventory-fixture".into());
    let mut inventory_headers = axum::http::HeaderMap::new();
    inventory_headers.insert("authorization", "Bearer inventory-fixture".parse().unwrap());
    let mut inventory = Box::pin(crate::mesh_inventory::inventory(
        State(state.clone()),
        inventory_headers,
    ));
    assert!(activity.as_mut().now_or_never().is_none());
    assert!(services.as_mut().now_or_never().is_none());
    assert!(inventory.as_mut().now_or_never().is_none());
    health_responds(router).await;
    let cron = state.cron.clone();
    tokio::time::timeout(
        Duration::from_millis(250),
        tokio::task::spawn_blocking(move || cron.lock().list().len()),
    )
    .await
    .expect("cron must remain independent of service SQL")
    .unwrap();
    assert!(state.svc.services_store.conn().try_lock().is_none());
    drop(held);
    assert!(activity.await.unwrap().0.is_empty());
    assert!(services.await.unwrap().0.is_empty());
    assert!(inventory.await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_task_list_and_show_wait_on_plan_sql() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let plans = state.svc.plan_store.clone();
    let held = HeldDependency::new(move |locked, release| {
        plans.with_database_held_for_test(|| {
            locked.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(3)).ok();
        });
    })
    .await;
    let mut list = Box::pin(crate::task_routes::list(
        State(state.clone()),
        Default::default(),
        Query(crate::task_routes::ListTasksQuery {
            project: None,
            active: None,
            all: None,
            mine: None,
            unassigned: None,
            plan: None,
            status: None,
        }),
    ));
    let mut show = Box::pin(crate::task_routes::show(
        State(state.clone()),
        Path("missing".into()),
    ));
    assert!(list.as_mut().now_or_never().is_none());
    assert!(show.as_mut().now_or_never().is_none());
    health_responds(router).await;
    assert!(list.as_mut().now_or_never().is_none());
    assert!(show.as_mut().now_or_never().is_none());
    drop(held);
    assert_eq!(list.await.unwrap().0["tasks"], serde_json::json!([]));
    assert_eq!(show.await.unwrap_err().0, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_theme_preparation_waits_on_config() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let config = state.config.clone();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = config.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut apply = Box::pin(crate::theme_routes::apply_theme_with(
        state.clone(),
        serde_json::from_value(serde_json::json!({"name":"github-dark"})).unwrap(),
        |_, _| Err("fixture persistence refusal".into()),
    ));
    assert!(apply.as_mut().now_or_never().is_none());
    health_responds(router).await;
    assert!(state.config.try_lock().is_none());
    assert!(apply.as_mut().now_or_never().is_none());
    drop(held);
    assert_eq!(apply.await.unwrap_err().1, "fixture persistence refusal");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rest_health_answers_while_notification_reads_and_writes_wait_on_store() {
    let fixture = ServiceFixture::new(MultiplexerState::empty());
    let state = app_state(&fixture);
    let router = crate::routes::create_router(state.clone());
    let notifications = state.svc.notif.clone();
    let held = HeldDependency::new(move |locked, release| {
        let _guard = notifications.lock();
        locked.send(()).unwrap();
        release.recv_timeout(Duration::from_secs(3)).ok();
    })
    .await;
    let mut list = Box::pin(crate::notification_routes::list_notifications(
        State(state.clone()),
        Default::default(),
        Query(serde_json::from_value(serde_json::json!({})).unwrap()),
    ));
    let mut mark = Box::pin(crate::notification_routes::mark_notification_read(
        State(state.clone()),
        Default::default(),
        Path("missing".into()),
    ));
    assert!(list.as_mut().now_or_never().is_none());
    assert!(mark.as_mut().now_or_never().is_none());
    health_responds(router).await;
    assert!(state.svc.notif.try_lock().is_none());
    drop(held);
    assert!(list.await.unwrap().0.as_array().unwrap().is_empty());
    assert_eq!(mark.await.unwrap_err().0, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn team_adopt_and_transfer_keep_health_list_and_send_responsive_with_plan_sql_held() {
    for transfer in [false, true] {
        codeterm_orchestration::register_orchestration();
        let (mut mux, source) = MultiplexerState::new();
        mux.set_orchestrator_role(&source).unwrap();
        let source_team = mux
            .resolve_or_mint_team_for_parent(&source)
            .unwrap()
            .team_id;
        let workspace = mux.find_workspace_for_tab(&source).unwrap();
        let destination = mux.create_tab(&workspace).unwrap();
        mux.set_orchestrator_role(&destination).unwrap();
        let destination_team = mux
            .resolve_or_mint_team_for_parent(&destination)
            .unwrap()
            .team_id;
        let fixture = ServiceFixture::new(mux);
        let state = app_state(&fixture);
        let plan = state
            .svc
            .plan_store
            .create_plan_for_team(
                "Held SQL".into(),
                fixture.temp.path().display().to_string(),
                codeterm_service_core::plan::OrchestratorRef::from_tab(&source),
                source_team,
                vec![],
                1,
            )
            .unwrap();
        let plans = state.svc.plan_store.clone();
        let held = HeldDependency::new(move |locked, release| {
            plans.with_database_held_for_test(|| {
                locked.send(()).unwrap();
                release.recv_timeout(Duration::from_secs(10)).unwrap();
            });
        })
        .await;
        let service = state.svc.clone();
        let plan_id = plan.id.clone();
        let expected_team = destination_team.clone();
        let operation = tokio::task::spawn_blocking(move || {
            if transfer {
                codeterm_orchestration::team::transfer_team_plan(
                    &service,
                    &plan_id,
                    codeterm_service_core::plan::OrchestratorRef::from_tab(&destination),
                    "held SQL transfer",
                    source,
                    2,
                )
            } else {
                codeterm_orchestration::team::adopt_plan_for_team(
                    &service,
                    &destination_team,
                    &plan_id,
                    codeterm_service_core::service::team_core::TeamMutationCaller::AgentTab(
                        destination,
                    ),
                    2,
                )
            }
        });
        let observer = state.svc.plan_store.clone();
        assert!(
            tokio::task::spawn_blocking(
                move || observer.wait_for_active_mutation_for_test(Duration::from_secs(2))
            )
            .await
            .unwrap(),
            "operation must enter the held SQL dependency"
        );
        assert!(!operation.is_finished());
        health_responds(crate::routes::create_router(state.clone())).await;
        let list = tokio::time::timeout(
            Duration::from_secs(1),
            crate::tab_routes::list_tabs(
                State(state.clone()),
                owner_headers(&state),
                Query(crate::tab_routes::TabListQuery {
                    local_only: true,
                    ..Default::default()
                }),
            ),
        )
        .await
        .expect("tab list must not wait for plan SQL")
        .unwrap();
        assert_eq!(list.0.as_array().unwrap().len(), 2);
        let send = tokio::time::timeout(
            Duration::from_secs(1),
            crate::tab_routes::send_text(
                State(state.clone()),
                owner_headers(&state),
                Path("missing-held-dependency-target".into()),
                axum::Json(serde_json::from_value(serde_json::json!({"text":"opaque"})).unwrap()),
            ),
        )
        .await
        .expect("send preflight must not wait for plan SQL")
        .unwrap_err();
        let response = axum::response::IntoResponse::into_response(send);
        let bytes = axum::body::to_bytes(response.into_body(), 16 * 1024)
            .await
            .unwrap();
        let rejection: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(rejection["error"]["code"], "tab_not_reachable_on_peer");
        assert!(!operation.is_finished());
        drop(held);
        let adopted = operation.await.unwrap().unwrap();
        assert_eq!(adopted.team_id.as_deref(), Some(expected_team.as_str()));
        assert_eq!(
            state.svc.plan_store.get(&plan.id).unwrap().team_id,
            adopted.team_id
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn acknowledged_dispatch_binding_keeps_mux_and_startup_registry_free_while_sql_is_held() {
    use codeterm_service_core::service::plan::plan_skills::{
        create_plan_skill, plan_skill_manifest, NewPlanSkill,
    };
    use codeterm_service_core::service::startup_lease::{StartupAuthority, StartupTaskIdentity};
    let (mut mux, creator) = MultiplexerState::new();
    mux.set_orchestrator_role(&creator).unwrap();
    let team = mux
        .resolve_or_mint_team_for_parent(&creator)
        .unwrap()
        .team_id;
    let workspace = mux.find_workspace_for_tab(&creator).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(
        &worker,
        codeterm_service_core::multiplexer::types::AgentRole::Worker,
        Some(&creator),
    )
    .unwrap();
    mux.get_tab_mut(&creator).unwrap().agent_type = Some("opaque-provider".into());
    mux.link_pty(&worker, "retained-launch-pty".into());
    let tab = mux.get_tab_mut(&worker).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("retained-session".into());
    tab.session_binding_nonce = Some("retained-nonce".into());
    tab.reported_session_binding_nonce = Some("retained-nonce".into());
    tab.boot_stage = Some(codeterm_service_core::service::boot_stage::BootStage::AgentReady);
    let fixture = ServiceFixture::new(mux);
    let state = app_state(&fixture);
    let store = &state.svc.plan_store;
    let plan = store
        .create_plan_for_team(
            "Retained binding".into(),
            fixture.temp.path().display().to_string(),
            codeterm_service_core::plan::OrchestratorRef::from_tab(&creator),
            team.clone(),
            vec![],
            1,
        )
        .unwrap();
    let task = store
        .add_task(
            &plan.id,
            codeterm_service_core::service::plan::NewTask {
                name: "retained binding task".into(),
                kind: "task".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    let attempt = store
        .begin_task_dispatch_with_policy(
            &plan.id,
            &task,
            &worker,
            "Worker",
            codeterm_service_core::plan::TaskLaneMetadata::default(),
            codeterm_service_core::plan::ExecutionIntent::Unspecified,
            None,
            Some("opaque-provider"),
            3,
        )
        .unwrap();
    create_plan_skill(
        store,
        &plan.id,
        NewPlanSkill {
            slug: "retained-binding".into(),
            title: None,
            area: None,
            check: None,
            rationale: None,
        },
    )
    .unwrap();
    store
        .set_attempt_plan_skill_manifest(
            &plan.id,
            &task,
            &attempt,
            plan_skill_manifest(store, &plan.id).unwrap(),
            4,
        )
        .unwrap();
    store
        .acknowledge_task_attempt(&plan.id, &task, &worker, &attempt, 5)
        .unwrap();
    let ack = store
        .acknowledge_task_attempt(&plan.id, &task, &worker, &attempt, 6)
        .unwrap();
    assert!(ack.already_acknowledged);
    state
        .svc
        .mux
        .write()
        .get_tab_mut(&worker)
        .unwrap()
        .task_binding = Some(codeterm_service_core::multiplexer::types::TaskBinding {
        plan_id: Some(plan.id.clone()),
        task_id: Some(task.clone()),
        attempt_id: Some(attempt.clone()),
        spawn_handle: None,
        bound_at: 3,
    });
    let _armed = state
        .svc
        .worker_ack
        .retain_task_startup_for_test(StartupAuthority {
            creator_tab_id: creator,
            child_tab_id: worker.clone(),
            attempt_id: "retained-launch-pty".into(),
            team_id: Some(team),
            task_identity: Some(StartupTaskIdentity {
                plan_id: plan.id.clone(),
                task_id: task.clone(),
                attempt_id: attempt.clone(),
            }),
            session_id: None,
            binding_nonce: Some("retained-nonce".into()),
            task_owner_epoch: Some(ack.attempt.interruption.owner_epoch),
        });
    let database = store.clone();
    let held = HeldDependency::new(move |entered, release| {
        database.with_database_held_for_test(|| {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(10)).unwrap();
        });
    })
    .await;
    let service = state.svc.clone();
    let completing = tokio::task::spawn_blocking(move || {
        service.complete_acknowledged_dispatch_binding(
            &ack,
            "retained-nonce",
            Some("retained-session"),
            7,
        )
    });
    let observer = store.clone();
    assert!(tokio::task::spawn_blocking(
        move || observer.wait_for_active_mutation_for_test(Duration::from_secs(2))
    )
    .await
    .unwrap());
    assert!(!completing.is_finished());
    assert!(
        state.svc.mux.try_write().is_some(),
        "SQL must not retain a mux reader"
    );
    assert!(
        state.svc.worker_ack.launch_registry_available_for_test(),
        "SQL must not retain startup registry guards"
    );
    crate::rest_liveness_tests::assert_health_list_send_respond(state.clone()).await;
    assert!(!completing.is_finished());
    drop(held);
    let completed = completing.await.unwrap().unwrap();
    assert_eq!(completed.session_id.as_deref(), Some("retained-session"));
    assert_eq!(
        completed.session_binding_nonce.as_deref(),
        Some("retained-nonce")
    );
    let stored = store.get(&plan.id).unwrap();
    assert_eq!(
        stored
            .tasks
            .iter()
            .find(|item| item.id == task)
            .unwrap()
            .execution_attempts
            .iter()
            .find(|item| item.id == attempt)
            .unwrap(),
        &completed
    );
}

mod restoration;

mod checkpoint_adoption;

mod team_recovery;

mod orphan_executor;

mod preview_encoding;

mod system_heartbeats;

mod hook_journal;
