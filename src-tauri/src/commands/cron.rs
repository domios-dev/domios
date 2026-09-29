#[tauri::command]
pub fn cron_list(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    workspace_id: Option<String>,
) -> Result<serde_json::Value, String> {
    let jobs = svc.cron_list(&cron, workspace_id.as_deref());
    serde_json::to_value(jobs).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_get(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_get(&cron, &id)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_create(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    name: String,
    workspace_id: String,
    schedule: String,
    task: String,
    agent: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
    oneshot: Option<bool>,
    jitter_max_ms: Option<u64>,
    tz: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<serde_json::Value, String> {
    let params = crate::cron::store::CronCreateParams {
        name,
        workspace_id,
        team_id: None,
        detach_from_team: false,
        schedule,
        task,
        description,
        agent,
        model,
        reasoning_effort,
        cwd,
        role: None,
        skill: None,
        creator_tab_id: None,
        report_target_tab_id: None,
        enabled: Some(true),
        max_history: None,
        oneshot,
        jitter_max_ms,
        tz,
        timeout_seconds,
    };
    let job = svc.cron_create(&cron, &wake, params)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_update(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    name: Option<String>,
    schedule: Option<String>,
    task: Option<String>,
    cwd: Option<String>,
    description: Option<String>,
    enabled: Option<bool>,
    agent: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    jitter_max_ms: Option<u64>,
    tz: Option<String>,
    timeout_seconds: Option<u64>,
) -> Result<serde_json::Value, String> {
    let patch = crate::cron::store::CronUpdateParams {
        name,
        description,
        schedule,
        task,
        cwd,
        role: None,
        skill: None,
        enabled,
        max_history: None,
        agent,
        model,
        reasoning_effort,
        jitter_max_ms,
        tz,
        timeout_seconds,
    };
    let job = svc.cron_update(&cron, &wake, &id, patch)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_delete(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<(), String> {
    svc.cron_delete(&cron, &wake, &id)
}

#[tauri::command]
pub fn cron_toggle(
    cron: tauri::State<'_, crate::CronState>,
    wake: tauri::State<'_, crate::CronWake>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    enabled: bool,
) -> Result<serde_json::Value, String> {
    let job = svc.cron_enable(&cron, &wake, &id, enabled)?;
    serde_json::to_value(job).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_trigger(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let run = svc.cron_trigger(&cron, &id)?;
    serde_json::to_value(run).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_cancel(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let cancelled = svc.cron_cancel_running(&cron, &id)?;
    Ok(serde_json::json!({ "cancelled": cancelled }))
}

#[tauri::command]
pub fn cron_history(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let history = svc.cron_history(&cron, &id)?;
    serde_json::to_value(history).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn cron_read_log(
    cron: tauri::State<'_, crate::CronState>,
    svc: tauri::State<'_, crate::service::ServiceState>,
    job_id: String,
    run_id: String,
) -> Result<String, String> {
    if let Some(text) = cron.lock().read_log(&job_id, &run_id) {
        return Ok(text);
    }

    let tab_id = cron
        .lock()
        .get_history(&job_id)
        .iter()
        .find(|r| r.run_id == run_id)
        .and_then(|r| r.tab_id.clone());

    if let Some(tab_id) = tab_id {
        let cmd = crate::dispatch::Command::GetOutput {
            tab_id,
            offset: None,
            max_bytes: None,
            strip_ansi: true,
            full: true,
        };
        let result = crate::dispatch::dispatch(&cmd, svc.as_ref())?;
        // GetOutput response uses "output", not "text"
        let text = result
            .into_json()
            .get("output")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        return Ok(text);
    }

    Err("Log not found".to_string())
}
