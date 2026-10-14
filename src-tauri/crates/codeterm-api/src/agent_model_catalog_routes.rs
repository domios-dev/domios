use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct RefreshProviderModelsBody {
    provider_id: String,
}

/// Refresh a provider's catalogue before an explicit model listing.
pub async fn refresh_agent_provider_models(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RefreshProviderModelsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let (discovery_required, refreshed) = if let Some(provider) =
        state.svc.agent_registry.provider_arc(&body.provider_id)
    {
        let discovery_required = provider
            .model_catalogue_capability()
            .live_discovery_required;
        codeterm_service_core::agent::model_catalog::invalidate(&body.provider_id);
        let refreshed = codeterm_service_core::agent::model_catalog::refresh_models(provider, true)
            .await
            .is_some();
        (discovery_required, refreshed)
    } else if let Some(provider) = state.svc.chatbackend_registry.get(&body.provider_id) {
        let discovery_required = provider
            .model_catalogue_capability()
            .live_discovery_required;
        codeterm_service_core::agent::model_catalog::invalidate(&body.provider_id);
        let registry = state.svc.chatbackend_registry.clone();
        let provider_id = body.provider_id.clone();
        let refreshed =
            tokio::task::spawn_blocking(move || registry.refresh_and_wait(&provider_id))
                .await
                .map_err(|error| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("chatBackend catalogue refresh failed: {error}"),
                    )
                })?;
        (discovery_required, refreshed)
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
    })))
}
