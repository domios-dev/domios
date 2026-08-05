use serde_json::json;

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::test_fixture::ServiceFixture;

use super::authorized_orchestrator_actor;

const NONCE: &str = "owner-nonce";
const LIVE_SESSION: &str = "live-session";

/// `stored_owner_session` is what the plan recorded when it was created; the tab's
/// live session is always LIVE_SESSION, so `None`/rotated values model a restart.
fn fixture(stored_owner_session: Option<&str>, team_bound: bool) -> (ServiceFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some(LIVE_SESSION.into());
        tab.session_binding_nonce = Some(NONCE.into());
        tab.reported_session_binding_nonce = Some(NONCE.into());
        tab.team_id = team_bound.then(|| team_id.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "review authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: stored_owner_session.map(str::to_string),
            title: None,
        },
    );
    plan.team_id = team_bound.then_some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    (fixture, plan.id, owner)
}

fn caller(tab_id: &str, session_id: &str, nonce: &str) -> serde_json::Value {
    json!({
        "caller_tab_id": tab_id,
        "session_id": session_id,
        "session_binding_nonce": nonce,
    })
}

/// The reproduction: owner tab and nonce match, the tab is a live Team-bound
/// Orchestrator, but the plan's STORED owner session is stale or absent.
#[test]
fn live_orchestrator_with_rotated_owner_session_is_authorized() {
    for stored in [None, Some("rotated-away")] {
        let (fixture, plan_id, owner) = fixture(stored, true);
        let params = caller(&owner, LIVE_SESSION, NONCE);

        let actor = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
            .expect("a live bound Plan Orchestrator must keep review authority");

        assert_eq!(actor.tab_id.as_deref(), Some(owner.as_str()));
        assert_eq!(actor.session_id.as_deref(), Some(LIVE_SESSION));
    }
}

#[test]
fn a_matching_stored_owner_session_is_still_authorized() {
    let (fixture, plan_id, owner) = fixture(Some(LIVE_SESSION), true);
    let params = caller(&owner, LIVE_SESSION, NONCE);

    assert!(authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params)).is_ok());
}

/// An omitted session id is resolved from the live tab, never guessed.
#[test]
fn an_omitted_session_id_resolves_from_the_live_tab() {
    let (fixture, plan_id, owner) = fixture(None, true);
    let params = json!({
        "caller_tab_id": owner,
        "session_binding_nonce": NONCE,
    });

    let actor = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params)).unwrap();

    assert_eq!(actor.session_id.as_deref(), Some(LIVE_SESSION));
}

#[test]
fn wrong_tab_wrong_nonce_and_wrong_live_session_are_all_refused() {
    let (fixture, plan_id, owner) = fixture(Some(LIVE_SESSION), true);
    for params in [
        caller("not-the-owner", LIVE_SESSION, NONCE),
        caller(&owner, LIVE_SESSION, "forged-nonce"),
        caller(&owner, "not-the-live-session", NONCE),
        caller("", LIVE_SESSION, NONCE),
        caller(&owner, LIVE_SESSION, ""),
    ] {
        let error = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
            .expect_err("only the live bound Plan Orchestrator may act");
        assert_eq!(error.code, -32003);
    }
}

#[test]
fn a_non_team_orchestrator_is_refused() {
    let (fixture, plan_id, owner) = fixture(None, false);
    let params = caller(&owner, LIVE_SESSION, NONCE);

    let error = authorized_orchestrator_actor(&fixture.service, &plan_id, Some(&params))
        .expect_err("a plan is Team-bound; a team-less Orchestrator has no authority");

    assert_eq!(error.code, -32003);
}

#[test]
fn a_worker_and_a_draft_planner_are_refused() {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    let planner = mux.create_tab(&workspace).unwrap();
    let worker = mux.create_tab(&workspace).unwrap();
    for (tab_id, session) in [
        (&owner, LIVE_SESSION),
        (&planner, "planner-session"),
        (&worker, "worker-session"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(NONCE.into());
        tab.reported_session_binding_nonce = Some(NONCE.into());
        tab.team_id = Some(team_id.clone());
    }
    {
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.parent_tab_id = Some(owner.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "review authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: None,
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();

    for (tab_id, session) in [(&planner, "planner-session"), (&worker, "worker-session")] {
        let params = caller(tab_id, session, NONCE);
        let error = authorized_orchestrator_actor(&fixture.service, &plan.id, Some(&params))
            .expect_err("only the Plan Orchestrator grants review capacity");
        assert_eq!(error.code, -32003);
    }
    let params = caller(&owner, LIVE_SESSION, NONCE);
    assert!(authorized_orchestrator_actor(&fixture.service, &plan.id, Some(&params)).is_ok());
}
