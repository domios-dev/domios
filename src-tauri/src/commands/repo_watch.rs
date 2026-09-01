use serde_json::{json, Value};
use std::path::PathBuf;

use crate::repowatch::RepoWatchRegistryHandle;

/// Shared by the Tauri command and the web-mirror REST route. A degraded
/// repository returns a named discriminant, never a bare error.
pub(crate) fn subscribe(registry: &RepoWatchRegistryHandle, cwd: PathBuf) -> Value {
    match registry.subscribe(&cwd) {
        Ok(sub) => json!({
            "status": "watching",
            "token": sub.token,
            "identity": sub.identity,
        }),
        Err(e) => json!({
            "status": "unavailable",
            "reason": e.reason.as_str(),
            "identity": e.identity,
            "detail": e.detail,
        }),
    }
}

pub(crate) fn unsubscribe(registry: &RepoWatchRegistryHandle, token: &str) -> Value {
    json!({ "released": registry.release(token) })
}

#[tauri::command]
pub async fn repo_watch_subscribe(
    svc: tauri::State<'_, crate::service::ServiceState>,
    cwd: String,
) -> Result<Value, String> {
    let registry = svc.repo_watch.clone();
    tokio::task::spawn_blocking(move || subscribe(&registry, PathBuf::from(cwd)))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn repo_watch_unsubscribe(
    svc: tauri::State<'_, crate::service::ServiceState>,
    token: String,
) -> Result<Value, String> {
    let registry = svc.repo_watch.clone();
    tokio::task::spawn_blocking(move || unsubscribe(&registry, &token))
        .await
        .map_err(|e| e.to_string())
}
