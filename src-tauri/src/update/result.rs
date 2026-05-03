//! Contract between the orchestrator script and the daemon boot-pickup.
//!
//! The script (PowerShell on Windows, shell elsewhere) writes a single
//! JSON file at `data_dir/upgrade-result.json` describing what it did.
//! When the new daemon starts, it reads this file, sends a Telegram
//! notification through the bridge, and deletes the file.
//!
//! Both the writer (Rust code that GENERATES the script) and the reader
//! (Rust code that PARSES the result on boot) reference the same structs
//! here. Any schema change ripples through both sides automatically.

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpgradeStatus {
    /// New binary booted and reported the expected version on /api/health.
    Success,
    /// New binary failed verification; .bak was restored and is now running.
    RolledBack,
    /// Both the new binary AND the .bak failed; manual recovery needed.
    RollbackFailed,
}

impl UpgradeStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::RolledBack => "rolled_back",
            Self::RollbackFailed => "rollback_failed",
        }
    }
}

/// Optional context that the OLD daemon stuffs into the staging directory
/// so the script can hand it back to the NEW daemon (or use it itself,
/// e.g. for auto-rollback messages). Lives at
/// `data_dir/upgrade-staging/context.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpgradeContext {
    pub old_version: String,
    pub new_version: String,
    pub channel: String,
    /// Telegram chat id to notify after restart, if any. The script never
    /// touches Telegram itself — it just preserves this so the new daemon
    /// can route the result message.
    #[serde(default)]
    pub notify_chat_id: Option<i64>,
    /// Path to the orchestrator log file the script writes during the swap.
    pub log_path: String,
    /// Started in RFC3339 (UTC). Filled in by the script when it begins.
    #[serde(default)]
    pub started_at: Option<String>,
}

/// Final result the script writes at `data_dir/upgrade-result.json`.
/// The new daemon picks this up on boot, formats a Telegram message,
/// and deletes the file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UpgradeResult {
    pub status: UpgradeStatus,
    pub old_version: String,
    pub new_version: String,
    /// Version reported by the new binary's /api/health. Empty if the new
    /// binary never came up.
    #[serde(default)]
    pub observed_version: String,
    pub channel: String,
    pub log_path: String,
    pub started_at: String,
    pub finished_at: String,
    /// Free-form error / explanation. Empty on success.
    #[serde(default)]
    pub error: String,
    /// Telegram chat id that should receive the notification, copied
    /// from the staging context. The new daemon reads this — the script
    /// never touches Telegram.
    #[serde(default)]
    pub notify_chat_id: Option<i64>,
}

impl UpgradeResult {
    pub fn render_telegram_message(&self) -> String {
        let header = match self.status {
            UpgradeStatus::Success => format!(
                "<b>codeterm self-update DONE</b>\nv{} → v{}",
                self.old_version, self.new_version
            ),
            UpgradeStatus::RolledBack => format!(
                "<b>codeterm self-update ROLLED BACK</b>\nstayed on v{} (target was v{})",
                self.old_version, self.new_version
            ),
            UpgradeStatus::RollbackFailed => format!(
                "<b>codeterm self-update ROLLBACK FAILED</b>\ntarget v{}, manual recovery required",
                self.new_version
            ),
        };
        let mut body = header;
        if !self.observed_version.is_empty()
            && self.observed_version != self.new_version
            && self.status != UpgradeStatus::RollbackFailed
        {
            body.push_str(&format!("\nobserved: v{}", self.observed_version));
        }
        if !self.error.is_empty() {
            body.push_str(&format!("\nreason: {}", self.error));
        }
        body.push_str(&format!("\nlog: {}", self.log_path));
        body
    }
}

pub const UPGRADE_RESULT_FILENAME: &str = "upgrade-result.json";
pub const UPGRADE_CONTEXT_FILENAME: &str = "context.json";
pub const UPGRADE_STAGING_DIR: &str = "upgrade-staging";

