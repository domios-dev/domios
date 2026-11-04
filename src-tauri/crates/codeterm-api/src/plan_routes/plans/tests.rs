use super::*;

#[test]
fn transfer_caller_requires_agent_header() {
    let mut headers = HeaderMap::new();
    headers.insert("X-CodeTerm-Tab-Id", "source-manager".parse().unwrap());
    assert_eq!(
        transfer_caller(&crate::auth::ActorHeaders::for_owner_test(headers.clone())).unwrap(),
        "source-manager"
    );
    let missing =
        transfer_caller(&crate::auth::ActorHeaders::for_owner_test(HeaderMap::new())).unwrap_err();
    assert_eq!(missing.0, StatusCode::FORBIDDEN);
    assert_eq!(
        team_plan_err(TeamPlanError::Team(
            "Caller tab worker is not authorized to manage Team team_source".into(),
        ))
        .0,
        StatusCode::FORBIDDEN
    );
}

#[test]
fn reference_labels_preserve_archived_unowned_stale_missing_and_permission_states() {
    let projection = |status: &str, owner: serde_json::Value| {
        Ok(serde_json::from_value(serde_json::json!({
            "title":"Selected plan", "status":status, "owner":owner,
            "task_refs":{"task_a":"task_a"},
            "canonical_project_path":"/project", "tasks":[{"id":"task_a", "name":"Selected task", "type":"implement", "status":"pending", "key":"COD-1"}],
            "sections":[{"id":"sec_a", "title":"Selected section"}]
        })).unwrap())
    };
    let request = |kind, id: Option<&str>| ResolveRefRequest {
        plan_id: "pl_a".into(),
        target_kind: kind,
        target_id: id.map(str::to_string),
    };
    let archived = projection("archived", serde_json::json!({}));
    let task = resolve_ref(
        Some(&archived),
        request(ResolveTargetKind::Task, Some("task_a")),
    );
    assert_eq!(task.state, "archived");
    assert_eq!(task.task_key.as_deref(), Some("COD-1"));
    assert_eq!(task.canonical_project_path.as_deref(), Some("/project"));
    let unowned = projection("active", serde_json::json!({}));
    assert_eq!(
        resolve_ref(
            Some(&unowned),
            request(ResolveTargetKind::Section, Some("sec_a"))
        )
        .state,
        "unowned"
    );
    assert_eq!(
        resolve_ref(
            Some(&unowned),
            request(ResolveTargetKind::Task, Some("task_missing"))
        )
        .state,
        "stale"
    );
    assert_eq!(
        resolve_ref(
            Some(&Err(PlanError::NotFound)),
            request(ResolveTargetKind::Plan, None)
        )
        .state,
        "missing"
    );
    assert_eq!(
        resolve_ref(
            Some(&Err(PlanError::Io("permission denied".into()))),
            request(ResolveTargetKind::Plan, None)
        )
        .state,
        "permission"
    );
    assert_eq!(
        resolve_ref(
            Some(&Err(PlanError::Io("unreachable".into()))),
            request(ResolveTargetKind::Plan, None)
        )
        .state,
        "unreachable"
    );
}
