use super::*;
fn binding() -> GoalBinding {
    GoalBinding {
        thread_id: "own".into(),
        binding_nonce: "nonce".into(),
        epoch: 3,
    }
}
fn intent(status: &str) -> ChatSessionIntent {
    ChatSessionIntent {
        title: None,
        goal: Some("fixture".into()),
        goal_status: Some(status.into()),
    }
}
#[test]
fn confirmation_requires_capability_and_exact_binding() {
    let cap = RuntimeGoalCapability {
        actions: vec![RuntimeGoalAction::Cancel],
    };
    let mut request = GoalRequest {
        action: RuntimeGoalAction::Cancel,
        confirmed: false,
        binding: Some(binding()),
        expected_intent: Some(intent("active")),
    };
    assert!(authorize(&request, &cap, &binding()).is_err());
    request.confirmed = true;
    assert!(authorize(&request, &cap, &binding()).is_ok());
    let mut stale = binding();
    stale.epoch += 1;
    assert!(authorize(&request, &cap, &stale).is_err());
    stale = binding();
    stale.thread_id = "foreign".into();
    assert!(authorize(&request, &cap, &stale).is_err());
    stale = binding();
    stale.binding_nonce = "new".into();
    assert!(authorize(&request, &cap, &stale).is_err());
    request.action = RuntimeGoalAction::Pause;
    assert!(authorize(&request, &cap, &binding()).is_err());
}
#[test]
fn only_confirmed_far_end_goal_results_succeed() {
    assert!(result_intent(RuntimeResult::Done, &binding()).is_err());
    assert!(result_intent(
        RuntimeResult::Goal {
            thread_id: "foreign".into(),
            intent: None
        },
        &binding()
    )
    .is_err());
    assert!(validate_outcome(RuntimeGoalAction::Cancel, &Some(intent("active"))).is_err());
    assert!(validate_outcome(RuntimeGoalAction::Cancel, &None).is_ok());
    for (action, status) in [
        (RuntimeGoalAction::Pause, "paused"),
        (RuntimeGoalAction::Resume, "active"),
        (RuntimeGoalAction::Complete, "complete"),
    ] {
        assert!(validate_outcome(action, &Some(intent(status))).is_ok());
        assert!(validate_outcome(action, &None).is_err());
    }
}

#[test]
fn goal_route_retains_runtime_control_authority() {
    use crate::api_support::access_policy::peer_access_allowed;
    use crate::mesh::types::AccessLevel;
    use axum::http::Method;
    let path = "/api/tabs/owned/agent/runtime/goal";
    assert!(peer_access_allowed(
        &AccessLevel::Full,
        "peer-a",
        &Method::POST,
        path,
        |_| None
    ));
    assert!(!peer_access_allowed(
        &AccessLevel::ReadOnly,
        "peer-a",
        &Method::POST,
        path,
        |_| None
    ));
    assert!(!peer_access_allowed(
        &AccessLevel::AgentIsolated,
        "peer-a",
        &Method::POST,
        path,
        |_| Some("peer-b".into())
    ));
}
