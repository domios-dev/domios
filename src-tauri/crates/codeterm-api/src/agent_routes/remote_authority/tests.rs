use super::*;
use serde_json::json;

fn agent() -> AuthenticatedAgent {
    AuthenticatedAgent {
        instance_id: "origin-machine".into(),
        tab_id: "origin-tab".into(),
        can_spawn: true,
    }
}

#[test]
fn remote_agent_spawn_retains_agent_ownership_without_local_hierarchy_authority() {
    let mut body: SpawnAgentBody =
        serde_json::from_value(json!({"agent":"opaque-provider", "task":"opaque-task"})).unwrap();
    prepare(&agent(), &mut body).unwrap_or_else(|error| panic!("{}", error.body));
    let role = super::super::validate_spawn_role(body.role.as_deref()).unwrap();
    let ownership = body.ownership.clone();
    let params = super::super::spawn_request::into_params(body, role, ownership);
    assert_eq!(params.role.as_deref(), Some("Worker"));
    assert_eq!(params.can_spawn, Some(false));
    assert_eq!(params.report_target_tab_id.as_deref(), Some("origin-tab"));
    assert_eq!(
        params.report_target_mesh_peer.as_deref(),
        Some("origin-machine")
    );
    assert_eq!(
        params.ownership.unwrap().controller_tab_id.as_deref(),
        Some("origin-tab")
    );
    assert!(params.spawned_by_tab_id.is_none());
    assert!(params.parent_tab_id.is_none());
    assert!(params.team_id.is_none());
    assert!(!params.user_created);
}

#[test]
fn remote_spawn_cannot_promote_or_claim_local_plan_and_team_authority() {
    for payload in [
        json!({"role":"orchestrator"}),
        json!({"role":"planner"}),
        json!({"team_id":"local-team"}),
        json!({"plan_id":"local-plan"}),
        json!({"plan_task":"local-plan/task"}),
        json!({"authorized_gates":["owner-gate"]}),
    ] {
        let mut body = serde_json::from_value(payload).unwrap();
        assert!(prepare(&agent(), &mut body).is_err());
    }
    let mut actor = agent();
    actor.can_spawn = false;
    let mut body = serde_json::from_value(json!({})).unwrap();
    assert!(prepare(&actor, &mut body).is_err());
}
