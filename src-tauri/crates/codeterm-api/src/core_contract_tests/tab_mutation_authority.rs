use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use codeterm_service_core::{
    multiplexer::{state::MultiplexerState, types::AgentRole},
    service::test_fixture::ServiceFixture,
};
use serde_json::{json, Value};
use tower::ServiceExt;

struct Fixture {
    _service: ServiceFixture,
    state: crate::routes::AppState,
    owner: String,
    member: String,
    worker: String,
    foreign: String,
    plan: String,
    foreign_plan: String,
}

impl Fixture {
    fn new() -> Self {
        let (mut mux, initial) = MultiplexerState::new();
        let workspace = mux.find_workspace_for_tab(&initial).unwrap();
        let owner = mux.create_tab(&workspace).unwrap();
        let member = mux.create_tab(&workspace).unwrap();
        let worker = mux.create_tab(&workspace).unwrap();
        let foreign = mux.create_tab(&workspace).unwrap();
        for id in [&owner, &foreign] {
            mux.set_orchestrator_role(id).unwrap();
            mux.resolve_or_mint_team_for_parent(id).unwrap();
        }
        mux.set_tab_role_kind(&member, AgentRole::Manager, Some("planner"), Some(&owner))
            .unwrap();
        mux.set_tab_role(&worker, AgentRole::Worker, Some(&owner))
            .unwrap();
        for id in [&owner, &member, &worker, &foreign] {
            let tab = mux.get_tab_mut(id).unwrap();
            tab.agent_type = Some("fixture-provider".into());
            tab.agent_session_id = Some(format!("session-{id}"));
            tab.session_binding_nonce = Some(format!("nonce-{id}"));
            tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        }
        let service = ServiceFixture::new(mux);
        service.set_limit_gate(codeterm_service_core::service::billing::LimitGate::dormant());
        let state = crate::test_support::app_state(&service);
        state.config.lock().api.token = Some("host-fixture".into());
        let mut plans = Vec::new();
        for id in [&owner, &foreign] {
            let mut plan = codeterm_service_core::plan::Plan::new(
                "Authority".into(),
                service.temp.path().display().to_string(),
                codeterm_service_core::plan::OrchestratorRef::from_tab(id),
            );
            plan.team_id = state.svc.mux.read().get_tab(id).unwrap().team_id.clone();
            state.svc.plan_store.save(&mut plan, 1).unwrap();
            plans.push(plan.id);
        }
        Self {
            _service: service,
            state,
            owner,
            member,
            worker,
            foreign,
            plan: plans.remove(0),
            foreign_plan: plans.remove(0),
        }
    }

    fn standalone(&self) -> String {
        let mut mux = self.state.svc.mux.write();
        let workspace = mux.find_workspace_for_tab(&self.owner).unwrap();
        let id = mux.create_tab(&workspace).unwrap();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("fixture-provider".into());
        tab.agent_session_id = Some(format!("session-{id}"));
        tab.session_binding_nonce = Some(format!("nonce-{id}"));
        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
        id
    }

    fn token(&self, id: &str) -> String {
        self.state
            .svc
            .frontend_pty_env(Some(id))
            .remove("CODETERM_API_TOKEN")
            .unwrap()
    }

