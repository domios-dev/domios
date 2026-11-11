use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Extension, Json,
};
use serde::Deserialize;

use crate::routes::AppState;

#[derive(Deserialize)]
pub struct ConfigQuery {
    key: Option<String>,
}

/// The stored `api.host`/`api.port` is a request to bind, not proof of one: the
/// live socket is snapshotted at process start and no config write can move it.
fn api_bind_disclosure(
    state: &AppState,
    cfg: &codeterm_service_core::config::schema::AppConfig,
) -> Option<serde_json::Value> {
    let configured = codeterm_service_core::config::canonical_api_bind_addr(&cfg.api)
        .ok()?
        .to_string();
    let pending = state.rest_api.pending_bind(&configured)?;
    Some(serde_json::json!({
        "restart_required": true,
        "live": pending.live,
        "configured": pending.configured,
    }))
}

fn api_listener_disclosure(
    state: &AppState,
    cfg: &codeterm_service_core::config::schema::AppConfig,
) -> serde_json::Value {
    let mut value = api_listener_value(
        state.rest_api.bound_addr(),
        state.rest_api.runtime_policy(),
        cfg,
    );
    value["ready_addresses"] = serde_json::json!(state.rest_api.listeners.ready());
    value["configured_mesh_tailnet_host"] = serde_json::json!(cfg.api.mesh_tailnet_host);
    value["requested_mesh_tailnet_address"] =
        serde_json::json!(state.rest_api.listeners.remote_bind());
    value["restart_required"] = serde_json::json!(
        value["restart_required"].as_bool().unwrap_or(true)
            || cfg.api.mesh_tailnet_host.as_deref()
                != state
                    .rest_api
                    .listeners
                    .remote_bind()
                    .map(|addr| addr.ip().to_string())
                    .as_deref()
    );
    value
}

fn api_listener_value(
    active_address: Option<std::net::SocketAddr>,
    active_policy: codeterm_service_core::startup::RestApiRuntimePolicy,
    cfg: &codeterm_service_core::config::schema::AppConfig,
) -> serde_json::Value {
    let active_enabled = active_address.is_some() && active_policy.enabled;
    let configured_address = codeterm_service_core::config::canonical_api_bind_addr(&cfg.api)
        .map(|address| address.to_string())
        .unwrap_or_else(|_| format!("{}:{}", cfg.api.host, cfg.api.port));
    let configured_origins = if cfg.api.cors {
        cfg.api.cors_allow_origins.clone()
    } else {
        Vec::new()
    };
    let active_address_text = active_address.map(|address| address.to_string());
    let restart_required = active_enabled != cfg.api.enabled
        || (cfg.api.enabled
            && (active_address_text.as_deref() != Some(configured_address.as_str())
                || active_policy.cors != cfg.api.cors
                || active_policy.cors_allow_origins != configured_origins));
    serde_json::json!({
        "active": {
            "enabled": active_enabled,
            "address": active_address_text,
            "cors": active_enabled && active_policy.cors,
            "cors_allow_origins": if active_enabled && active_policy.cors {
                active_policy.cors_allow_origins
            } else {
                Vec::<String>::new()
            },
        },
        "configured": {
            "enabled": cfg.api.enabled,
            "address": configured_address,
            "cors": cfg.api.cors,
            "cors_allow_origins": configured_origins,
            "authenticated": cfg.api.token.as_ref().is_some_and(|token| !token.is_empty()),
        },
        "restart_required": restart_required,
    })
}

pub(crate) fn redact_rest_config(mut config: serde_json::Value) -> serde_json::Value {
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
    _headers: HeaderMap,
    Query(q): Query<ConfigQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
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
            if api_query_includes_listener(&key) {
                answer["api_listener"] = api_listener_disclosure(&state, &cfg);
            }
            if matches!(key.as_str(), "api.host" | "api.port") {
                if let Some(bind) = api_bind_disclosure(&state, &cfg) {
                    answer["api_bind"] = bind;
                }
            }
            Ok(Json(answer))
        }
        None => Ok(Json(full)),
    }
}