/// Parse a result file from disk. Returns Ok(None) if the file isn't there
/// (the normal "no upgrade just happened" case).
pub fn read_result(path: &Path) -> Result<Option<UpgradeResult>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let body = std::fs::read_to_string(path)
        .map_err(|e| format!("read {}: {}", path.display(), e))?;
    let parsed: UpgradeResult = serde_json::from_str(&body)
        .map_err(|e| format!("parse upgrade-result.json: {}", e))?;
    Ok(Some(parsed))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_success() -> UpgradeResult {
        UpgradeResult {
            status: UpgradeStatus::Success,
            old_version: "1.1.2".into(),
            new_version: "1.1.3".into(),
            observed_version: "1.1.3".into(),
            channel: "stable".into(),
            log_path: r"C:\Users\u\.codeterm-dev\upgrade-1.1.2-to-1.1.3.log".into(),
            started_at: "2026-05-02T10:00:00Z".into(),
            finished_at: "2026-05-02T10:00:42Z".into(),
            error: String::new(),
            notify_chat_id: Some(12345),
        }
    }

    #[test]
    fn round_trip_success() {
        let r = sample_success();
        let json = serde_json::to_string(&r).unwrap();
        let back: UpgradeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn round_trip_rolled_back() {
        let mut r = sample_success();
        r.status = UpgradeStatus::RolledBack;
        r.observed_version = "1.1.4".into();
        r.error = "health check returned wrong version".into();
        let json = serde_json::to_string(&r).unwrap();
        let back: UpgradeResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn missing_optional_fields_default() {
        let json = r#"{
            "status": "success",
            "old_version": "1.0.0",
            "new_version": "1.0.1",
            "channel": "stable",
            "log_path": "/tmp/up.log",
            "started_at": "2026-05-02T10:00:00Z",
            "finished_at": "2026-05-02T10:00:10Z"
        }"#;
        let r: UpgradeResult = serde_json::from_str(json).unwrap();
        assert_eq!(r.observed_version, "");
        assert_eq!(r.error, "");
        assert!(r.notify_chat_id.is_none());
    }

    #[test]
    fn render_message_success() {
        let msg = sample_success().render_telegram_message();
        assert!(msg.contains("DONE"), "msg: {}", msg);
        assert!(msg.contains("v1.1.2"));
        assert!(msg.contains("v1.1.3"));
        assert!(msg.contains("log: "));
    }

    #[test]
    fn render_message_rolled_back_includes_reason() {
        let mut r = sample_success();
        r.status = UpgradeStatus::RolledBack;
        r.observed_version = "1.1.4".into();
        r.error = "wrong version on /api/health".into();
        let msg = r.render_telegram_message();
        assert!(msg.contains("ROLLED BACK"));
        assert!(msg.contains("v1.1.2"));
        assert!(msg.contains("reason: wrong version on /api/health"));
        assert!(msg.contains("observed: v1.1.4"));
    }

    #[test]
    fn render_message_rollback_failed() {
        let mut r = sample_success();
        r.status = UpgradeStatus::RollbackFailed;
        r.error = "bak missing".into();
        let msg = r.render_telegram_message();
        assert!(msg.contains("ROLLBACK FAILED"));
        assert!(msg.contains("manual recovery"));
    }

    #[test]
    fn read_result_missing_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("upgrade-result.json");
        let r = read_result(&p).unwrap();
        assert!(r.is_none());
    }

    #[test]
    fn read_result_parses_file() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("upgrade-result.json");
        std::fs::write(&p, serde_json::to_string(&sample_success()).unwrap()).unwrap();
        let r = read_result(&p).unwrap().unwrap();
        assert_eq!(r.status, UpgradeStatus::Success);
        assert_eq!(r.new_version, "1.1.3");
    }

    #[test]
    fn upgrade_context_round_trip() {
        let ctx = UpgradeContext {
            old_version: "1.1.2".into(),
            new_version: "1.1.3".into(),
            channel: "stable".into(),
            notify_chat_id: Some(7),
            log_path: "/tmp/x.log".into(),
            started_at: Some("2026-05-02T10:00:00Z".into()),
        };
        let json = serde_json::to_string(&ctx).unwrap();
        let back: UpgradeContext = serde_json::from_str(&json).unwrap();
        assert_eq!(ctx, back);
    }
}
