use super::*;
use axum::http::HeaderMap;

use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::multiplexer::types::AgentRole;

#[test]
fn staffing_brief_alias_preserves_data_and_rejects_duplicate_inputs() {
    let token = "opaque-brief\n`$()`";
    let body: AddTeamMemberBody = serde_json::from_value(serde_json::json!({
        "provider": "fixture-provider", "role": "cartographer", "brief": token,
    }))
    .unwrap();
    assert_eq!(body.task.as_deref(), Some(token));
    assert!(
        serde_json::from_value::<AddTeamMemberBody>(serde_json::json!({
            "provider": "fixture-provider", "role": "cartographer", "task": "old", "brief": "new",
        }))
        .is_err()
    );
}

fn team_members() -> (MultiplexerState, String, String, String) {
    let (mut mux, orchestrator) = MultiplexerState::new();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&orchestrator)
        .unwrap()
        .team_id;
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let worker = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker, AgentRole::Worker, Some(&orchestrator))
        .unwrap();
    (mux, team_id, orchestrator, worker)
}

#[test]
fn team_rename_errors_have_stable_http_statuses() {
    assert_eq!(
        map_rename_error("Team team_x not found".into()).0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        map_rename_error("Caller Tab worker is not authorized to manage Team team_x".into()).0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(validate_name("   ").unwrap_err().0, StatusCode::BAD_REQUEST);
}

#[test]
fn team_close_errors_keep_authorization_and_cleanup_statuses() {
    assert_eq!(
        map_close_team_error(format!(
            "{}: caller lacks Team authority",
            codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
        ))
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        map_close_team_error("Team lane cleanup failed: disk unavailable".into()).0,
        StatusCode::INTERNAL_SERVER_ERROR
    );
    assert_eq!(
        map_close_team_error(format!(
            "{}: Team has unfinished work",
            codeterm_service_core::service::TEAM_CLOSE_BUSY_REFUSAL_PREFIX
        ))
        .0,
        StatusCode::CONFLICT
    );
}

#[test]
fn team_rename_caller_accepts_headerless_owner_and_attributes_tab_callers() {
    use codeterm_service_core::service::TeamMutationCaller;

    assert_eq!(
        rename_team_caller(&crate::auth::ActorHeaders::for_owner_test(HeaderMap::new())).unwrap(),
        TeamMutationCaller::Human
    );
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", "tab-mgr".parse().unwrap());
    assert_eq!(
        rename_team_caller(&crate::auth::ActorHeaders::for_owner_test(headers)).unwrap(),
        TeamMutationCaller::AgentTab("tab-mgr".into())
    );
    let tab_actor = crate::auth::ActorHeaders::authenticated(
        &crate::auth::CallerIdentity::Tab(
            codeterm_service_core::service::tab_credentials::TabCaller {
                tab_id: "tab-agent".into(),
                binding_nonce: "nonce".into(),
            },
        ),
        crate::auth::AuthenticatedOrigin::Agent,
        HeaderMap::new(),
    )
    .unwrap();
    assert_eq!(
        rename_team_caller(&tab_actor).unwrap(),
        TeamMutationCaller::AgentTab("tab-agent".into())
    );
}

#[test]
fn http_team_rename_returns_updated_team() {
    let (mut mux, team_id, orchestrator, _) = team_members();
    let events = crate::events::EventBroadcaster::new();
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", orchestrator.parse().unwrap());

    let team = rename_team_adapter(
        &crate::auth::ActorHeaders::for_owner_test(headers.clone()),
        team_id.clone(),
        RenameTeamBody {
            name: "Renamed".into(),
        },
        |team_id, caller, name| {
            codeterm_orchestration::team::rename_team_with_state_change(
                &mut mux, &events, &team_id, name, &caller,
            )
        },
    )
    .unwrap();
    assert_eq!(team.id, team_id);
    assert_eq!(team.name.as_deref(), Some("Renamed"));
}

#[test]
fn http_team_rename_enforces_caller_permission() {
    let (mut mux, team_id, _orchestrator, worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    let events = crate::events::EventBroadcaster::new();

    let mut worker_headers = HeaderMap::new();
    worker_headers.insert("X-CodeTerm-Tab-Id", worker.parse().unwrap());
    assert_eq!(
        rename_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(worker_headers.clone()),
            team_id.clone(),
            RenameTeamBody {
                name: "Denied worker".into(),
            },
            |team_id, caller, name| {
                codeterm_orchestration::team::rename_team_with_state_change(
                    &mut mux, &events, &team_id, name, &caller,
                )
            },
        )
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    let mut foreign_headers = HeaderMap::new();
    foreign_headers.insert("X-CodeTerm-Tab-Id", foreign.parse().unwrap());
    assert_eq!(
        rename_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(foreign_headers.clone()),
            team_id.clone(),
            RenameTeamBody {
                name: "Denied foreign".into(),
            },
            |team_id, caller, name| {
                codeterm_orchestration::team::rename_team_with_state_change(
                    &mut mux, &events, &team_id, name, &caller,
                )
            },
        )
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        rename_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(worker_headers.clone()),
            team_id.clone(),
            RenameTeamBody { name: "   ".into() },
            |_, _, _| unreachable!(),
        )
        .unwrap_err()
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        rename_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(worker_headers.clone()),
            "team_missing".into(),
            RenameTeamBody {
                name: "Missing team".into(),
            },
            |team_id, caller, name| {
                codeterm_orchestration::team::rename_team_with_state_change(
                    &mut mux, &events, &team_id, name, &caller,
                )
            },
        )
        .unwrap_err()
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(mux.get_team(&team_id).unwrap().name, None);
    let renamed_by_owner = rename_team_adapter(
        &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
        team_id.clone(),
        RenameTeamBody {
            name: "Owner renamed".into(),
        },
        |team_id, caller, name| {
            codeterm_orchestration::team::rename_team_with_state_change(
                &mut mux, &events, &team_id, name, &caller,
            )
        },
    )
    .unwrap();
    assert_eq!(renamed_by_owner.name.as_deref(), Some("Owner renamed"));
}