    async fn mutate(
        &self,
        token: &str,
        target: &str,
        method: &str,
        suffix: &str,
        body: Value,
    ) -> StatusCode {
        crate::routes::create_router(self.state.clone())
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(format!("/api/tabs/{target}/{suffix}"))
                    .header("authorization", format!("Bearer {token}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    fn identity(&self, id: &str) -> Value {
        let mux = self.state.svc.mux.read();
        let tab = mux.get_tab(id).unwrap();
        json!([
            tab.role,
            tab.role_profile,
            tab.team_id,
            tab.parent_tab_id,
            tab.can_spawn
        ])
    }
}

#[tokio::test]
async fn tab_mutation_routes_refuse_self_foreign_and_worker_callers_without_effects() {
    let fixture = Fixture::new();
    for (caller, target) in [
        (&fixture.owner, &fixture.owner),
        (&fixture.worker, &fixture.worker),
        (&fixture.worker, &fixture.member),
        (&fixture.foreign, &fixture.member),
        (&fixture.owner, &fixture.foreign),
    ] {
        let before = fixture.identity(target);
        let token = fixture.token(caller);
        for (method, suffix, body) in [
            (
                "POST",
                "role",
                json!({"role":"Worker","parent_tab_id":fixture.owner}),
            ),
            ("DELETE", "role", json!({})),
            ("POST", "can-spawn", json!({"can_spawn":true})),
            ("POST", "make-orchestrator", json!({})),
            ("POST", "make-planner", json!({"plan_id":fixture.plan})),
        ] {
            assert_eq!(
                fixture.mutate(&token, target, method, suffix, body).await,
                StatusCode::FORBIDDEN,
                "{caller} -> {target}: {suffix}"
            );
            assert_eq!(fixture.identity(target), before);
        }
    }
}

#[tokio::test]
async fn tab_mutation_routes_keep_host_and_same_team_orchestrator_access() {
    for host in [false, true] {
        let fixture = Fixture::new();
        let token = if host {
            "host-fixture".into()
        } else {
            fixture.token(&fixture.owner)
        };
        assert_eq!(
            fixture
                .mutate(
                    &token,
                    &fixture.member,
                    "POST",
                    "can-spawn",
                    json!({"can_spawn":true})
                )
                .await,
            StatusCode::OK
        );
        assert!(
            fixture
                .state
                .svc
                .mux
                .read()
                .get_tab(&fixture.member)
                .unwrap()
                .can_spawn
        );
        assert_eq!(
            fixture
                .mutate(
                    &token,
                    &fixture.member,
                    "POST",
                    "make-planner",
                    json!({"plan_id":fixture.plan})
                )
                .await,
            StatusCode::OK
        );
        assert!(
            matches!(fixture.state.svc.plan_store.get(&fixture.plan).unwrap().planner,
            Some(codeterm_service_core::plan::PlannerRef::Tab { tab_id, .. }) if tab_id == fixture.member)
        );
        assert_eq!(
            fixture
                .mutate(
                    &token,
                    &fixture.member,
                    "POST",
                    "role",
                    json!({"role":"Planner","plan_id":fixture.plan})
                )
                .await,
            StatusCode::OK
        );
        assert_eq!(
            fixture
                .mutate(
                    &token,
                    &fixture.member,
                    "POST",
                    "role",
                    json!({"role":"Worker","parent_tab_id":fixture.owner})
                )
                .await,
            StatusCode::OK
        );
        assert_eq!(
            fixture
                .state
                .svc
                .mux
                .read()
                .get_tab(&fixture.member)
                .unwrap()
                .role,
            Some(AgentRole::Worker)
        );
        assert_eq!(
            fixture
                .mutate(&token, &fixture.member, "DELETE", "role", json!({}))
                .await,
            StatusCode::OK
        );
        assert!(fixture
            .state
            .svc
            .mux
            .read()
            .get_tab(&fixture.member)
            .unwrap()
            .role
            .is_none());
    }
    let fixture = Fixture::new();
    assert_eq!(
        fixture
            .mutate(
                "host-fixture",
                &fixture.foreign,
                "POST",
                "can-spawn",
                json!({"can_spawn":false})
            )
            .await,
        StatusCode::OK
    );
    assert!(
        !fixture
            .state
            .svc
            .mux
            .read()
            .get_tab(&fixture.foreign)
            .unwrap()
            .can_spawn
    );
    assert_eq!(
        fixture
            .mutate(
                "host-fixture",
                &fixture.foreign,
                "POST",
                "make-orchestrator",
                json!({})
            )
            .await,
        StatusCode::OK
    );
    assert!(fixture
        .state
        .svc
        .mux
        .read()
        .get_tab(&fixture.foreign)
        .unwrap()
        .anchors_team());
}

#[tokio::test]
async fn tab_mutation_routes_refuse_foreign_destinations_and_new_team_promotions() {
    let fixture = Fixture::new();
    let token = fixture.token(&fixture.owner);
    let before = fixture.identity(&fixture.member);
    for (suffix, body) in [
        (
            "role",
            json!({"role":"Worker","parent_tab_id":fixture.foreign}),
        ),
        (
            "role",
            json!({"role":"Planner","plan_id":fixture.foreign_plan}),
        ),
        ("role", json!({"role":"Orchestrator"})),
        ("make-planner", json!({"plan_id":fixture.foreign_plan})),
        ("make-orchestrator", json!({})),
    ] {
        assert_eq!(
            fixture
                .mutate(&token, &fixture.member, "POST", suffix, body)
                .await,
            StatusCode::FORBIDDEN,
            "{suffix}"
        );
        assert_eq!(fixture.identity(&fixture.member), before);
        assert!(fixture
            .state
            .svc
            .plan_store
            .get(&fixture.foreign_plan)
            .unwrap()
            .planner
            .is_none());
    }
}

#[tokio::test]
async fn standalone_self_promotion_matches_host_make_orchestrator() {
    for host in [false, true] {
        let fixture = Fixture::new();
        let target = fixture.standalone();
        let token = if host {
            "host-fixture".into()
        } else {
            fixture.token(&target)
        };
        assert_eq!(
            fixture
                .mutate(&token, &target, "POST", "make-orchestrator", json!({}))
                .await,
            StatusCode::OK
        );
        let mux = fixture.state.svc.mux.read();
        let tab = mux.get_tab(&target).unwrap();
        assert_eq!(tab.role, Some(AgentRole::Manager));
        assert_eq!(tab.role_profile.as_deref(), Some("orchestrator"));
        assert!(tab.can_spawn);
        assert!(tab.parent_tab_id.is_none());
        assert!(mux.get_team(tab.team_id.as_deref().unwrap()).is_some());
        assert!(tab.pending_greeting.is_some());
    }
}

#[tokio::test]
async fn standalone_self_promotion_refuses_other_mutations_and_ineligible_tabs() {
    let fixture = Fixture::new();
    let standalone = fixture.standalone();
    let token = fixture.token(&standalone);
    let before = fixture.identity(&standalone);
    for (method, suffix, body) in [
        ("POST", "role", json!({"role":"Orchestrator"})),
        ("DELETE", "role", json!({})),
        ("POST", "can-spawn", json!({"can_spawn":true})),
        ("POST", "make-planner", json!({"plan_id":fixture.plan})),
    ] {
        assert_eq!(
            fixture
                .mutate(&token, &standalone, method, suffix, body)
                .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(fixture.identity(&standalone), before);
    }
    let other = fixture.standalone();
    let before = fixture.identity(&other);
    assert_eq!(
        fixture
            .mutate(&token, &other, "POST", "make-orchestrator", json!({}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fixture.identity(&other), before);
    for target in [&fixture.worker, &fixture.member] {
        let before = fixture.identity(target);
        assert_eq!(
            fixture
                .mutate(
                    &fixture.token(target),
                    target,
                    "POST",
                    "make-orchestrator",
                    json!({})
                )
                .await,
            StatusCode::FORBIDDEN
        );
        assert_eq!(fixture.identity(target), before);
    }
    for case in [
        "team",
        "concierge",
        "neighbor",
        "remote",
        "mesh",
        "shell",
        "unverified",
        "exited",
        "protected",
        "profile",
    ] {
        let target = fixture.standalone();
        let token = fixture.token(&target);
        {
            let mut mux = fixture.state.svc.mux.write();
            let team = mux.get_tab(&fixture.owner).unwrap().team_id.clone();
            let tab = mux.get_tab_mut(&target).unwrap();
            match case {
                "team" => tab.team_id = team,
                "concierge" | "neighbor" => {
                    tab.neighbor_authority = Some(
                        codeterm_service_core::multiplexer::types::NeighborAuthority {
                            uid: "friend".into(),
                            friendship_id: "friendship".into(),
                            root_tab_id: (case == "neighbor").then(|| "concierge".into()),
                        },
                    )
                }
                "remote" => {
                    tab.ownership = Some(codeterm_types::pty::RemoteOwnership {
                        controller_title: "controller".into(),
                        controller_tab_id: None,
                        peer_id: "peer".into(),
                        peer_name: "peer".into(),
                        team: None,
                    })
                }
                "mesh" => tab.report_target_mesh_peer = Some("peer".into()),
                "shell" => tab.agent_type = None,
                "unverified" => tab.reported_session_binding_nonce = None,
                "exited" => tab.exit_suspected_at = Some(std::time::Instant::now()),
                "protected" => tab.protected = true,
                "profile" => tab.role_profile = Some("planner".into()),
                _ => unreachable!(),
            }
        }
        let before = fixture.identity(&target);
        assert_eq!(
            fixture
                .mutate(&token, &target, "POST", "make-orchestrator", json!({}))
                .await,
            StatusCode::FORBIDDEN,
            "{case}"
        );
        assert_eq!(fixture.identity(&target), before, "{case}");
    }
}

#[tokio::test]
async fn standalone_self_promotion_refuses_peer_and_missing_auth() {
    use codeterm_service_core::mesh::types::{AccessLevel, PairedPeer, RELAY_PROTOCOL_VERSION};
    let fixture = Fixture::new();
    let target = fixture.standalone();
    let (private_key, public_key) = codeterm_service_core::mesh::crown::generate_peer_keypair();
    {
        let mut mesh = fixture.state.mesh.lock();
        mesh.insert_account_peer_for_test(PairedPeer {
            instance_id: "paired".into(),
            instance_name: "paired".into(),
            addresses: vec![],
            api_port: 7685,
            public_key,
            paired_at: "now".into(),
            last_seen: None,
            online: true,
            is_primary: false,
            access_level: AccessLevel::Full,
            version: None,
            web_ui: None,
            relay_protocol_version: RELAY_PROTOCOL_VERSION,
            consecutive_misses: 0,
            last_active_ms: None,
        });
    }
    fixture.state.config.lock().mesh.enabled = true;
    let peer_token =
        codeterm_service_core::mesh::crown::sign_request(&private_key, "paired").unwrap();

    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        "authorization",
        format!("Bearer {peer_token}").parse().unwrap(),
    );
    assert!(matches!(
        crate::auth::caller_access_level(&fixture.state, &headers),
        Ok(AccessLevel::Full)
    ));
    let before = fixture.identity(&target);
    assert_eq!(
        fixture
            .mutate(&peer_token, &target, "POST", "make-orchestrator", json!({}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fixture.identity(&target), before);
    let request = Request::builder()
        .method("POST")
        .uri(format!("/api/tabs/{target}/make-orchestrator"))
        .header("content-type", "application/json")
        .body(Body::from("{}"))
        .unwrap();
    let response = crate::routes::create_router(fixture.state.clone())
        .oneshot(request)
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(fixture.identity(&target), before);
}

#[tokio::test]
async fn standalone_self_promotion_refuses_service_push() {
    let fixture = Fixture::new();
    let target = fixture.standalone();
    let service = fixture.state.svc.services_store.create(serde_json::from_value(json!({
        "name": "authority-fixture", "kind": "daemon", "health_mode": "http", "push_enabled": true
    })).unwrap()).unwrap();
    let token = fixture.state.svc.tokens.generate(&service).unwrap();
    assert_eq!(
        codeterm_service_core::service::push::validate_push(
            &format!("Bearer {token}"),
            &fixture.state.svc.tokens,
            &fixture.state.svc.services_store
        )
        .unwrap(),
        Some(service)
    );
    let before = fixture.identity(&target);
    assert_eq!(
        fixture
            .mutate(&token, &target, "POST", "make-orchestrator", json!({}))
            .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(fixture.identity(&target), before);
}

#[test]
fn standalone_self_promotion_rechecks_prepared_caller_at_commit() {
    use codeterm_service_core::service::tab_mutation_authority::{
        TabMutationCaller, TabMutationScope,
    };
    for case in ["new-session", "worker-team", "team", "neighbor"] {
        let fixture = Fixture::new();
        let target = fixture.standalone();
        let token = fixture.token(&target);
        let caller = fixture.state.svc.identify_tab_credential(&token).unwrap();
        fixture
            .state
            .svc
            .authorize_tab_mutation(
                TabMutationCaller::Tab(&caller),
                &target,
                TabMutationScope {
                    creates_team: true,
                    self_promotion: true,
                    ..Default::default()
                },
            )
            .unwrap();
        {
            let mut mux = fixture.state.svc.mux.write();
            if case == "worker-team" {
                mux.set_tab_role(&target, AgentRole::Worker, Some(&fixture.owner))
                    .unwrap();
            } else {
                let team = mux.get_tab(&fixture.owner).unwrap().team_id.clone();
                let tab = mux.get_tab_mut(&target).unwrap();
                match case {
                    "new-session" => {
                        tab.agent_session_id = Some("replacement-session".into());
                        tab.session_binding_nonce = Some("replacement-nonce".into());
                        tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
                    }
                    "team" => tab.team_id = team,
                    "neighbor" => {
                        tab.neighbor_authority = Some(
                            codeterm_service_core::multiplexer::types::NeighborAuthority {
                                uid: "friend".into(),
                                friendship_id: "friendship".into(),
                                root_tab_id: None,
                            },
                        )
                    }
                    _ => unreachable!(),
                }
            }
        }
        let (before, teams) = {
            let mux = fixture.state.svc.mux.read();
            (
                serde_json::to_value(mux.get_tab(&target).unwrap()).unwrap(),
                mux.team_count(),
            )
        };
        assert!(
            codeterm_orchestration::make_orchestrator_as_tab(&fixture.state.svc, &target, &caller,)
                .is_err(),
            "{case}"
        );
        let mux = fixture.state.svc.mux.read();
        let tab = mux.get_tab(&target).unwrap();
        assert_eq!(serde_json::to_value(tab).unwrap(), before, "{case}");
        assert_eq!(mux.team_count(), teams, "{case}");
        assert!(!tab.can_spawn, "{case}");
        assert!(tab.pending_greeting.is_none(), "{case}");
    }
}
