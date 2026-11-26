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
    TeamClose {
        peer_id: &'a str,
        team_id: &'a str,
        force: bool,
        confirm: Option<&'a str>,
        force_busy: bool,
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
    if let Some(team_id) = team_close_target(method, path) {
        return PeerRelayAuthorization::TeamClose {
            peer_id,
            team_id,
            force: relay_bool_field(body, "force"),
            confirm: body.get("confirm").and_then(serde_json::Value::as_str),
            force_busy: relay_bool_field(body, "force_busy"),
        };
    }
    match workspace_close_target(method, path) {
        Some(workspace_id) => PeerRelayAuthorization::WorkspaceClose {
            peer_id,
            workspace_id,
            force: relay_bool_field(body, "force"),
        },
        None => PeerRelayAuthorization::Forward,
    }
}

pub(super) async fn dispatch_peer_relay(
    state: &AppState,
    authorization: PeerRelayAuthorization<'_>,
    cmd: &codeterm_service_core::mesh::types::RelayCommand,
    access: &AccessLevel,
    origin: crate::auth::AuthenticatedOrigin,
    agent: Option<&codeterm_service_core::mesh::crown::AuthenticatedAgent>,
    grant: &crate::auth::peer_grant::PeerGrant,
) -> Result<Option<(codeterm_service_core::mesh::types::RelayResponse, bool)>, (StatusCode, String)>
{
    if (!origin.owner_trusted()
        && matches!(authorization, PeerRelayAuthorization::WorkspaceClose { .. }))
        || (origin != crate::auth::AuthenticatedOrigin::Owner
            && matches!(
                authorization,
                PeerRelayAuthorization::WorkspaceClosePreview { .. }
                    | PeerRelayAuthorization::TeamClose { .. }
                    | PeerRelayAuthorization::OwnerTabClose { .. }
            ))
    {
        return Err((
            StatusCode::FORBIDDEN,
            "An agent relay cannot use peer authority".into(),
        ));
    }
    match authorization {
        PeerRelayAuthorization::Rejected => Ok(Some((
            super::relay_failure(
                StatusCode::FORBIDDEN.as_u16(),
                codeterm_mesh::contract::delivery::PEER_RELAY_NOT_AUTHORIZED,
                Some(false),
                format!(
                    "Peer not authorized to {} {} (access_level={:?})",
                    cmd.method, cmd.path, access
                ),
            ),
            true,
        ))),
        PeerRelayAuthorization::WorkspaceClose {
            peer_id,
            workspace_id,
            force,
        } => close_workspace_from_mesh_peer(state, peer_id, workspace_id, force, grant.clone())
            .await
            .map(|response| Some((response, false))),
        PeerRelayAuthorization::WorkspaceClosePreview { workspace_id, .. } => {
            let response = workspace_close_preview_from_mesh_peer(state, workspace_id).await?;
            if !grant.current(&state.mesh.lock()) {
                return Ok(Some((super::grant_refusal(), true)));
            }
            Ok(Some((response, false)))
        }
        PeerRelayAuthorization::TeamClose {
            peer_id,
            team_id,
            force,
            confirm,
            force_busy,
        } => close_team_from_mesh_peer(
            state,
            peer_id,
            team_id,
            force,
            confirm,
            force_busy,
            grant.clone(),
        )
        .await
        .map(|response| Some((response, false))),
        PeerRelayAuthorization::MeshRpc { peer_id } => {
            mesh_rpc_from_mesh_peer(state, peer_id, &cmd.body, origin, agent)
                .await
                .map(|response| Some((response, false)))
        }
        PeerRelayAuthorization::TabClose { peer_id, tab_id } => {
            close_tab_from_mesh_peer(state, peer_id, tab_id, &cmd.path, origin, agent, grant.clone())
                .await
                .map(|response| Some((response, false)))
        }
        PeerRelayAuthorization::OwnerTabClose { tab_id } => {
            close_tab_from_mesh_owner(state, tab_id, &cmd.path, grant.clone())
                .await
                .map(|response| Some((response, false)))
        }
        PeerRelayAuthorization::Forward => Ok(None),
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

fn relay_bool_field(body: &serde_json::Value, field: &str) -> bool {
    body.get(field)
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
    grant: crate::auth::peer_grant::PeerGrant,
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
        crate::tab_routes::close_remote_tab(state, peer_id, controller, tab_id, force, grant).await,
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

fn team_close_target<'a>(method: &Method, path: &'a str) -> Option<&'a str> {
    if *method != Method::POST {
        return None;
    }
    let team_id = path.strip_prefix("/api/teams/")?.strip_suffix("/close")?;
    (!team_id.is_empty() && !team_id.contains('/')).then_some(team_id)
}

fn relay_error(
    status: StatusCode,
    message: String,
) -> codeterm_service_core::mesh::types::RelayResponse {
    codeterm_service_core::mesh::types::RelayResponse {
        ok: false,
        status: status.as_u16(),
        data: None,
        error: Some(message),
    }
}

pub(super) async fn close_team_from_mesh_peer(
    state: &AppState,
    peer_id: &str,
    team_id: &str,
    force: bool,
    confirm: Option<&str>,
    force_busy: bool,
    grant: crate::auth::peer_grant::PeerGrant,
) -> Result<codeterm_service_core::mesh::types::RelayResponse, (StatusCode, String)> {
    if peer_id.trim().is_empty() {
        return Ok(relay_error(
            StatusCode::FORBIDDEN,
            "authenticated mesh peer id is required".into(),
        ));
    }
    if !(force && confirm == Some(team_id)) {
        return Ok(relay_error(
            StatusCode::FORBIDDEN,
            format!(
                "{}: explicit confirmation is required to close Team {team_id}",
                codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
            ),
        ));
    }
    let admission: Result<(), (StatusCode, String)> = {
        let mux = state.svc.mux.read();
        let mesh = state.mesh.lock();
        if !grant.current(&mesh) {
            return Ok(super::grant_refusal());
        }
        match mux.get_team(team_id) {
            None => Err((StatusCode::NOT_FOUND, format!("Team {team_id} not found"))),
            Some(team) => {
                if let Err(error) = mux.ensure_workspace_membership_mutable(&team.workspace_id) {
                    Err((StatusCode::CONFLICT, error))
                } else if mux
                    .get_workspace(&team.workspace_id)
                    .is_some_and(|workspace| workspace.protected)
                {
                    Err((
                        StatusCode::FORBIDDEN,
                        format!(
                            "{}: Cannot close Team in protected workspace",
                            codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
                        ),
                    ))
                } else if mux
                    .team_member_tab_ids(team_id)
                    .iter()
                    .any(|tab_id| mux.get_tab(tab_id).is_some_and(|tab| tab.protected))
                {
                    Err((
                        StatusCode::FORBIDDEN,
                        format!(
                            "{}: Cannot close Team containing a protected Tab",
                            codeterm_service_core::service::TEAM_CLOSE_REFUSAL_PREFIX
                        ),
                    ))
                } else {
                    Ok(())
                }
            }
        }
    };
    if let Err((status, message)) = admission {
        return Ok(relay_error(status, message));
    }
    match codeterm_orchestration::TeamExt::close_team(
        state.svc.as_ref(),
        team_id,
        codeterm_service_core::service::TeamMutationCaller::AuthenticatedMeshPeer(
            peer_id.to_string(),
        ),
        force_busy,
    )
    .await
    {
        Ok(result) => {
            let data = serde_json::to_value(result)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
            Ok(codeterm_service_core::mesh::types::RelayResponse {
                ok: true,
                status: StatusCode::OK.as_u16(),
                data: Some(data),
                error: None,
            })
        }
        Err(error) => {
            let (status, message) = crate::team_routes::map_close_team_error(error);
            Ok(relay_error(status, message))
        }
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
