use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub struct RefreshProviderModelsBody {
    provider_id: String,
}

/// Refresh a provider's catalogue before an explicit model listing.
pub async fn refresh_agent_provider_models(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RefreshProviderModelsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let (discovery_required, refreshed, reason) = if let Some(provider) =
        state.svc.agent_registry.provider_arc(&body.provider_id)
    {
        state
            .svc
            .refresh_remote_task_environments(&body.provider_id)
            .await
            .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))?;
        let discovery_required = provider
            .model_catalogue_capability()
            .live_discovery_required;
        codeterm_service_core::agent::model_catalog::invalidate(&body.provider_id);
        let refreshed = codeterm_service_core::agent::model_catalog::refresh_models(provider, true)
            .await
            .is_some();
        let reason = (!refreshed)
            .then(|| {
                codeterm_service_core::agent::model_catalog::last_refresh_failure(&body.provider_id)
            })
            .flatten()
            .map(|failure| failure.to_string());
        (discovery_required, refreshed, reason)
    } else if let Some(provider) = state.svc.chatbackend_registry.get(&body.provider_id) {
        let discovery_required = provider
            .model_catalogue_capability()
            .live_discovery_required;
        codeterm_service_core::agent::model_catalog::invalidate(&body.provider_id);
        let registry = state.svc.chatbackend_registry.clone();
        let provider_id = body.provider_id.clone();
        let completed =
            tokio::task::spawn_blocking(move || registry.refresh_and_wait(&provider_id))
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("chatBackend catalogue refresh failed: {error}"),
                    )
                })?;
        let reason = if completed {
            state
                .svc
                .chatbackend_registry
                .model_capability_error(&body.provider_id)
        } else {
            Some("the catalogue refresh did not complete".to_string())
        };
        (discovery_required, reason.is_none(), reason)
    } else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown provider: {}", body.provider_id),
        ));
    };
    Ok(Json(serde_json::json!({
        "provider_id": body.provider_id,
        "version": codeterm_service_core::agent::model_catalog::current_version(&body.provider_id),
        "discovery_required": discovery_required,
        "refreshed": refreshed,
        "reason": reason,
    })))
}

#[cfg(test)]
mod tests;
