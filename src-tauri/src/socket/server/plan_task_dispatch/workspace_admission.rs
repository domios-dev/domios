use serde_json::json;

use crate::service::ServiceState;

use super::{dispatch_failed, JsonRpcError};

pub(super) const DISPATCH_WORKSPACE_CLOSING_CODE: i32 = -32014;

fn workspace_closing_dispatch_error(workspace_id: &str) -> JsonRpcError {
    JsonRpcError {
        code: DISPATCH_WORKSPACE_CLOSING_CODE,
        message: format!("dispatch refused: workspace '{workspace_id}' is closing"),
        data: Some(json!({
            "type": "dispatch_workspace_closing",
            "workspace_id": workspace_id,
            "reason": "workspace_is_closing",
        })),
    }
}

pub(super) fn admit_dispatch_workspace(
    workspace_id: &str,
    ensure_mutable: impl FnOnce(&str) -> Result<(), String>,
) -> Result<(), JsonRpcError> {
    ensure_mutable(workspace_id).map_err(|reason| {
        if reason.starts_with("Workspace is closing:") {
            workspace_closing_dispatch_error(workspace_id)
        } else {
            dispatch_failed("workspace_preflight", reason)
        }
    })
}

pub(super) fn preflight_dispatch_workspace(
    svc: &ServiceState,
    requested_workspace_id: Option<String>,
    parent_tab_id: Option<&str>,
    caller_tab_id: Option<&str>,
) -> Result<(), JsonRpcError> {
    let workspace_id = {
        let mux = svc.mux.read();
        crate::service::spawn::resolve_spawn_workspace_id(
            requested_workspace_id,
            parent_tab_id,
            caller_tab_id,
            |tab_id| mux.find_workspace_for_tab(tab_id),
            mux.get_active_workspace()
                .map(|workspace| workspace.id.clone()),
            mux.list_workspaces()
                .first()
                .map(|workspace| workspace.id.clone()),
        )
        .ok()
    };
    let Some(workspace_id) = workspace_id else {
        return Ok(());
    };
    let mux = svc.mux.read();
    admit_dispatch_workspace(&workspace_id, |id| {
        mux.ensure_workspace_membership_mutable(id)
    })
}
