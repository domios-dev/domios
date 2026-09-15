//! Telegram bridge boot/restart entry. Re-exported at the crate root.

use std::time::Duration;

use chrono::Utc;

use crate::bridge;
use crate::bridge_host::{attachment_store, ServiceBridgeTabPort};
use crate::config;
use crate::service::{self, ServiceState};

/// Start (or hot-restart) the Telegram bridge. Async because we may need to
/// stop a previously running supervisor before claiming the getUpdates lock.
/// Caller MUST hold `bridge_supervisor.op_guard()` to serialize against
/// concurrent /api/bridge/{start,stop} — this fn does not lock.
pub async fn start_telegram_bridge(svc: &ServiceState) -> Result<(), String> {
    let attachments = attachment_store();
    let token = bridge::telegram::read_token(attachments.as_ref())
        .ok_or_else(|| "telegram: no token at secrets_dir/telegram.token".to_string())?;

    if svc.bridge_supervisor.is_active() {
        log::info!("[bridge] hot-restart: stopping previous supervisor");
        svc.bridge_supervisor
            .stop(Duration::from_secs(30))
            .await
            .map_err(|e| format!("previous bridge stop failed: {}", e))?;
    }

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    *svc.bridge_outbound.lock() = Some(tx);
    *svc.bridge_outbound_rx.lock() = None;

    {
        let mut mux = svc.mux.write();
        if let Some(pid) = mux.resolve_tab_id(service::general_agent::GENERAL_ALIAS) {
            if let Some(tab) = mux.get_tab_mut(&pid) {
                if !tab.connected_services.contains(&"telegram".to_string()) {
                    tab.connected_services.push("telegram".to_string());
                }
            }
        }
    }

    svc.bridge_threads.attach(
        std::sync::Arc::new(codeterm_bridge::bridge::threads::TelegramTopicTransport::new(
            token.clone(),
        )),
        codeterm_bridge::bridge::threads::transport::bot_id_from_token(&token),
    );

    let cfg = config::load_config();
    let telegram = bridge::telegram::TelegramService::new(
        token,
        cfg.integrations.telegram_allowed_users.clone(),
        attachments,
    );
    let tab: std::sync::Arc<dyn codeterm_bridge::ports::BridgeTabPort> =
        std::sync::Arc::new(ServiceBridgeTabPort((**svc).clone()));
    let manager = bridge::BridgeManager::new(tab);
    let cancel = svc.bridge_supervisor.cancel_token();
    let status = svc.bridge_supervisor.as_status_sink();

    let handle = tokio::spawn(async move {
        log::info!("[bridge] manager spawned");
        manager
            .start(vec![Box::new(telegram)], rx, cancel, status)
            .await;
        log::info!("[bridge] manager exited");
    });

    svc.bridge_supervisor.set_task(handle, Utc::now());
    log::info!("[bridge] Telegram bridge started");
    Ok(())
}

/// Drop the service tag from the General Agent tab's connected_services so
/// the frontend stops showing the integration logo. Safe to call when the
/// tab / service tag is missing — silent no-op.
pub fn unmark_tab_connected_service(svc: &ServiceState, service: &str) {
    let removed = {
        let mut mux = svc.mux.write();
        let Some(pid) = mux.resolve_tab_id(service::general_agent::GENERAL_ALIAS) else {
            return;
        };
        let Some(tab) = mux.get_tab_mut(&pid) else {
            return;
        };
        let before = tab.connected_services.len();
        tab.connected_services.retain(|s| s != service);
        before != tab.connected_services.len()
    };
    if removed {
        svc.emit_state_changed();
    }
}

/// Synchronous wrapper used at boot — schedules the async start on the
/// tauri runtime and logs failures. Callers must NOT block on completion.
pub fn spawn_telegram_bridge(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        let _op = svc.bridge_supervisor.op_guard().await;
        if let Err(e) = start_telegram_bridge(&svc).await {
            log::warn!("[bridge] start failed: {}", e);
        }
    });
}