fn api_query_includes_listener(key: &str) -> bool {
    key == "api" || key.starts_with("api.")
}

#[derive(Deserialize)]
pub struct PatchConfigBody {
    key: Option<String>,
    value: Option<serde_json::Value>,
    #[serde(flatten)]
    partial: serde_json::Map<String, serde_json::Value>,
}

/// THE AUTHORITY RULE at the config seam. A write that names one key is the
/// shape an agent's own CLI makes, and a write that arrives from inside a tab
/// came from something running in one — either way nobody tapped anything, so
/// neither may move where notifications go. An agent offers instead, and the
/// tapped answer is what applies it.
pub(crate) fn admit_config_write(
    caller_tab: Option<&str>,
    key: Option<&str>,
    partial: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), String> {
    use codeterm_service_core::notification::authority::{admit_unattended_change, SURFACE_ROOT};
    if let Some(key) = key {
        return admit_unattended_change(key);
    }
    if caller_tab.is_some() && partial.contains_key(SURFACE_ROOT) {
        return admit_unattended_change(SURFACE_ROOT);
    }
    Ok(())
}

pub async fn patch_config_api(
    AxumState(state): AxumState<AppState>,
    Extension(origin): Extension<crate::auth::AuthenticatedOrigin>,
    headers: HeaderMap,
    Json(body): Json<PatchConfigBody>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let cfg = state.config.lock().clone();
    let mut cfg_json = serde_json::to_value(&cfg)
        .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;

    if let (Some(key), Some(value)) = (body.key.as_deref(), &body.value) {
        dot_set(&mut cfg_json, key, value.clone())
            .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid key path".to_string()))?;
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
            "Provide key+value or partial config".to_string(),
        ));
    }

    let mut new_cfg: codeterm_service_core::config::schema::AppConfig =
        serde_json::from_value(cfg_json)
            .map_err(|error| (StatusCode::BAD_REQUEST, error.to_string()))?;
    if new_cfg.browser_upload_folders != cfg.browser_upload_folders
        && origin != crate::auth::AuthenticatedOrigin::Owner
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Only the owner can change browser upload folders".into(),
        ));
    }
    let headers = crate::auth::ActorHeaders::authenticate(&state, headers)?;
    if admit_config_write(
        crate::tab_routes::caller_tab_header(&headers).as_deref(),
        body.key.as_deref(),
        &body.partial,
    )
    .is_err()
    {
        return Err((
            StatusCode::FORBIDDEN,
            "Where notifications go changes only on a tapped answer".to_string(),
        ));
    }
    for (profile, folders) in &new_cfg.browser_upload_folders {
        codeterm_service_core::service::browser::profile::validate_named_profile(profile)
            .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
        for folder in folders {
            let path = std::path::Path::new(folder);
            if !path.is_absolute()
                || folder.len() > 4096
                || path.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
                || folder
                    .split(['/', '\\'])
                    .any(|part| matches!(part, "." | ".."))
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    "Browser upload folders must be absolute folders without traversal".into(),
                ));
            }
        }
    }
    codeterm_service_core::config::sanitize_plugin_settings(&mut new_cfg);
    if new_cfg.general_agent.agent_provider != cfg.general_agent.agent_provider {
        codeterm_service_core::service::general_agent::validate_general_agent_provider(
            &state.svc.agent_registry,
            &new_cfg.general_agent.agent_provider,
        )
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    }
    codeterm_service_core::config::bounds::validate_app_config(&new_cfg)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    new_cfg.api.token = cfg.api.token.clone();
    codeterm_service_core::config::validate_api_trust_boundary(&new_cfg.api)
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;

    // builtin fresh (clients send the old colors along with the new name).
    let current_theme_name = state.config.lock().theme.name.clone();
    if new_cfg.theme.name != current_theme_name {
        let requested = new_cfg.theme.name.clone();
        if let Some(mut builtin) = codeterm_service_core::config::schema::get_any_theme(&requested)
        {
            builtin.name = requested;
            new_cfg.theme = builtin;
        }
    } else {
        codeterm_service_core::config::resolve_theme(&mut new_cfg);
    }

    // Detect rename so can sync mesh state + push the new name to peers. Mirror of the same logic in the Tauri update_config command.
    let prev_name = state.config.lock().mesh.instance_name.clone();
    let prev_mesh_enabled = state.config.lock().mesh.enabled;
    let neighbors_changed = cfg.neighbors.enabled != new_cfg.neighbors.enabled;
    let new_name = new_cfg.mesh.instance_name.clone();
    let new_mesh_enabled = new_cfg.mesh.enabled;
    let renamed = !new_name.is_empty() && new_name != prev_name;
    let mesh_disabled = prev_mesh_enabled && !new_mesh_enabled;
    let changed_catalogue_settings =
        codeterm_service_core::config::changed_provider_settings(&cfg, &new_cfg);

    let config_state = state.config.clone();
    let expected_folders = cfg.browser_upload_folders.clone();
    new_cfg = tokio::task::spawn_blocking(move || {
        if origin == crate::auth::AuthenticatedOrigin::Owner {
            codeterm_service_core::config::save_owner_config_and_apply(
                &config_state,
                new_cfg,
                &expected_folders,
            )
            .map_err(|error| match error {
                codeterm_service_core::config::ConfigCommitError::StaleUploadFolders => {
                    (StatusCode::CONFLICT, error.to_string())
                }
                codeterm_service_core::config::ConfigCommitError::Save(message) => {
                    (StatusCode::INTERNAL_SERVER_ERROR, message)
                }
            })
        } else {
            codeterm_service_core::config::save_config_and_apply(&config_state, new_cfg)
                .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error))
        }
    })
    .await
    .map_err(|error| (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;

    if new_cfg.theme.name != current_theme_name {
        state
            .svc
            .record_product_feature(codeterm_analytics::Feature::ThemeChanged);
    }

    if serde_json::to_value(&cfg.plugins).ok() != serde_json::to_value(&new_cfg.plugins).ok() {
        if let Err(error) =
            codeterm_commands::update::discovery::plugins_changed(state.svc.ui.clone()).await
        {
            log::warn!("[plugin-update] settings availability refresh failed: {error}");
        }
    }
    codeterm_service_core::agent::model_binary_watch::refresh_after_settings_change(
        &state.svc.agent_registry,
        &state.svc.chatbackend_registry,
        changed_catalogue_settings,
    );

    // this, browser clients save the config but never see it applied.
    state.svc.ui.emit(
        "config-changed",
        serde_json::to_value(&new_cfg).unwrap_or_default(),
    );
    if neighbors_changed {
        state.svc.neighbors_config_changed();
    }

    if mesh_disabled {
        log::info!("[mesh] disabled mid-session — marking peers offline");
        {
            let mut m = state.mesh.lock();
            for peer in m.paired_peers.values_mut() {
                peer.online = false;
            }
        }
        state
            .svc
            .ui
            .emit("mesh-state-changed", serde_json::Value::Null);
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
            let report = codeterm_service_core::mesh::broadcast_announce(&mesh, None).await;
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
    _headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<serde_json::Value>, (StatusCode, &'static str)> {
    let text = body
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or((StatusCode::BAD_REQUEST, "text required"))?;
    let chat_id = body.get("chat_id").and_then(|v| v.as_i64());

    let buttons = codeterm_service_core::bridge::manager::caller_buttons(body.get("buttons"))
        .map_err(|message| (StatusCode::BAD_REQUEST, message))?;
    let tx = state.svc.bridge_outbound.lock();
    let tx = tx
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"))?;
    let append = body
        .get("append")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    tx.send(codeterm_service_core::bridge::manager::OutboundMessage {
        user_id: chat_id,
        content: codeterm_service_core::bridge::traits::OutboundContent::Text {
            text: text.to_string(),
        },
        append,
        buttons,
        edit: None,
        completion: None,
        receipt: None,
        route: codeterm_service_core::bridge::manager::OutboundRoute::Chat,
    })
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Send failed"))?;

    Ok(Json(serde_json::json!({"ok": true})))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod upload_grants_tests;
