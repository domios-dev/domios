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
