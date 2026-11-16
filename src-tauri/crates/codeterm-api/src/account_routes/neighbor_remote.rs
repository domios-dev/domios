use super::*;
use crate::auth::{AgentHeaders, RequestActor};
use codeterm_neighbors_service::service::neighbors::{NeighborCommand, NeighborRuntimeCommand};

fn admit(actor: &RequestActor) -> Result<(), Response> {
    if matches!(actor, RequestActor::Owner) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Neighbor viewing requires owner authority",
        )
            .into_response())
    }
}

fn read_only() -> Response {
    (
        StatusCode::FORBIDDEN,
        "Open this machine directly to change its Neighbors settings.",
    )
        .into_response()
}

fn runtime_read(command: &NeighborRuntimeCommand) -> bool {
    matches!(
        command,
        NeighborRuntimeCommand::Status
            | NeighborRuntimeCommand::History { .. }
            | NeighborRuntimeCommand::Permission { .. }
            | NeighborRuntimeCommand::NotificationPreference { .. }
            | NeighborRuntimeCommand::DeliveryStatus { .. }
    )
}

pub async fn list(
    State(state): State<AppState>,
    headers: AgentHeaders,
    Json(command): Json<NeighborCommand>,
) -> Result<Json<serde_json::Value>, Response> {
    admit(&headers.actor)?;
    if !matches!(
        &command,
        NeighborCommand::List {} | NeighborCommand::Refresh {}
    ) {
        return Err(read_only());
    }
    super::neighbors(
        State(state),
        Some(Extension(CallerIdentity::Local)),
        Json(command),
    )
    .await
    .map_err(IntoResponse::into_response)
}

pub async fn runtime(
    State(state): State<AppState>,
    headers: AgentHeaders,
    Json(command): Json<NeighborRuntimeCommand>,
) -> Result<Json<serde_json::Value>, Response> {
    admit(&headers.actor)?;
    if !runtime_read(&command) {
        return Err(read_only());
    }
    super::neighbor_runtime(
        State(state),
        Some(Extension(CallerIdentity::Local)),
        Default::default(),
        Json(command),
    )
    .await
}

pub async fn files(
    State(state): State<AppState>,
    headers: AgentHeaders,
    Json(command): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, Response> {
    admit(&headers.actor)?;
    if !matches!(command["operation"].as_str(), Some("list" | "usage")) {
        return Err(read_only());
    }
    super::neighbor_files(
        State(state),
        Some(Extension(CallerIdentity::Local)),
        Json(command),
    )
    .await
}

#[cfg(test)]
mod tests;
