#[tauri::command]
pub async fn automation_graph_snapshot(
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: String,
    team_id: serde_json::Value,
) -> Result<crate::sentinel::graph::Snapshot, String> {
    let team = crate::service::sentinel::graph::explicit_team(&team_id)?;
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        svc.automation_graph_snapshot(&workspace_id, team.as_deref())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn automation_graph_stop_sentinel(
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: String,
    team_id: serde_json::Value,
    id: String,
) -> Result<serde_json::Value, String> {
    let team = crate::service::sentinel::graph::explicit_team(&team_id)?;
    let svc = svc.inner().clone();
    let result = tokio::task::spawn_blocking(move || {
        svc.automation_graph_stop_host(&workspace_id, team.as_deref(), &id)
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(serde_json::json!({"id":result.id,"state":"stopped","reason":result.last_reason}))
}

#[tauri::command]
pub async fn automation_graph_workspace_snapshot(
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: String,
) -> Result<crate::sentinel::graph::Snapshot, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || svc.automation_graph_workspace_snapshot(&workspace_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn automation_graph_delete_sentinel(
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: String,
    team_id: serde_json::Value,
    id: String,
) -> Result<(), String> {
    let team = crate::service::sentinel::graph::explicit_team(&team_id)?;
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        svc.automation_graph_delete_host(&workspace_id, team.as_deref(), &id)
    })
    .await
    .map_err(|e| e.to_string())?
}
