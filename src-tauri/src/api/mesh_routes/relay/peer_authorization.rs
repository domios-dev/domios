use crate::api::access_policy::{peer_access_allowed, request_path};
use crate::api::routes::AppState;
use axum::http::{Method, StatusCode};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PeerRelayAuthorization<'a> {
    Rejected,
    WorkspaceClose {
        peer_id: &'a str,
        workspace_id: &'a str,
        force: bool,
    },
    MeshRpc {
        peer_id: &'a str,
    },
    Forward,
}

pub(super) fn authorize_peer_relay<'a>(
    peer_id: &'a str,
    access: &crate::mesh::types::AccessLevel,
    method: &Method,
    path: &'a str,
    body: &serde_json::Value,
    tab_owner: impl FnOnce(&str) -> Option<String>,
) -> PeerRelayAuthorization<'a> {
    if !peer_access_allowed(access, peer_id, method, path, tab_owner) {
        return PeerRelayAuthorization::Rejected;
    }
    let path = request_path(path);
    if *method == Method::POST && path == "/api/mesh/rpc" {
        return PeerRelayAuthorization::MeshRpc { peer_id };
    }
    match workspace_close_target(method, path) {
        Some(workspace_id) => PeerRelayAuthorization::WorkspaceClose {
            peer_id,
            workspace_id,
            force: relay_workspace_close_force(body),
        },
        None => PeerRelayAuthorization::Forward,
    }
}

pub(super) async fn mesh_rpc_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    body: &serde_json::Value,
) -> Result<crate::mesh::types::RelayResponse, (StatusCode, String)> {
    let request: super::super::rpc::MeshRpcRequest =
        serde_json::from_value(body.clone()).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid mesh RPC body: {error}"),
            )
        })?;
    let peer_id = peer_id.to_string();
    let response = super::super::rpc::handle_mesh_rpc(state, request, move || Ok(peer_id)).await;
    Ok(crate::mesh::types::RelayResponse {
        ok: true,
        status: StatusCode::OK.as_u16(),
        data: Some(serde_json::to_value(response).unwrap_or(serde_json::Value::Null)),
        error: None,
    })
}

fn relay_workspace_close_force(body: &serde_json::Value) -> bool {
    body.get("force")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}

fn workspace_close_target<'a>(method: &Method, path: &'a str) -> Option<&'a str> {
    if *method != Method::POST {
        return None;
    }
    let workspace_id = path
        .strip_prefix("/api/workspaces/")?
        .strip_suffix("/close")?;
    (!workspace_id.is_empty() && !workspace_id.contains('/')).then_some(workspace_id)
}

pub(super) async fn close_workspace_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    workspace_id: &str,
    force: bool,
) -> Result<crate::mesh::types::RelayResponse, (StatusCode, String)> {
    let service = state.svc.clone();
    let peer_id = peer_id.to_string();
    let workspace_id = workspace_id.to_string();
    let result = tokio::task::spawn_blocking(move || {
        service.close_workspace_from_authenticated_mesh_peer(&peer_id, &workspace_id, force)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Workspace close task failed: {error}"),
        )
    })?;
    match result {
        Ok(()) => Ok(crate::mesh::types::RelayResponse {
            ok: true,
            status: StatusCode::OK.as_u16(),
            data: Some(serde_json::json!({ "ok": true })),
            error: None,
        }),
        Err(error) => {
            let (status, message) = crate::api::tab_routes::map_close_error(error);
            Ok(crate::mesh::types::RelayResponse {
                ok: false,
                status: status.as_u16(),
                data: None,
                error: Some(message),
            })
        }
    }
}
