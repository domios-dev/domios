use codeterm_service_core::service::workspace_agent_fixture as workspace_with_agent_tab;

#[test]
fn optional_body_contract_workspace_agent_guard_is_reported_as_forbidden() {
    let (fixture, workspace_id, _, caller_tab) = workspace_with_agent_tab();
    let error = fixture
        .service
        .close_workspace_guarded(Some(&caller_tab), &workspace_id, false)
        .unwrap_err();
    let (status, message) = crate::tab_routes::map_close_error(error);

    assert_eq!(status, axum::http::StatusCode::FORBIDDEN);
    assert!(message.contains("Release guard agent"));
}
