use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct ConfigQuery {
    key: Option<String>,
}

/// The stored `api.host`/`api.port` is a request to bind, not proof of one: the
/// live socket is snapshotted at process start and no config write can move it.
fn api_bind_disclosure(
    state: &AppState,
    cfg: &crate::config::schema::AppConfig,
) -> Option<serde_json::Value> {
    let pending = state
        .rest_api
        .pending_bind(&format!("{}:{}", cfg.api.host, cfg.api.port))?;
    Some(serde_json::json!({
        "restart_required": true,
        "live": pending.live,
        "configured": pending.configured,
    }))
}

fn api_listener_disclosure(
    state: &AppState,
    cfg: &crate::config::schema::AppConfig,
) -> serde_json::Value {
    serde_json::json!({
        "active": state.rest_api.bound_addr().map(|address| address.to_string()),
        "configured": format!("{}:{}", cfg.api.host, cfg.api.port),
        "authenticated": cfg.api.token.as_ref().is_some_and(|token| !token.is_empty()),
        "cors_allow_origins": if cfg.api.cors {
            cfg.api.cors_allow_origins.clone()
        } else {
            Vec::<String>::new()
        },
    })
}

fn redact_rest_config(mut config: serde_json::Value) -> serde_json::Value {
    if let Some(api) = config
        .get_mut("api")
        .and_then(serde_json::Value::as_object_mut)
    {
        api.insert("token".into(), serde_json::Value::Null);
    }
    config
}

pub async fn get_config_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ConfigQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cfg = state.config.lock().clone();
    let full = serde_json::to_value(&cfg).map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Config serialization failed",
        )
    })?;
    let mut full = redact_rest_config(full);
    if let Some(object) = full.as_object_mut() {
        object.insert("api_listener".into(), api_listener_disclosure(&state, &cfg));
    }
    match q.key {
        Some(key) => {
            let val = dot_get(&full, &key).ok_or((StatusCode::NOT_FOUND, "Key not found"))?;
            let mut answer = serde_json::json!({ "key": &key, "value": val });
            if matches!(key.as_str(), "api.host" | "api.port") {
                answer["api_listener"] = api_listener_disclosure(&state, &cfg);
                if let Some(bind) = api_bind_disclosure(&state, &cfg) {
                    answer["api_bind"] = bind;
                }
            }
            Ok(Json(answer))
        }
        None => Ok(Json(full)),
    }
}

#[derive(Deserialize)]
pub(crate) struct PatchConfigBody {
    key: Option<String>,
    value: Option<serde_json::Value>,
    #[serde(flatten)]
    partial: serde_json::Map<String, serde_json::Value>,
}

