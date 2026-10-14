use axum::http::StatusCode;

use super::map_close_error;

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

#[test]
fn partial_team_teardown_workspace_close_failure_is_internal_error() {
    let error = format!(
        "{}: team lane cleanup failed",
        crate::service::WORKSPACE_CLOSE_CLEANUP_FAILURE_PREFIX
    );
    let (status, message) = map_close_error(error);

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(message.contains("team lane cleanup failed"));
}

#[test]
fn team_close_authorization_refusal_is_forbidden() {
    let error = format!(
        "{}: agent is not a Team manager",
        crate::service::TEAM_CLOSE_REFUSAL_PREFIX
    );
    let (status, message) = map_close_error(error);

    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(message.contains("not a Team manager"));
}

#[test]
fn busy_worker_close_refusal_is_conflict() {
    let error = format!(
        "{}: Tab worker-tab holds work",
        crate::service::worker_loss::BUSY_WORKER_CLOSE_REFUSAL_PREFIX
    );
    let (status, message) = map_close_error(error);

    assert_eq!(status, StatusCode::CONFLICT);
    assert!(message.contains("worker-tab"));
}
