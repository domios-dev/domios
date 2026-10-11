use super::{authority, AppState, RouteError};
use crate::sentinel::graph::{files::FileReference, Identity};
use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NodeFilesBody {
    identity: Identity,
    bucket: Option<String>,
    handler: Option<String>,
}

pub(super) fn validate_request(
    identity: &Identity,
    bucket: Option<&str>,
    handler: Option<&str>,
) -> Result<(), RouteError> {
    if identity.kind != "blueprint"
        || identity.id.is_empty()
        || identity.workspace_id.is_empty()
        || identity.id.len() > 256
        || identity.workspace_id.len() > 256
        || identity
            .team_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        || identity
            .owner_tab_id
            .as_ref()
            .is_some_and(|id| id.is_empty() || id.len() > 256)
        || bucket.is_some() != handler.is_some()
        || [bucket, handler]
            .into_iter()
            .flatten()
            .any(|value| value.is_empty() || value.len() > 256)
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Exact bounded Blueprint identity and command route required".into(),
        ));
    }
    Ok(())
}

pub(super) fn validate_paths(files: Vec<FileReference>) -> Result<Vec<FileReference>, RouteError> {
    if files.len() > 64
        || files.iter().any(|file| {
            file.path.len() > 4096
                || file.short_path.len() > 4096
                || file.name.len() > 4096
                || file.path.contains('\0')
        })
    {
        return Err((
            StatusCode::BAD_REQUEST,
            "Blueprint file references exceed response bounds".into(),
        ));
    }
    Ok(files)
}

pub(super) async fn node_files(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<NodeFilesBody>,
) -> Result<Json<Vec<FileReference>>, RouteError> {
    authority::validate_claim(&state, &headers, &authority::CallerClaim::default())?;
    validate_request(
        &body.identity,
        body.bucket.as_deref(),
        body.handler.as_deref(),
    )?;
    let files = tokio::task::spawn_blocking(move || {
        state.svc.automation_node_files(
            &body.identity,
            body.bucket.as_deref(),
            body.handler.as_deref(),
        )
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    validate_paths(files).map(Json)
}
