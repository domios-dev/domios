use crate::{
    auth::{AgentHeaders, RequestActor},
    routes::AppState,
};
use axum::{extract::State, http::StatusCode, Json};

fn admit(actor: &RequestActor) -> Result<(), (StatusCode, String)> {
    if matches!(actor, RequestActor::Owner) {
        Ok(())
    } else {
        Err((
            StatusCode::FORBIDDEN,
            "Neighbor sidebar requires owner authority".into(),
        ))
    }
}

pub async fn read(
    State(state): State<AppState>,
    headers: AgentHeaders,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    admit(&headers.actor)?;
    state
        .svc
        .neighbor_sidebar_projection()
        .await
        .map(Json)
        .map_err(super::unavailable)
}

#[cfg(test)]
mod tests;
