#[tauri::command]
pub async fn blueprint_export_file(
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: String,
    team_id: serde_json::Value,
    id: String,
) -> Result<codeterm_types::blueprint_recipe::Recipe, String> {
    let team = crate::service::sentinel::graph::explicit_team(&team_id)?;
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        svc.blueprint_export_host(&workspace_id, team.as_deref(), &id)
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn blueprint_import_file(
    svc: tauri::State<'_, crate::service::ServiceState>,
    caller_tab_id: String,
    recipe: codeterm_types::blueprint_recipe::Recipe,
    assignments: Vec<String>,
) -> Result<serde_json::Value, String> {
    let svc = svc.inner().clone();
    tokio::task::spawn_blocking(move || {
        svc.blueprint_import_host(&caller_tab_id, recipe, &assignments)
    })
    .await
    .map_err(|error| error.to_string())?
}
