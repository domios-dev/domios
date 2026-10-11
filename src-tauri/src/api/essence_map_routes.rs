use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::commands::{
    read_workspace_essence_maps_core, read_workspace_essence_source_core, WorkspaceEssenceMapFile,
};

#[derive(Debug, Deserialize, Default)]
pub(crate) struct EssenceMapRequest {
    #[serde(default)]
    path: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct EssenceSourceQuery {
    project: String,
    path: String,
}

pub(crate) async fn list_essence_maps(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(request): Json<EssenceMapRequest>,
) -> Result<Json<Vec<WorkspaceEssenceMapFile>>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let result =
        tokio::task::spawn_blocking(move || read_workspace_essence_maps_core(&request.path))
            .await
            .map_err(|error| {
                crate::logs::record(
                    crate::logs::LogLevel::Error,
                    "map",
                    "Workspace map request failed",
                    Some(serde_json::json!({ "error": error.to_string() })),
                );
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Could not load project maps".to_string(),
                )
            })?;
    Ok(Json(result))
}

pub(crate) async fn read_essence_source(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(query): Query<EssenceSourceQuery>,
) -> Result<Json<String>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    tokio::task::spawn_blocking(move || {
        read_workspace_essence_source_core(&query.project, &query.path)
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| (StatusCode::NOT_FOUND, error))
    .map(Json)
}

#[cfg(test)]
mod tests {
    use super::EssenceMapRequest;

    #[test]
    fn map_request_accepts_project_path_sequence() {
        let request: EssenceMapRequest = serde_json::from_value(serde_json::json!({
            "path": ["/workspace/one & two", "C:/workspace/three"]
        }))
        .unwrap();
        assert_eq!(request.path, ["/workspace/one & two", "C:/workspace/three"]);
        assert!(
            serde_json::from_value::<EssenceMapRequest>(serde_json::json!({
                "path": "/workspace/one"
            }))
            .is_err()
        );
    }
}
