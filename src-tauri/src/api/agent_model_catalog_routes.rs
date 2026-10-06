use axum::{
    extract::State as AxumState,
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct RefreshProviderModelsBody {
    provider_id: String,
}

/// Refresh a provider's catalogue before an explicit model listing. A
/// live-required catalogue is forced; an optional one gets the reader-gated
/// re-probe (TTL, credential/config/binary change, unknown verdicts).
pub async fn refresh_agent_provider_models(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<RefreshProviderModelsBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(status, message)| (status, message.to_string()))?;
    let Some(provider) = state.svc.agent_registry.provider_arc(&body.provider_id) else {
        return Err((
            StatusCode::NOT_FOUND,
            format!("unknown agent provider: {}", body.provider_id),
        ));
    };
    let discovery_required = provider
        .model_catalogue_capability()
        .live_discovery_required;
    let refreshed = if discovery_required {
        crate::agent::model_catalog::refresh_models(provider, true)
            .await
            .is_some()
    } else {
        crate::agent::model_catalog::refresh_for_reader(provider)
            .await
            .is_some()
    };
    Ok(Json(serde_json::json!({
        "provider_id": body.provider_id,
        "discovery_required": discovery_required,
        "refreshed": refreshed,
    })))
}
