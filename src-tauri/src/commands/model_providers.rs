use crate::service::{
    model_providers::{ModelProviderError, ModelProviderRequest},
    ServiceState,
};

#[tauri::command]
pub async fn model_provider_manage(
    request: ModelProviderRequest,
    provider_id: Option<String>,
    svc: tauri::State<'_, ServiceState>,
) -> Result<serde_json::Value, ModelProviderError> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.model_provider_command(provider_id.as_deref(), request))
        .await
        .map_err(|_| ModelProviderError::Config)?
}
