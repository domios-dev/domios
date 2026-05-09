use crate::services::store::{self, Service, ServiceCreate, ServiceKind, ServiceUpdate};

#[tauri::command]
pub fn services_list(
    svc: tauri::State<'_, crate::service::ServiceState>,
    cron: tauri::State<'_, crate::CronState>,
    kind: Option<String>,
    enabled: Option<bool>,
) -> Result<Vec<Service>, String> {
    let kind = kind.as_deref().and_then(parse_kind);
    let merged = {
        let cron_lock = cron.lock();
        store::list_local(&svc.services_store, &cron_lock).map_err(|e| e.to_string())?
    };
    Ok(merged
        .into_iter()
        .filter(|s| kind.as_ref().map_or(true, |k| &s.kind == k))
        .filter(|s| enabled.map_or(true, |e| s.enabled == e))
        .collect())
}

#[tauri::command]
pub fn services_create(
    svc: tauri::State<'_, crate::service::ServiceState>,
    input: ServiceCreate,
) -> Result<serde_json::Value, String> {
    if input.kind != ServiceKind::Daemon {
        return Err(
            "Only daemon-kind services can be created via this endpoint; use cron commands for cron jobs"
                .into(),
        );
    }
    let id = svc.services_store.create(input).map_err(|e| e.to_string())?;
    let token = svc.tokens.generate(&id).map_err(|e| e.to_string())?;
    let service = svc
        .services_store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "service vanished after create".to_string())?;
    Ok(serde_json::json!({ "service": service, "token": token }))
}

#[tauri::command]
pub fn services_get(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<Option<Service>, String> {
    svc.services_store.get(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn services_update(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
    patch: ServiceUpdate,
) -> Result<Service, String> {
    svc.services_store
        .update(&id, patch)
        .map_err(|e| e.to_string())?;
    svc.services_store
        .get(&id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("service {} not found after update", id))
}

#[tauri::command]
pub fn services_delete(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<(), String> {
    svc.services_store.delete(&id).map_err(|e| e.to_string())?;
    let _ = svc.tokens.delete(&id);
    Ok(())
}

#[tauri::command]
pub fn services_regenerate_token(
    svc: tauri::State<'_, crate::service::ServiceState>,
    id: String,
) -> Result<String, String> {
    svc.tokens.generate(&id).map_err(|e| e.to_string())
}

fn parse_kind(s: &str) -> Option<ServiceKind> {
    match s {
        "cron" => Some(ServiceKind::Cron),
        "daemon" => Some(ServiceKind::Daemon),
        _ => None,
    }
}
