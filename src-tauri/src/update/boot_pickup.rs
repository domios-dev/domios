//! Boot-time pickup of `upgrade-result.json`.
//!
//! Called once after the bridge has been wired. Reads the result file
//! left by the orchestrator script (if any), pushes a Telegram message
//! through the bridge, then deletes the file.

use std::time::Duration;

use crate::bridge::manager::OutboundMessage;
use crate::bridge::traits::OutboundContent;
use crate::service::ServiceState;

use super::result::{read_result, UPGRADE_RESULT_FILENAME};

/// Spawn the boot-pickup task. Doesn't block the caller. Safe to call
/// when no Telegram bridge is configured — it just no-ops without a
/// chat id or bridge sender.
pub fn spawn(svc: ServiceState) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run(&svc).await {
            log::warn!("[self-update] boot pickup failed: {}", e);
        }
    });
}

async fn run(svc: &ServiceState) -> Result<(), String> {
    let result_path = crate::paths::data_dir().join(UPGRADE_RESULT_FILENAME);
    let Some(result) = read_result(&result_path)? else {
        return Ok(());
    };
    log::info!(
        "[self-update] boot pickup: {} (v{} -> v{}, observed=v{})",
        result.status.as_str(),
        result.old_version,
        result.new_version,
        result.observed_version,
    );

    let chat_id = result.notify_chat_id;
    let Some(chat_id) = chat_id else {
        log::info!("[self-update] no notify_chat_id in result — skipping telegram, deleting file");
        let _ = std::fs::remove_file(&result_path);
        return Ok(());
    };

    let text = result.render_telegram_message();
    push_to_bridge(svc, chat_id, &text).await;

    if let Err(e) = std::fs::remove_file(&result_path) {
        log::warn!(
            "[self-update] failed to delete {}: {}",
            result_path.display(),
            e
        );
    }
    Ok(())
}

/// Wait up to ~10s for the bridge sender to come online, then push
/// the message. The bridge starts asynchronously after `init_states`,
/// so on a cold boot we may briefly race it.
async fn push_to_bridge(svc: &ServiceState, chat_id: i64, text: &str) {
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let tx = { svc.bridge_outbound.lock().clone() };
        if let Some(tx) = tx {
            let msg = OutboundMessage {
                user_id: Some(chat_id),
                content: OutboundContent::Text { text: text.to_string() },
                append: false,
                buttons: None,
            };
            if let Err(e) = tx.send(msg) {
                log::warn!("[self-update] bridge send failed: {}", e);
            } else {
                log::info!("[self-update] result message queued for chat {}", chat_id);
            }
            return;
        }
        if std::time::Instant::now() >= deadline {
            log::warn!("[self-update] bridge never came online; dropping result notification");
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::super::result::{UpgradeResult, UpgradeStatus};

    fn sample(status: UpgradeStatus) -> UpgradeResult {
        UpgradeResult {
            status,
            old_version: "1.1.2".into(),
            new_version: "1.1.3".into(),
            observed_version: match status {
                UpgradeStatus::Success => "1.1.3".into(),
                UpgradeStatus::RolledBack => "1.1.2".into(),
                UpgradeStatus::RollbackFailed => String::new(),
            },
            channel: "stable".into(),
            log_path: "/tmp/up.log".into(),
            started_at: "2026-05-02T10:00:00Z".into(),
            finished_at: "2026-05-02T10:00:42Z".into(),
            error: match status {
                UpgradeStatus::Success => String::new(),
                UpgradeStatus::RolledBack => "wrong version on /api/health".into(),
                UpgradeStatus::RollbackFailed => "bak missing".into(),
            },
            notify_chat_id: Some(99),
        }
    }

    #[test]
    fn message_for_success_says_done() {
        let m = sample(UpgradeStatus::Success).render_telegram_message();
        assert!(m.contains("DONE"), "msg: {}", m);
        assert!(m.contains("v1.1.2"));
        assert!(m.contains("v1.1.3"));
    }

    #[test]
    fn message_for_rolled_back_includes_reason() {
        let m = sample(UpgradeStatus::RolledBack).render_telegram_message();
        assert!(m.contains("ROLLED BACK"));
        assert!(m.contains("reason: wrong version on /api/health"));
    }

    #[test]
    fn message_for_rollback_failed_warns_manual_recovery() {
        let m = sample(UpgradeStatus::RollbackFailed).render_telegram_message();
        assert!(m.contains("ROLLBACK FAILED"));
        assert!(m.contains("manual recovery"));
    }
}
