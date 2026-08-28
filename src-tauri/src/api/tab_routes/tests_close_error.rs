use axum::http::StatusCode;

use super::super::map_close_error;

#[test]
fn optional_body_contract_workspace_agent_tab_close_refusal_is_forbidden() {
    let workspace_id = "workspace-with-agent-tabs";
    let error = format!(
        "{}: {workspace_id} contains agent tabs",
        crate::service::AGENT_TAB_CLOSE_REFUSAL_PREFIX
    );
    let (status, message) = map_close_error(error);

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message.contains(workspace_id));
}

#[test]
fn optional_body_contract_missing_workspace_close_remains_not_found() {
    let workspace_id = "missing-workspace";
    let (status, message) = map_close_error(format!("Workspace not found: {workspace_id}"));

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(message.contains(workspace_id));
}
