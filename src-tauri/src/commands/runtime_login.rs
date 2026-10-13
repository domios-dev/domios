use crate::service::{
    agent_runtime::login::{operate, LoginRequest},
    ServiceState,
};

#[tauri::command]
pub async fn agent_runtime_login(
    svc: tauri::State<'_, ServiceState>,
    tab_id: String,
    request: LoginRequest,
) -> Result<serde_json::Value, String> {
    operate(&svc, &tab_id, request).await
}
