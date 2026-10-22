use super::{workspace_close_caller, CloseWorkspaceBody, WorkspaceCloseCaller};
use axum::http::HeaderMap;

#[test]
fn workspace_close_route_reads_force_from_body() {
    let no_headers = HeaderMap::new();
    assert_eq!(
        workspace_close_caller(&no_headers, &CloseWorkspaceBody::default(), "workspace-1"),
        WorkspaceCloseCaller::Unattributed,
    );
    assert_eq!(
        workspace_close_caller(
            &no_headers,
            &CloseWorkspaceBody {
                force: false,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::Unattributed,
    );
    assert_eq!(
        workspace_close_caller(
            &no_headers,
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::HumanConfirmedHeaderless,
    );
    assert_eq!(
        workspace_close_caller(
            &no_headers,
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-other".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::Unattributed,
    );
    let mut agent_headers = HeaderMap::new();
    agent_headers.insert("X-CodeTerm-Tab-Id", "agent-1".parse().unwrap());
    assert_eq!(
        workspace_close_caller(
            &agent_headers,
            &CloseWorkspaceBody {
                force: true,
                confirm: Some("workspace-1".into()),
            },
            "workspace-1",
        ),
        WorkspaceCloseCaller::AgentTab("agent-1".into()),
    );
}
