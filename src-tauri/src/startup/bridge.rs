//! Telegram bridge boot/restart entry. Re-exported at the crate root.

use crate::bridge;
use crate::config;
use crate::service::{self, ServiceState};

/// Start or restart Telegram bridge. Safe to call multiple times.
pub fn start_telegram_bridge(svc: &ServiceState) {
    let token = match bridge::telegram::read_token() {
        Some(t) => t,
        None => { log::warn!("Telegram: no token found"); return; }
    };

    // Abort previous bridge task if running
    if let Some(old_task) = svc.bridge_task.lock().take() {
        log::info!("[bridge] Stopping previous bridge...");
        old_task.abort();
    }

    // Create fresh channel (old tx/rx are dropped with the aborted task)
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    *svc.bridge_outbound.lock() = Some(tx);
    // Store rx for potential future use (not needed here since we use it immediately)
    // but keep the field consistent
    *svc.bridge_outbound_rx.lock() = None;

    // Mark General Agent pane as connected
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
        token, cfg.integrations.telegram_allowed_users.clone(),
    );
    let manager = bridge::BridgeManager::new((**svc).clone());

    let handle = tauri::async_runtime::spawn(async move {
        log::info!("[bridge] spawned on tauri async runtime");
        let result = tokio::task::spawn(async move {
            manager.start(vec![Box::new(telegram)], rx).await;
        }).await;
        match result {
            Ok(()) => log::error!("[bridge] manager exited unexpectedly"),
            Err(e) if e.is_cancelled() => log::info!("[bridge] stopped (restart or shutdown)"),
            Err(e) => log::error!("[bridge] manager PANICKED: {}", e),
        }
    });

    *svc.bridge_task.lock() = Some(handle);
    log::info!("[bridge] Telegram bridge started");
}
