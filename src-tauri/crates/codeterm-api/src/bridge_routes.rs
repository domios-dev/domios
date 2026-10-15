use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::routes::{check_auth, AppState};
use codeterm_service_core::bridge;
use codeterm_service_core::bridge_host::AppAttachmentStorePort;
use codeterm_service_core::config::schema::AppConfig;

const TELEGRAM_SECRET_NAME: &str = "telegram_bot_token";

#[derive(Deserialize)]
pub struct ServiceQuery {
    #[serde(default = "default_service")]
    pub service: String,
}

fn default_service() -> String {
    "telegram".to_string()
}

fn ensure_telegram(service: &str) -> Result<(), (StatusCode, &'static str)> {
    codeterm_service_core::bridge::manager::ensure_supported_service(service)
        .map_err(|msg| (StatusCode::BAD_REQUEST, msg))
}

pub async fn bridge_state(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(q): Query<ServiceQuery>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&q.service)?;

    let snap = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .snapshot();
    let token_present = bridge::telegram::token_path(&AppAttachmentStorePort).exists();
    let bot_id = bridge::telegram::read_token(&AppAttachmentStorePort)
        .map(|token| codeterm_bridge::bridge::threads::transport::bot_id_from_token(&token))
        .filter(|id| *id != 0);

    let has_secret_in_vault = state
        .svc
        .secrets
        .read()
        .await
        .has(TELEGRAM_SECRET_NAME)
        .await
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
        "bot_id": bot_id,
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

fn default_stop_timeout_ms() -> u64 {
    30_000
}

pub async fn bridge_stop(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<StopBody>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&body.service)?;

    let _op = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .op_guard()
        .await;
    state
        .svc
        .ext::<std::sync::Arc<codeterm_bridge::bridge::threads::ThreadService>>()
        .detach();

    let was_active = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .is_active();
    if !was_active {
        persist_telegram_enabled(&state, false);
        return Ok(Json(json!({
            "ok": true,
            "released_in_ms": 0,
            "was_active": false,
        })));
    }

    let elapsed = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .stop(Duration::from_millis(body.timeout_ms))
        .await
        .map_err(|_| (StatusCode::REQUEST_TIMEOUT, "bridge stop timed out"))?;

    persist_telegram_enabled(&state, false);

    codeterm_service_core::startup::unmark_tab_connected_service(&state.svc, &body.service);

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
    /// Copy the vault token over an existing token file, so a newly saved token wins.
    #[serde(default)]
    pub replace_token: bool,
    #[serde(default = "default_wait_for_lock_ms")]
    pub wait_for_lock_ms: u64,
}

fn default_wait_for_lock_ms() -> u64 {
    10_000
}

