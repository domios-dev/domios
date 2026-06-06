use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use tauri::Emitter;

use crate::api::routes::{check_auth, AppState};

#[derive(Deserialize)]
pub(crate) struct ConfigQuery {
    key: Option<String>,
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
    match q.key {
        Some(key) => {
            let val = dot_get(&full, &key).ok_or((StatusCode::NOT_FOUND, "Key not found"))?;
            Ok(Json(serde_json::json!({ "key": key, "value": val })))
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

    let new_cfg: crate::config::schema::AppConfig = serde_json::from_value(cfg_json)
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid config value"))?;

    // Detect rename so we can sync mesh state + push the new name to peers.
    // Mirror of the same logic in the Tauri update_config command.
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
        crate::api::events::emit_everywhere(&state.svc.app_handle, "mesh-state-changed", serde_json::Value::Null);
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

    Ok(Json(serde_json::json!({ "ok": true })))
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
