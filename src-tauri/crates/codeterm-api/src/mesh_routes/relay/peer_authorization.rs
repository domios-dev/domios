use crate::routes::AppState;
use axum::http::{Method, StatusCode};
use codeterm_service_core::api_support::access_policy::{
    allows_full_dispatch, is_local_only_path, peer_access_allowed, request_path, request_scope,
};
use codeterm_service_core::mesh::types::AccessLevel;
use codeterm_service_core::service::close_authority::{MeshCloseGrant, OwnerTabCloseScope};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PeerRelayAuthorization<'a> {
    Rejected,
    WorkspaceClose {
        peer_id: &'a str,
        workspace_id: &'a str,
        force: bool,
    },
    WorkspaceClosePreview {
        peer_id: &'a str,
        workspace_id: &'a str,
    },
    MeshRpc {
        peer_id: &'a str,
    },
    TabClose {
        peer_id: &'a str,
        tab_id: &'a str,
    },
    OwnerTabClose {
        tab_id: &'a str,
    },
    Forward,
}

pub(super) fn authorize_peer_relay<'a>(
    peer_id: &'a str,
    access: &AccessLevel,
    method: &Method,
    path: &'a str,
    body: &serde_json::Value,
    tab_owner: impl FnOnce(&str) -> Option<String>,
) -> PeerRelayAuthorization<'a> {
    let forward_to_routed_policy = allows_full_dispatch(access)
        && !is_local_only_path(path)
        && request_scope(method, path).is_none();
    if !forward_to_routed_policy && !peer_access_allowed(access, peer_id, method, path, tab_owner) {
        return PeerRelayAuthorization::Rejected;
    }
    let path = request_path(path);
    if *method == Method::POST && path == "/api/mesh/rpc" {
        return PeerRelayAuthorization::MeshRpc { peer_id };
    }
    if *method == Method::POST {
        if let Some(tab_id) = path
            .strip_prefix("/api/tabs/")
            .and_then(|path| path.strip_suffix("/close"))
        {
            if !tab_id.is_empty() && !tab_id.contains('/') {
                if body.get("as_owner").and_then(serde_json::Value::as_bool) == Some(true) {
                    return if matches!(access, AccessLevel::Full) {
                        PeerRelayAuthorization::OwnerTabClose { tab_id }
                    } else {
                        PeerRelayAuthorization::Rejected
                    };
                }
                return PeerRelayAuthorization::TabClose { peer_id, tab_id };
            }
        }
    }
    if let Some(workspace_id) = workspace_close_preview_target(method, path) {
        return PeerRelayAuthorization::WorkspaceClosePreview {
            peer_id,
            workspace_id,
        };
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
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let request: super::super::rpc::MeshRpcRequest =
        serde_json::from_value(body.clone()).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                format!("Invalid mesh RPC body: {error}"),
            )
        })?;
    let peer_id = peer_id.to_string();
    let response =
        super::super::rpc::handle_mesh_rpc(state, request, origin, agent, move || Ok(peer_id))
            .await;
    Ok(codeterm_service_core::mesh::types::RelayResponse {
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

pub(super) async fn close_tab_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    tab_id: &str,
    path: &str,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let force = tab_close_force(path)?;
    let (peer_id, controller) = match (origin, agent) {
        (crate::auth::AuthenticatedOrigin::Agent, Some(agent)) => {
            (agent.instance_id.as_str(), Some(agent.tab_id.as_str()))
        }
        (origin, _) if origin.owner_trusted() => (peer_id, None),
        _ => {
            return Err((
                StatusCode::FORBIDDEN,
                "Authenticated remote creator required".into(),
            ))
        }
    };
    Ok(tab_close_relay_response(
        crate::tab_routes::close_remote_tab(state, peer_id, controller, tab_id, force).await,
    ))
}

pub(super) async fn close_tab_from_mesh_owner(
    state: &AppState,
    tab_id: &str,
    path: &str,
    grant: crate::auth::peer_grant::PeerGrant,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let force = tab_close_force(path)?;
    let grant = MeshCloseGrant::new(state.mesh.clone(), move |mesh| grant.current(mesh));
    Ok(tab_close_relay_response(
        crate::web_routes::close_tab_as_owner(
            state,
            tab_id,
            force,
            OwnerTabCloseScope::Mesh(grant),
        )
        .await,
    ))
}

fn tab_close_force(path: &str) -> Result<bool, (StatusCode, String)> {
    let uri = path
        .parse::<axum::http::Uri>()
        .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    let axum::extract::Query(query) =
        axum::extract::Query::<crate::tab_routes::CloseTabQuery>::try_from_uri(&uri)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    Ok(query.force)
}

fn tab_close_relay_response(
    result: Result<axum::Json<serde_json::Value>, (StatusCode, String)>,
) -> codeterm_service_core::mesh::types::RelayResponse {
    match result {
        Ok(axum::Json(data)) => codeterm_service_core::mesh::types::RelayResponse {
            ok: true,
            status: 200,
            data: Some(data),
            error: None,
        },
        Err((status, message)) => codeterm_service_core::mesh::types::RelayResponse {
            ok: false,
            status: status.as_u16(),
            data: None,
            error: Some(message),
        },
    }
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

fn workspace_close_preview_target<'a>(method: &Method, path: &'a str) -> Option<&'a str> {
    if *method != Method::GET {
        return None;
    }
    let workspace_id = path
        .strip_prefix("/api/workspaces/")?
        .strip_suffix("/close-preview")?;
    (!workspace_id.is_empty() && !workspace_id.contains('/')).then_some(workspace_id)
}

pub(super) async fn workspace_close_preview_from_mesh_peer(
    state: &AppState,
    workspace_id: &str,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    match crate::workspace_routes::close_preview_for_mesh_peer(state, workspace_id).await {
        Ok(preview) => {
            let data = serde_json::to_value(preview)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            Ok(codeterm_service_core::mesh::types::RelayResponse {
                ok: true,
                status: StatusCode::OK.as_u16(),
                data: Some(data),
                error: None,
            })
        }
        Err((status, message)) => Ok(codeterm_service_core::mesh::types::RelayResponse {
            ok: false,
            status: status.as_u16(),
            data: None,
            error: Some(message),
        }),
    }
}

pub(super) async fn close_workspace_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    workspace_id: &str,
    force: bool,
    grant: crate::auth::peer_grant::PeerGrant,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    let service = state.svc.clone();
    let peer_id = peer_id.to_string();
    let workspace_id = workspace_id.to_string();
    let grant = MeshCloseGrant::new(state.mesh.clone(), move |mesh| grant.current(mesh));
    let result = tokio::task::spawn_blocking(move || {
        service.close_workspace_from_authenticated_mesh_peer(&peer_id, &workspace_id, force, &grant)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Workspace close task failed: {error}"),
        )
    })?;
    match result {
        Ok(()) => Ok(codeterm_service_core::mesh::types::RelayResponse {
            ok: true,
            status: StatusCode::OK.as_u16(),
            data: Some(serde_json::json!({ "ok": true })),
            error: None,
        }),
        Err(error) => {
            let (status, message) = crate::tab_routes::map_close_error(error);
            Ok(codeterm_service_core::mesh::types::RelayResponse {
                ok: false,
                status: status.as_u16(),
                data: None,
                error: Some(message),
            })
        }
    }
}