pub async fn bridge_start(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<StartBody>,
) -> Result<Json<Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, m)| (s, m.to_string()))?;
    ensure_telegram(&body.service).map_err(|(s, m)| (s, m.to_string()))?;

    let _op = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .op_guard()
        .await;

    let token_file_exists = bridge::telegram::token_path(&AppAttachmentStorePort).exists();
    if copies_vault_token(body.ensure_token, body.replace_token, token_file_exists) {
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

    persist_telegram_enabled(&state, true);

    let started = Instant::now();
    if let Err(e) = codeterm_service_core::startup::start_telegram_bridge(&state.svc).await {
        return Err((StatusCode::INTERNAL_SERVER_ERROR, e));
    }

    let claim_anchor = state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .snapshot()
        .started_at;
    let deadline = started + Duration::from_millis(body.wait_for_lock_ms.max(250));
    let poll = Duration::from_millis(250);
    let mut claimed = false;
    let mut last_error: Option<String> = None;

    loop {
        let snap = state
            .svc
            .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
            .snapshot();
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

fn copies_vault_token(ensure_token: bool, replace_token: bool, token_file_exists: bool) -> bool {
    replace_token || (ensure_token && !token_file_exists)
}

fn persist_telegram_enabled(state: &AppState, enabled: bool) {
    let snapshot = set_telegram_enabled(
        &state.config,
        enabled,
        codeterm_service_core::config::save_config_pub,
    );
    state.svc.ui.emit(
        "config-changed",
        serde_json::to_value(&snapshot).unwrap_or(serde_json::Value::Null),
    );
}

/// Returns the daemon's config after the flag change; it must reach every client,
/// or the next UI save writes its stale copy back over it.
fn set_telegram_enabled(
    config: &Mutex<AppConfig>,
    enabled: bool,
    save: impl FnOnce(&AppConfig) -> Result<(), String>,
) -> AppConfig {
    let mut cfg = config.lock();
    cfg.integrations.telegram_enabled = enabled;
    if let Err(e) = save(&cfg) {
        log::warn!(
            "[bridge] failed to persist telegram_enabled={}: {}",
            enabled,
            e
        );
    }
    cfg.clone()
}

fn persist_token_file(token: &str) -> Result<(), String> {
    let path = bridge::telegram::token_path(&AppAttachmentStorePort);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create secrets dir: {}", e))?;
    }
    std::fs::write(&path, token).map_err(|e| format!("write token: {}", e))?;
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
    pub chat_id: Option<i64>,
    pub text: String,
}

fn first_allowed_bridge_user(svc: &codeterm_service_core::service::ServiceContext) -> Option<i64> {
    svc.configuration()
        .integrations
        .telegram_allowed_users
        .iter()
        .find_map(|id| id.trim().parse::<i64>().ok())
}

pub async fn bridge_announce(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Json(body): Json<AnnounceBody>,
) -> Result<Json<Value>, (StatusCode, &'static str)> {
    check_auth(&state, &headers)?;
    ensure_telegram(&body.service)?;

    let chat_id = body
        .chat_id
        .or_else(|| first_allowed_bridge_user(&state.svc))
        .ok_or((
            StatusCode::BAD_REQUEST,
            "No chat_id given and no allow-listed bridge users to default to",
        ))?;

    if !state
        .svc
        .ext::<std::sync::Arc<codeterm_service_core::bridge::supervisor::BridgeSupervisor>>()
        .is_active()
    {
        return Err((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"));
    }

    let tx_guard = state.svc.bridge_outbound.lock();
    let tx = tx_guard
        .as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "Bridge not active"))?;
    tx.send(codeterm_service_core::bridge::manager::OutboundMessage {
        user_id: Some(chat_id),
        content: codeterm_service_core::bridge::traits::OutboundContent::Text { text: body.text },
        append: false,
        buttons: None,
        edit: None,
        receipt: None,
        route: codeterm_service_core::bridge::manager::OutboundRoute::Chat,
    })
    .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "Send failed"))?;

    Ok(Json(json!({"ok": true})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with_allowlist() -> Mutex<AppConfig> {
        let mut cfg = AppConfig::default();
        cfg.integrations.telegram_enabled = true;
        cfg.integrations.telegram_allowed_users = vec!["42".to_string()];
        Mutex::new(cfg)
    }

    #[test]
    fn vault_token_is_copied_when_missing_or_explicitly_replaced() {
        assert!(copies_vault_token(true, false, false));
        assert!(!copies_vault_token(true, false, true));
        assert!(copies_vault_token(false, true, true));
        assert!(copies_vault_token(true, true, true));
        assert!(!copies_vault_token(false, false, false));
    }

    #[test]
    fn set_telegram_enabled_returns_the_config_it_persisted() {
        let state = config_with_allowlist();
        let mut saved = None;

        let snapshot = set_telegram_enabled(&state, false, |cfg| {
            saved = Some(serde_json::to_value(cfg).unwrap());
            Ok(())
        });

        let snapshot = serde_json::to_value(&snapshot).unwrap();
        assert_eq!(Some(&snapshot), saved.as_ref());
        assert_eq!(snapshot, serde_json::to_value(&*state.lock()).unwrap());
        assert_eq!(snapshot["integrations"]["telegram_enabled"], false);
        assert_eq!(
            snapshot["integrations"]["telegram_allowed_users"],
            json!(["42"])
        );
    }

    #[test]
    fn set_telegram_enabled_returns_memory_state_when_save_fails() {
        let state = config_with_allowlist();

        let snapshot = set_telegram_enabled(&state, false, |_| Err("disk full".to_string()));

        assert!(!snapshot.integrations.telegram_enabled);
        assert!(!state.lock().integrations.telegram_enabled);
        assert_eq!(
            snapshot.integrations.telegram_allowed_users,
            vec!["42".to_string()]
        );
    }
}
