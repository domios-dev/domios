use super::*;
use crate::server::plan_fields::optional_string_field;

#[test]
fn lane_cleanup_dispatch_uses_owned_utility_binding_and_exact_plan_scope() {
    let (fixture, plan_id, _, owner) = codeterm_service_core::service::plan_authority_fixture();
    let svc = std::sync::Arc::new(fixture.service.clone());
    let utility_id = {
        let mut mux = fixture.service.mux.write();
        let owner_id = owner.tab_id.as_deref().unwrap();
        let workspace = mux.find_workspace_for_tab(owner_id).unwrap();
        let team = mux.get_tab(owner_id).unwrap().team_id.clone();
        let id = mux.create_tab(&workspace).unwrap();
        let utility = mux.get_tab_mut(&id).unwrap();
        utility.team_id = team;
        utility.parent_tab_id = Some(owner_id.into());
        utility.close_restore = Some(
            codeterm_service_core::multiplexer::types::CloseRestoreTabState {
                utility_owner_tab_id: Some(owner_id.into()),
                ..Default::default()
            },
        );
        utility.session_binding_nonce = Some("utility-nonce".into());
        utility.pty_id = Some("utility-pty".into());
        id
    };
    let params = json!({
        "plan_id": plan_id, "confirm": true,
        "caller_tab_id": utility_id, "session_binding_nonce": "utility-nonce"
    });
    for confirm in [false, true] {
        let mut request = params.clone();
        request["confirm"] = json!(confirm);
        let response = dispatch("plan.lanes_clean", &Some(request), &svc).unwrap();
        assert_eq!(
            response,
            json!({"plan_id": plan_id, "dry_run": !confirm, "lanes": []})
        );
    }
    let mut foreign = params.clone();
    foreign["session_binding_nonce"] = json!("foreign");
    foreign["parent_tab_id"] = json!(owner.tab_id);
    assert_eq!(
        dispatch("plan.lanes_clean", &Some(foreign), &svc)
            .unwrap_err()
            .code,
        -32003
    );
    let mut fetch = params.clone();
    fetch["confirm"] = json!(false);
    fetch["fetch"] = json!(true);
    fetch["session_binding_nonce"] = json!("foreign");
    assert_eq!(
        dispatch("plan.lanes_clean", &Some(fetch), &svc)
            .unwrap_err()
            .code,
        -32003
    );
    let mut plan = fixture.service.plan_store.get(&plan_id).unwrap();
    plan.team_id = Some("foreign-Team".into());
    fixture.service.plan_store.save(&mut plan, 2).unwrap();
    assert_eq!(
        dispatch("plan.lanes_clean", &Some(params), &svc)
            .unwrap_err()
            .code,
        -32003
    );
}

#[test]
fn optional_string_field_preserves_missing_vs_null() {
    let params = json!({
        "model": "opus",
        "why": null
    });

    assert_eq!(optional_string_field(Some(&params), "missing"), None);
    assert_eq!(optional_string_field(Some(&params), "why"), Some(None));
    assert_eq!(
        optional_string_field(Some(&params), "model"),
        Some(Some("opus"))
    );
}

#[test]
fn archived_plan_next_response_never_advises_dispatch() {
    let response = archived_next_response("pl_archived", PlanStatus::Archived).unwrap();
    assert_eq!(response["plan_id"], "pl_archived");
    assert_eq!(response["next_actions"], json!([]));
    assert!(archived_next_response("pl_active", PlanStatus::Active).is_none());
}

#[test]
fn transfer_caller_requires_agent_param() {
    assert_eq!(
        transfer_caller(&Some(json!({ "caller_tab_id": "source-manager" }))).unwrap(),
        "source-manager"
    );
    let missing = transfer_caller(&None).unwrap_err();
    assert_eq!(missing.code, -32602);
    assert!(missing.message.contains("caller_tab_id is required"));
}

#[test]
fn plan_create_uses_team_workspace_project_path() {
    let (mut mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("Team project".into(), Some("C:/team/workspace".into()));
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    mux.get_tab_mut(&tab_id).unwrap().team_id = Some("team_1".into());

    let params = json!({ "caller_tab_id": tab_id.clone() });
    assert_eq!(plan_project_path(&params, &mux), "C:/team/workspace");
    let hinted = json!({ "caller_tab_id": tab_id.clone(), "caller_cwd": "C:/elsewhere" });
    assert_eq!(
        plan_project_path(&hinted, &mux),
        "C:/team/workspace",
        "a Team caller's own cwd never outranks its Team workspace"
    );
    assert_eq!(
        plan_project_path(&json!({ "caller_cwd": "C:/elsewhere" }), &mux),
        "C:/elsewhere",
        "a caller outside any Team keeps its cwd"
    );
    assert_eq!(
        plan_project_path(
            &json!({ "caller_tab_id": tab_id, "project_path": "C:/override" }),
            &mux
        ),
        "C:/override"
    );
}
