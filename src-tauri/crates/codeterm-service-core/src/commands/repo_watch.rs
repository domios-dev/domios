use serde_json::{json, Value};
use std::path::PathBuf;

use crate::repowatch::RepoWatchRegistryHandle;

/// Shared by the Tauri command and the web-mirror REST route. A degraded
/// repository returns a named discriminant, never a bare error.
pub fn subscribe(registry: &RepoWatchRegistryHandle, cwd: PathBuf) -> Value {
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

pub fn unsubscribe(registry: &RepoWatchRegistryHandle, token: &str) -> Value {
    json!({ "released": registry.release(token) })
}