#[tokio::test]
async fn http_team_rename_route_authorizes_owner_full_peer_and_agent_callers() {
    use axum::{body::Body, http::Request};
    use codeterm_service_core::mesh::crown::{
        generate_peer_keypair, sign_request_with_agent, sign_request_with_origin,
        AuthenticatedAgent, AuthenticatedOrigin,
    };
    use codeterm_service_core::mesh::types::{
        AccessLevel, PairedPeer, Permission, RELAY_PROTOCOL_VERSION,
    };
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use tower::ServiceExt;

    let (mut mux, team_id, orchestrator, worker) = team_members();
    let workspace_id = mux.find_workspace_for_tab(&orchestrator).unwrap();
    let planner = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role_kind(
        &planner,
        AgentRole::Manager,
        Some("planner"),
        Some(&orchestrator),
    )
    .unwrap();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    let other_ws = mux.create_workspace("Other".into(), None).id;
    let other_orchestrator = mux.create_tab(&other_ws).unwrap();
    mux.resolve_or_mint_team_for_parent(&other_orchestrator)
        .unwrap();
    for tab_id in [
        &orchestrator,
        &planner,
        &worker,
        &foreign,
        &other_orchestrator,
    ] {
        mux.get_tab_mut(tab_id).unwrap().session_binding_nonce = Some(format!("nonce-{tab_id}"));
    }

    let fixture = ServiceFixture::new(mux);
    let state = crate::test_support::app_state(&fixture);
    state.config.lock().api.token = Some("owner-secret".into());
    state.config.lock().mesh.enabled = true;

    let paired_peer = |id: &str, public_key: String, access_level: AccessLevel| PairedPeer {
        instance_id: id.into(),
        instance_name: id.into(),
        addresses: Vec::new(),
        api_port: 0,
        public_key,
        paired_at: "now".into(),
        last_seen: None,
        online: true,
        is_primary: false,
        access_level,
        domios_version: None,
        domios_build: None,
        web_ui: None,
        relay_protocol_version: RELAY_PROTOCOL_VERSION,
        consecutive_misses: 0,
        last_active_ms: None,
    };

    let (full_sk, full_pk) = generate_peer_keypair();
    let (ro_sk, ro_pk) = generate_peer_keypair();
    let (iso_sk, iso_pk) = generate_peer_keypair();
    let (manage_sk, manage_pk) = generate_peer_keypair();
    let (admin_sk, admin_pk) = generate_peer_keypair();
    {
        let mut mesh = state.mesh.lock();
        mesh.insert_account_peer_for_test(paired_peer("peer-full", full_pk, AccessLevel::Full));
        mesh.insert_account_peer_for_test(paired_peer("peer-ro", ro_pk, AccessLevel::ReadOnly));
        mesh.insert_account_peer_for_test(paired_peer(
            "peer-iso",
            iso_pk,
            AccessLevel::AgentIsolated,
        ));
        mesh.insert_account_peer_for_test(paired_peer(
            "peer-manage",
            manage_pk,
            AccessLevel::Custom(vec![Permission::ManageWorkspaces]),
        ));
        mesh.insert_account_peer_for_test(paired_peer(
            "peer-admin",
            admin_pk,
            AccessLevel::Custom(vec![Permission::Admin]),
        ));
    }

    let tab_token = |tab_id: &str| {
        state
            .svc
            .frontend_pty_env(Some(tab_id))
            .unwrap()
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    };
    let orchestrator_token = tab_token(&orchestrator);
    let planner_token = tab_token(&planner);
    let worker_token = tab_token(&worker);
    let foreign_token = tab_token(&foreign);
    let other_orchestrator_token = tab_token(&other_orchestrator);

    let app = crate::routes::create_router(state.clone());
    let send = |token: Option<&str>, body: serde_json::Value| {
        let app = app.clone();
        let uri = format!("/api/teams/{team_id}/rename");
        let token = token.map(str::to_owned);
        async move {
            let mut req = Request::builder()
                .method("POST")
                .uri(uri)
                .header("content-type", "application/json");
            if let Some(token) = token {
                req = req.header("authorization", format!("Bearer {token}"));
            }
            app.oneshot(req.body(Body::from(body.to_string())).unwrap())
                .await
                .unwrap()
                .status()
        }
    };
    let current_name = || {
        fixture
            .service
            .mux
            .read()
            .get_team(&team_id)
            .and_then(|team| team.name.clone())
    };

    assert_eq!(
        send(None, serde_json::json!({ "name": "No auth" })).await,
        StatusCode::UNAUTHORIZED
    );

    // Local owner (headerless GUI/API call) succeeds.
    assert_eq!(
        send(
            Some("owner-secret"),
            serde_json::json!({ "name": "Owner Name" })
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(current_name().as_deref(), Some("Owner Name"));

    // Authenticated Full peer acting for the owner succeeds.
    let full_owner_token =
        sign_request_with_origin(&full_sk, "peer-full", AuthenticatedOrigin::Owner).unwrap();
    assert_eq!(
        send(
            Some(&full_owner_token),
            serde_json::json!({ "name": "Remote Owner Name" }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(current_name().as_deref(), Some("Remote Owner Name"));

    // Full peer claiming a local tab or using Agent/Mesh origin is refused.
    assert_eq!(
        send(
            Some(&full_owner_token),
            serde_json::json!({
                "name": "Spoofed Tab",
                "caller_tab_id": orchestrator,
            }),
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let full_mesh_token =
        sign_request_with_origin(&full_sk, "peer-full", AuthenticatedOrigin::Mesh).unwrap();
    assert_eq!(
        send(
            Some(&full_mesh_token),
            serde_json::json!({ "name": "Mesh Origin" }),
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let full_agent_token = sign_request_with_agent(
        &full_sk,
        "peer-full",
        AuthenticatedOrigin::Agent,
        Some(&AuthenticatedAgent {
            instance_id: "peer-full".into(),
            tab_id: "remote-agent".into(),
            can_spawn: true,
        }),
    )
    .unwrap();
    assert_eq!(
        send(
            Some(&full_agent_token),
            serde_json::json!({ "name": "Remote Agent" }),
        )
        .await,
        StatusCode::FORBIDDEN
    );

    // Non-Full peers acting for the owner are refused.
    for (peer_id, sk) in [
        ("peer-ro", &ro_sk),
        ("peer-iso", &iso_sk),
        ("peer-manage", &manage_sk),
        ("peer-admin", &admin_sk),
    ] {
        let token = sign_request_with_origin(sk, peer_id, AuthenticatedOrigin::Owner).unwrap();
        assert_eq!(
            send(
                Some(&token),
                serde_json::json!({ "name": format!("Refused {peer_id}") }),
            )
            .await,
            StatusCode::FORBIDDEN,
            "{peer_id}"
        );
    }
    assert_eq!(current_name().as_deref(), Some("Remote Owner Name"));

    // Agent / Tab callers keep their exact Team authority rules.
    assert_eq!(
        send(
            Some(&orchestrator_token),
            serde_json::json!({ "name": "Orchestrator Name" }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(current_name().as_deref(), Some("Orchestrator Name"));
    assert_eq!(
        send(
            Some(&planner_token),
            serde_json::json!({ "name": "Planner Name" }),
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(current_name().as_deref(), Some("Planner Name"));
    for (label, token) in [
        ("worker", worker_token.as_str()),
        ("foreign", foreign_token.as_str()),
        ("other_orchestrator", other_orchestrator_token.as_str()),
    ] {
        assert_eq!(
            send(
                Some(token),
                serde_json::json!({ "name": format!("Denied {label}") }),
            )
            .await,
            StatusCode::FORBIDDEN,
            "{label}"
        );
    }
    assert_eq!(current_name().as_deref(), Some("Planner Name"));
}

#[tokio::test]
async fn http_close_team_enforces_permission() {
    let (mut mux, team_id, orchestrator, worker) = team_members();
    let events = crate::events::EventBroadcaster::new();
    let plan_root = tempfile::tempdir().unwrap();
    let plan_store =
        codeterm_service_core::service::plan::PlanStore::new_for_test(plan_root.path());
    assert_eq!(
        close_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
            team_id.clone(),
            CloseTeamBody::default(),
            async |team_id, caller| {
                codeterm_orchestration::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    &caller,
                    1,
                )
                .map(|result| result.0)
            }
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    let mut worker_headers = HeaderMap::new();
    worker_headers.insert("X-CodeTerm-Tab-Id", worker.parse().unwrap());
    assert_eq!(
        close_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(worker_headers.clone()),
            team_id.clone(),
            CloseTeamBody::default(),
            async |team_id: String, caller| {
                codeterm_orchestration::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    &caller,
                    1,
                )
                .map(|result| result.0)
            }
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    let workspace_id = mux.find_workspace_for_tab(&worker).unwrap();
    let foreign = mux.create_tab(&workspace_id).unwrap();
    let mut foreign_headers = HeaderMap::new();
    foreign_headers.insert("X-CodeTerm-Tab-Id", foreign.parse().unwrap());
    assert_eq!(
        close_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(foreign_headers.clone()),
            team_id.clone(),
            CloseTeamBody::default(),
            async |team_id: String, caller| {
                codeterm_orchestration::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    &caller,
                    1,
                )
                .map(|result| result.0)
            }
        )
        .await
        .unwrap_err()
        .0,
        StatusCode::FORBIDDEN
    );
    let mut manager_headers = HeaderMap::new();
    manager_headers.insert("X-CodeTerm-Tab-Id", orchestrator.parse().unwrap());
    assert_eq!(
        close_team_adapter(
            &crate::auth::ActorHeaders::for_owner_test(manager_headers.clone()),
            team_id.clone(),
            CloseTeamBody::default(),
            async |team_id: String, caller| {
                codeterm_orchestration::close_team_with_state_change(
                    &mut mux,
                    &events,
                    &plan_store,
                    &team_id,
                    &caller,
                    1,
                )
                .map(|result| result.0)
            }
        )
        .await
        .unwrap()
        .id,
        team_id
    );
}

#[test]
fn headerless_team_close_requires_explicit_confirmation() {
    use codeterm_service_core::service::TeamMutationCaller;

    let team_id = "team-1";
    assert!(matches!(
        close_team_caller(
            &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
            team_id,
            &CloseTeamBody::default()
        ),
        TeamMutationCaller::Unattributed
    ));
    assert!(matches!(
        close_team_caller(
            &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
            team_id,
            &CloseTeamBody {
                force: false,
                confirm: Some(team_id.into()),
                force_busy: false,
            },
        ),
        TeamMutationCaller::Unattributed
    ));
    assert!(matches!(
        close_team_caller(
            &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
            team_id,
            &CloseTeamBody {
                force: true,
                confirm: Some("team-other".into()),
                force_busy: false,
            },
        ),
        TeamMutationCaller::Unattributed
    ));
    let confirmed = close_team_caller(
        &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
        team_id,
        &CloseTeamBody {
            force: true,
            confirm: Some(team_id.into()),
            force_busy: false,
        },
    );
    assert_eq!(confirmed, TeamMutationCaller::HumanConfirmedHeaderless,);
}

/// codeterm#418: the recovery add is the one Team verb a headless Team can
/// still reach, so an absent caller header travels as "no caller".
#[tokio::test]
async fn http_add_team_member_passes_an_absent_caller_tab_through() {
    let caller = add_team_member_adapter(
        &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
        "team_x".into(),
        AddTeamMemberBody {
            provider: "provider".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
        |_, caller_tab_id, _| async move { Ok(caller_tab_id) },
    )
    .await
    .expect("a caller-less add reaches the service");
    assert_eq!(caller, None);
}

#[tokio::test]
async fn http_add_team_member_delegates_acting_tab_and_body() {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", "manager".parse().unwrap());
    let result = add_team_member_adapter(
        &crate::auth::ActorHeaders::for_owner_test(headers.clone()),
        "team_x".into(),
        AddTeamMemberBody {
            provider: "provider".into(),
            role: "planner".into(),
            title: Some("Planner".into()),
            task: Some("Plan it".into()),
            model: Some("model".into()),
        },
        |team_id, caller_tab_id: Option<String>, body: AddTeamMemberBody| async move {
            assert_eq!(team_id, "team_x");
            assert_eq!(caller_tab_id.as_deref(), Some("manager"));
            assert_eq!(body.role, "planner");
            Ok((body.provider, body.title, body.task, body.model))
        },
    )
    .await
    .unwrap();
    assert_eq!(
        result,
        (
            "provider".into(),
            Some("Planner".into()),
            Some("Plan it".into()),
            Some("model".into()),
        )
    );
}

#[tokio::test]
async fn http_cartographer_add_keeps_the_role_kind_in_the_staffing_contract() {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", "manager".parse().unwrap());
    let result = add_team_member_adapter(
        &crate::auth::ActorHeaders::for_owner_test(headers.clone()),
        "team_x".into(),
        AddTeamMemberBody {
            provider: "provider-neutral".into(),
            role: "cartographer".into(),
            title: None,
            task: None,
            model: None,
        },
        |team_id, caller_tab_id: Option<String>, body: AddTeamMemberBody| async move {
            assert_eq!(team_id, "team_x");
            assert_eq!(caller_tab_id.as_deref(), Some("manager"));
            Ok((body.role, body.provider))
        },
    )
    .await
    .unwrap();

    assert_eq!(result.0, "cartographer");
    assert_eq!(result.1, "provider-neutral");
}

#[test]
fn close_inventory_timeout_and_busy_have_distinct_http_statuses() {
    let timeout = serde_json::json!({
        "type": "close_inventory_timeout",
        "message": "capture deadline"
    })
    .to_string();
    let (status, body) = map_close_team_error(timeout.clone());
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(body, timeout);
    for kind in ["close_inventory_busy", "close_scope_busy"] {
        let error = serde_json::json!({"type": kind, "message": "scope occupied"}).to_string();
        assert_eq!(map_close_team_error(error).0, StatusCode::CONFLICT);
    }
}

#[tokio::test]
async fn http_team_add_keeps_typed_spawn_target_refusals() {
    let error = add_team_member_adapter::<()>(
        &crate::auth::ActorHeaders::for_owner_test(HeaderMap::new()),
        "team_x".into(),
        AddTeamMemberBody {
            provider: "plain".into(),
            role: "orchestrator".into(),
            title: None,
            task: None,
            model: None,
        },
        |_, _, _| async {
            Err(SpawnTargetRefusal::ExecutionSandbox {
                provider: "plain".into(),
                message: "opaque confinement refusal".into(),
            })
        },
    )
    .await
    .unwrap_err();
    assert_eq!(error.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error.body["error"]["code"], "execution_sandbox_unsupported");
    assert_eq!(error.body["error"]["message"], "opaque confinement refusal");
}
