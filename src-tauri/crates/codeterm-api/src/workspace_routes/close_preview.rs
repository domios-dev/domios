use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, Method, StatusCode},
    Json,
};
use codeterm_types::workspace_close_preview::WorkspaceClosePreview;
use serde::Deserialize;

use super::{build_workspace_close_preview, WorkspaceClosePreviewError};
use crate::routes::AppState;

#[derive(Default, Deserialize)]
pub(crate) struct ClosePreviewScope {
    pub(super) team_id: Option<String>,
    #[serde(default)]
    pub(super) resources_only: bool,
}

pub(crate) async fn close_preview(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(scope): Query<ClosePreviewScope>,
) -> Result<Json<WorkspaceClosePreview>, (StatusCode, String)> {
    let (identity, origin) = crate::auth::authenticated_request(&state, &headers)?;
    if origin != crate::auth::AuthenticatedOrigin::Owner {
        return Err((
            StatusCode::FORBIDDEN,
            "Only owner-origin requests may inspect workspace close previews".into(),
        ));
    }
    if headers
        .get("x-codeterm-tab-id")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|tab_id| !tab_id.trim().is_empty())
    {
        return Err((
            StatusCode::FORBIDDEN,
            "An agent tab cannot inspect workspace close previews".into(),
        ));
    }
    match identity {
        crate::auth::CallerIdentity::Local => {}
        crate::auth::CallerIdentity::Peer { access_level, .. } => {
            let path = format!("/api/workspaces/{id}/close-preview");
            if !codeterm_service_core::api_support::access_policy::check_access(
                &access_level,
                &Method::GET,
                &path,
            ) {
                return Err((
                    StatusCode::FORBIDDEN,
                    "Peer lacks workspace close authority".into(),
                ));
            }
        }
        crate::auth::CallerIdentity::Tab(_)
        | crate::auth::CallerIdentity::ServicePush { .. }
        | crate::auth::CallerIdentity::Relayed => {
            return Err((
                StatusCode::FORBIDDEN,
                "Caller lacks authority to inspect workspace close previews".into(),
            ));
        }
    }
    read_close_preview(&state, &id, scope).await.map(Json)
}

pub(crate) async fn close_preview_for_mesh_peer(
    state: &AppState,
    workspace_id: &str,
) -> Result<WorkspaceClosePreview, (StatusCode, String)> {
    read_close_preview(state, workspace_id, ClosePreviewScope::default()).await
}

async fn read_close_preview(
    state: &AppState,
    workspace_id: &str,
    scope: ClosePreviewScope,
) -> Result<WorkspaceClosePreview, (StatusCode, String)> {
    let svc = state.svc.clone();
    let cron = state.cron.clone();
    let installation_id = state.mesh.lock().ident.identity.instance_id.clone();
    let workspace_id = workspace_id.to_string();
    tokio::task::spawn_blocking(move || {
        build_workspace_close_preview(&svc, &cron, &installation_id, &workspace_id, &scope)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Workspace preview worker failed: {error}"),
        )
    })?
    .map_err(WorkspaceClosePreviewError::into_http)
}