pub async fn patch_config_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<PatchConfigBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let cfg = state.config.lock().clone();
    let mut cfg_json = serde_json::to_value(&cfg)
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Serialization failed"))?;

    if let (Some(key), Some(value)) = (body.key.as_deref(), &body.value) {
        dot_set(&mut cfg_json, key, value.clone())
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid key path"))?;
    } else if !body.partial.is_empty() {
        if let serde_json::Value::Object(ref mut map) = cfg_json {
            for (k, v) in &body.partial {
                if k == "key" || k == "value" {
                    continue;
                }
                map.insert(k.clone(), v.clone());
            }
        }
    } else {
        return Err((
            StatusCode::BAD_REQUEST,
            "Provide key+value or partial config",
        ));
    }

    let mut new_cfg: crate::config::schema::AppConfig = serde_json::from_value(cfg_json)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid config value"))?;
    new_cfg.api.token = cfg.api.token.clone();
    crate::config::validate_api_trust_boundary(&new_cfg.api)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Unsafe API listener configuration"))?;

    // builtin fresh (clients send the old colors along with the new name).
    let current_theme_name = state.config.lock().theme.name.clone();
    if new_cfg.theme.name != current_theme_name {
        let requested = new_cfg.theme.name.clone();
        if let Some(mut builtin) = crate::config::schema::get_any_theme(&requested) {
            builtin.name = requested;
            new_cfg.theme = builtin;
        }
    } else {
        crate::config::resolve_theme(&mut new_cfg);
    }

    // Detect rename so can sync mesh state + push the new name to peers. Mirror of the same logic in the Tauri update_config command.
    let prev_name = state.config.lock().mesh.instance_name.clone();
    let prev_mesh_enabled = state.config.lock().mesh.enabled;
    let new_name = new_cfg.mesh.instance_name.clone();
    let new_mesh_enabled = new_cfg.mesh.enabled;
    let renamed = !new_name.is_empty() && new_name != prev_name;
    let mesh_disabled = prev_mesh_enabled && !new_mesh_enabled;

    *state.config.lock() = new_cfg.clone();
    if let Err(e) = crate::config::save_config_pub(&new_cfg) {
        log::error!("Config save failed: {}", e);
    }

    // this, browser clients save the config but never see it applied.
    crate::api::events::emit_everywhere(
        &state.svc.app_handle,
        "config-changed",
        serde_json::to_value(&new_cfg).unwrap_or_default(),
    );

    if mesh_disabled {
        log::info!("[mesh] disabled mid-session — stopping broadcast + marking peers offline");
        crate::mesh::discovery::stop_broadcast(&state.mesh);
        {
            let mut m = state.mesh.lock();
            for peer in m.paired_peers.values_mut() {
                peer.online = false;
            }
            m.discovered_peers.clear();
        }
        crate::api::events::emit_everywhere(
            &state.svc.app_handle,
            "mesh-state-changed",
            serde_json::Value::Null,
        );
    }

    if renamed {
        let mesh = state.mesh.clone();
        {
            let mut m = mesh.lock();
            m.ident.instance_name = new_name.clone();
        }
        log::info!(
            "[mesh] instance renamed: {:?} -> {}, broadcasting announce",
            prev_name,
            new_name
        );
        tokio::spawn(async move {
            let report = crate::mesh::broadcast_announce(&mesh, None).await;
            log::info!(
                "[mesh] post-rename announce sent={} ok={} stale={} errors={}",
                report.sent,
                report.ok,
                report.stale_peers.len(),
                report.errors.len()
            );
        });
    }

    let mut applied = serde_json::json!({ "ok": true });
    applied["api_listener"] = api_listener_disclosure(&state, &new_cfg);
    if let Some(bind) = api_bind_disclosure(&state, &new_cfg) {
        log::warn!(
            "[api] config stores {} but the live listener holds {} — restart to apply",
            bind["configured"],
            bind["live"]
        );
        applied["api_bind"] = bind;
    }
    Ok(Json(applied))
}

fn dot_get(val: &serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = val;
    for key in path.split('.') {
        current = current.get(key)?;
    }
    Some(current.clone())
}

fn dot_set(val: &mut serde_json::Value, path: &str, new_val: serde_json::Value) -> Result<(), ()> {
    let parts: Vec<&str> = path.split('.').collect();
    if parts.is_empty() {
        return Err(());
    }
    let mut current = val;
    for &key in &parts[..parts.len() - 1] {
        current = current.get_mut(key).ok_or(())?;
    }
    if let serde_json::Value::Object(ref mut map) = current {
        map.insert(parts.last().unwrap().to_string(), new_val);
        Ok(())
    } else {
        Err(())
    }
}

pub async fn bridge_send_api(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    let text = body
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "text required"))?;
    let chat_id = body.get("chat_id").and_then(|v| v.as_i64());

    let tx = state.svc.bridge_outbound.lock();
    let tx = tx
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"))?;
    let append = body
        .get("append")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let buttons: Option<crate::bridge::traits::InlineKeyboard> = body
        .get("buttons")
        .and_then(|v| serde_json::from_value(v.clone()).ok());
    tx.send(crate::bridge::manager::OutboundMessage {
        user_id: chat_id,
        content: crate::bridge::traits::OutboundContent::Text {
            text: text.to_string(),
        },
        append,
        buttons,
    })
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Send failed"))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::redact_rest_config;

    #[test]
    fn rest_config_never_serializes_api_token() {
        let config = serde_json::json!({
            "api": { "host": "0.0.0.0", "token": "permanent-secret" },
            "mesh": { "enabled": true }
        });
        let redacted = redact_rest_config(config);
        assert_eq!(redacted["api"]["token"], serde_json::Value::Null);
        assert_eq!(redacted["api"]["host"], "0.0.0.0");
        assert_eq!(redacted["mesh"]["enabled"], true);
    }
}
