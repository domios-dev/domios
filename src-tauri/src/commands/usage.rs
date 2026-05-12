#[tauri::command]
pub fn usage_latest(
    svc: tauri::State<'_, crate::service::ServiceState>,
    provider: String,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let row = svc.usage_store.get_latest(&provider, account_id.as_deref())?;
    serde_json::to_value(row).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn usage_history(
    svc: tauri::State<'_, crate::service::ServiceState>,
    provider: String,
    since: i64,
    limit: Option<i64>,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let lim = limit.unwrap_or(200).clamp(1, 5000);
    let rows = svc
        .usage_store
        .get_history(&provider, since, lim, account_id.as_deref())?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn usage_daily_rollup(
    svc: tauri::State<'_, crate::service::ServiceState>,
    provider: String,
    since: i64,
) -> Result<serde_json::Value, String> {
    let rows = svc.usage_store.get_daily_rollup(&provider, since)?;
    serde_json::to_value(rows).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn usage_projection(
    svc: tauri::State<'_, crate::service::ServiceState>,
    provider: String,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let proj = crate::usage::projection::compute(
        &svc.usage_store,
        &provider,
        account_id.as_deref(),
        now_ms,
    )?;
    serde_json::to_value(proj).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn usage_refresh(
    svc: tauri::State<'_, crate::service::ServiceState>,
    provider: String,
    account_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let poller = svc
        .usage_poller
        .lock()
        .clone()
        .ok_or_else(|| "poller not running".to_string())?;
    poller.refresh_now(&provider, account_id);
    let probe_id = format!("probe_{}", ulid::Ulid::new());
    Ok(serde_json::json!({"queued": true, "probe_id": probe_id}))
}
