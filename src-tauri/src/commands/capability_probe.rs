use tauri::State;
#[tauri::command]
pub async fn refresh_provider_capabilities(
    svc: State<'_, crate::service::ServiceState>,
    tab_id: String,
) -> Result<codeterm_types::agent::capability_probe::ProbeSnapshot, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        crate::service::capability_probe::refresh(&svc, &tab_id, true)
    })
    .await
    .map_err(|error| error.to_string())
}
