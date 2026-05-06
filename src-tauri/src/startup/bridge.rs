//! Telegram bridge boot/restart entry. Re-exported at the crate root.

use std::time::Duration;

use chrono::Utc;

use crate::bridge;
use crate::config;
use crate::service::{self, ServiceState};

/// Start (or hot-restart) the Telegram bridge. Async because we may need to
/// stop a previously running supervisor before claiming the getUpdates lock.
/// Caller MUST hold `bridge_supervisor.op_guard()` to serialize against
/// concurrent /api/bridge/{start,stop} — this fn does not lock.
pub async fn start_telegram_bridge(svc: &ServiceState) -> Result<(), String> {
    let token = bridge::telegram::read_token()
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
        let mut mux = svc.mux.lock();
        if let Some(pid) = mux.resolve_pane_id(service::general_agent::GENERAL_ALIAS) {
            if let Some(pane) = mux.get_pane_mut(&pid) {
                if !pane.connected_services.contains(&"telegram".to_string()) {
                    pane.connected_services.push("telegram".to_string());
                }
            }
        }
    }

    let cfg = config::load_config();
    let telegram = bridge::telegram::TelegramService::new(
        token,
        cfg.integrations.telegram_allowed_users.clone(),
    );
    let manager = bridge::BridgeManager::new((**svc).clone());
    let cancel = svc.bridge_supervisor.cancel_token();
    let state = svc.bridge_supervisor.state_handle();

    let handle = tauri::async_runtime::spawn(async move {
        log::info!("[bridge] manager spawned");
        manager.start(vec![Box::new(telegram)], rx, cancel, state).await;
        log::info!("[bridge] manager exited");
    });

    svc.bridge_supervisor.set_task(handle, Utc::now());
    log::info!("[bridge] Telegram bridge started");
    Ok(())
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

/// Test helper — start a TelegramService against an explicit api_base, with a
/// fresh supervisor. Returns the supervisor so the test can stop it later.
#[cfg(test)]
pub async fn start_telegram_bridge_for_test(
    token: String,
    api_base: String,
) -> std::sync::Arc<bridge::supervisor::BridgeSupervisor> {
    use std::sync::Arc;

    let supervisor = Arc::new(bridge::supervisor::BridgeSupervisor::new());
    let cancel = supervisor.cancel_token();
    let state = supervisor.state_handle();

    let svc = bridge::telegram::TelegramService::with_base(token, api_base);
    let (tx, mut rx_inbound) = tokio::sync::mpsc::channel(8);
    tokio::spawn(async move {
        while rx_inbound.recv().await.is_some() {}
    });

    use crate::bridge::traits::MessagingService;
    let svc_box: Box<dyn MessagingService> = Box::new(svc);
    let handle = tauri::async_runtime::spawn(async move {
        svc_box.start(tx, cancel, state).await;
    });
    supervisor.set_task(handle, Utc::now());
    supervisor
}
