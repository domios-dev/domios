use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::api::routes::{check_auth, AppState};
use crate::bridge;

const TELEGRAM_SECRET_NAME: &str = "telegram_bot_token";

#[derive(Deserialize)]
pub struct ServiceQuery {
    #[serde(default = "default_service")]
    pub service: String,
}

fn default_service() -> String { "telegram".to_string() }

fn ensure_telegram(service: &str) -> Result<(), (StatusCode, &'static str)> {
    if service == "telegram" {
        Ok(())
    } else {
        Err((StatusCode::BAD_REQUEST, "service unsupported"))
    }
}

pub async fn bridge_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ServiceQuery>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&q.service)?;

    let snap = state.svc.bridge_supervisor.snapshot();
    let token_present = bridge::telegram::token_path().exists();

    let has_secret_in_vault = state.svc.secrets.read().await
        .has(TELEGRAM_SECRET_NAME).await
        .unwrap_or(false);

    let (config_enabled, instance_name) = {
        let cfg = state.config.lock();
        (
            cfg.integrations.telegram_enabled,
            cfg.mesh.instance_name.clone(),
        )
    };

    Ok(Json(json!({
        "service": snap.service,
        "active": snap.active,
        "config_enabled": config_enabled,
        "token_present": token_present,
        "has_secret_in_vault": has_secret_in_vault,
        "secret_name": TELEGRAM_SECRET_NAME,
        "started_at": snap.started_at.map(|t| t.to_rfc3339()),
        "last_poll_ok": snap.last_poll_ok.map(|t| t.to_rfc3339()),
        "last_poll_error": snap.last_poll_error,
        "error_count": snap.error_count,
        "instance_name": instance_name,
    })))
}

#[derive(Deserialize)]
pub struct StopBody {
    #[serde(default = "default_service")]
    pub service: String,
    #[serde(default = "default_stop_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_stop_timeout_ms() -> u64 { 30_000 }

pub async fn bridge_stop(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<StopBody>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&body.service)?;

    let _op = state.svc.bridge_supervisor.op_guard().await;

    let was_active = state.svc.bridge_supervisor.is_active();
    if !was_active {
        let mut cfg = state.config.lock();
        cfg.integrations.telegram_enabled = false;
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            log::warn!("[bridge] failed to persist disable: {}", e);
        }
        return Ok(Json(json!({
            "ok": true,
            "released_in_ms": 0,
            "was_active": false,
        })));
    }

    let elapsed = state.svc.bridge_supervisor
        .stop(Duration::from_millis(body.timeout_ms))
        .await
        .map_err(|_| (StatusCode::REQUEST_TIMEOUT, "bridge stop timed out"))?;

    {
        let mut cfg = state.config.lock();
        cfg.integrations.telegram_enabled = false;
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            log::warn!("[bridge] failed to persist disable: {}", e);
        }
    }

    Ok(Json(json!({
        "ok": true,
        "released_in_ms": elapsed.as_millis() as u64,
        "was_active": true,
    })))
}

#[derive(Deserialize)]
pub struct StartBody {
    #[serde(default = "default_service")]
    pub service: String,
    #[serde(default)]
    pub ensure_token: bool,
    #[serde(default = "default_wait_for_lock_ms")]
    pub wait_for_lock_ms: u64,
}

fn default_wait_for_lock_ms() -> u64 { 10_000 }

pub async fn bridge_start(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<StartBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_telegram(&body.service).map_err(|(s, m)| (s, m.to_string()))?;

    let _op = state.svc.bridge_supervisor.op_guard().await;

    if body.ensure_token && !bridge::telegram::token_path().exists() {
        let store = state.svc.secrets.read().await;
        match store.get(TELEGRAM_SECRET_NAME).await {
            Ok(value) => {
                drop(store);
                if let Err(e) = persist_token_file(value.value.expose_secret()) {
                    return Err((StatusCode::INTERNAL_SERVER_ERROR, e));
                }
            }
            Err(_) => {
                return Err((
                    StatusCode::PRECONDITION_FAILED,
                    "no token in vault".to_string(),
                ));
            }
        }
    }

    {
        let mut cfg = state.config.lock();
        cfg.integrations.telegram_enabled = true;
        if let Err(e) = crate::config::save_config_pub(&cfg) {
            log::warn!("[bridge] failed to persist enable: {}", e);
        }
    }

    let started = Instant::now();
    if let Err(e) = crate::startup::start_telegram_bridge(&state.svc).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e));
    }

    let claim_anchor = state.svc.bridge_supervisor.snapshot().started_at;
    let deadline = started + Duration::from_millis(body.wait_for_lock_ms.max(250));
    let poll = Duration::from_millis(250);
    let mut claimed = false;
    let mut last_error: Option<String> = None;

    loop {
        let snap = state.svc.bridge_supervisor.snapshot();
        if let (Some(ok), Some(anchor)) = (snap.last_poll_ok, claim_anchor) {
            if ok >= anchor {
                claimed = true;
                break;
            }
        }
        if let Some(err) = snap.last_poll_error.clone() {
            last_error = Some(err.clone());
            if err.contains("409") {
                return Err((
                    StatusCode::CONFLICT,
                    format!("conflict claiming Telegram lock: {}", err),
                ));
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(poll).await;
    }

    let started_in_ms = started.elapsed().as_millis() as u64;
    if claimed {
        Ok(Json(json!({
            "ok": true,
            "started_in_ms": started_in_ms,
            "claimed_lock": true,
        })))
    } else {
        Ok(Json(json!({
            "ok": false,
            "started_in_ms": started_in_ms,
            "claimed_lock": false,
            "error": last_error.unwrap_or_else(|| "no successful poll within wait window".to_string()),
        })))
    }
}

fn persist_token_file(token: &str) -> Result<(), String> {
    let path = bridge::telegram::token_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create secrets dir: {}", e))?;
    }
    std::fs::write(&path, token)
        .map_err(|e| format!("write token: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&path)
            .map_err(|e| format!("stat token: {}", e))?
            .permissions();
        perms.set_mode(0o600);
        let _ = std::fs::set_permissions(&path, perms);
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct AnnounceBody {
    #[serde(default = "default_service")]
    pub service: String,
    pub chat_id: i64,
    pub text: String,
}

pub async fn bridge_announce(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnnounceBody>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&body.service)?;

    if !state.svc.bridge_supervisor.is_active() {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"));
    }

    let tx_guard = state.svc.bridge_outbound.lock();
    let tx = tx_guard
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"))?;
    tx.send(crate::bridge::manager::OutboundMessage {
        user_id: Some(body.chat_id),
        content: crate::bridge::traits::OutboundContent::Text { text: body.text },
        append: false,
        buttons: None,
    })
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Send failed"))?;

    Ok(Json(json!({"ok": true})))
}
