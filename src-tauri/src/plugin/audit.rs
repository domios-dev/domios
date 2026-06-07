use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

pub fn log_path(root: &Path) -> PathBuf {
    root.join("logs").join("plugin-events.log")
}

pub fn append_event(root: &Path, event: &serde_json::Value) -> Result<(), String> {
    let path = log_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    let line = serde_json::to_string(event).map_err(|e| e.to_string())?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("open {}: {e}", path.display()))?;
    writeln!(file, "{line}").map_err(|e| format!("write {}: {e}", path.display()))
}

pub fn append_daemon_reloaded(plugin: &str, version: &str) {
    let event = json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "kind": "daemon-reloaded",
        "actor": "daemon",
        "ok": true,
        "plugin": plugin,
        "version": version,
    });
    if let Err(e) = append_event(&crate::paths::data_dir(), &event) {
        log::warn!("[plugin/audit] daemon-reloaded audit failed for {plugin}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_event_writes_jsonl_under_logs_plugin_events() {
        let root = tempfile::tempdir().unwrap();
        append_event(
            root.path(),
            &json!({
                "ts": "2026-06-06T12:00:00Z",
                "kind": "daemon-reloaded",
                "plugin": "demo",
                "version": "0.1.0"
            }),
        )
        .unwrap();

        let path = log_path(root.path());
        let body = std::fs::read_to_string(path).unwrap();
        let line: serde_json::Value = serde_json::from_str(body.trim()).unwrap();
        assert_eq!(line["kind"], "daemon-reloaded");
        assert_eq!(line["plugin"], "demo");
    }
}
