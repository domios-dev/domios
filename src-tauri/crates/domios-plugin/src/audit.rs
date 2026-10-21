use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::json;

pub fn log_path(root: &Path) -> PathBuf {
    root.join("logs").join("plugin-events.log")
}

/// The audit log is append-only, so without a bound it grows for the life of the
/// install. One machine reached 74 MB and 570k lines before anyone looked.
const MAX_AUDIT_BYTES: u64 = 8 * 1024 * 1024;

/// Roll the log over once it passes its cap, keeping exactly one previous
/// generation. Rotation failure is never fatal: losing an audit line is better
/// than failing the operation the line describes.
fn rotate_if_oversized(path: &Path) {
    let oversized = fs::metadata(path).is_ok_and(|meta| meta.len() > MAX_AUDIT_BYTES);
    if !oversized {
        return;
    }
    let previous = path.with_extension("log.1");
    let _ = fs::remove_file(&previous);
    if let Err(error) = fs::rename(path, &previous) {
        log::warn!(
            "[plugin/audit] rotation failed for {}: {error}",
            path.display()
        );
    }
}

pub fn append_event(root: &Path, event: &serde_json::Value) -> Result<(), String> {
    let path = log_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    rotate_if_oversized(&path);
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
    if let Err(e) = append_event(&codeterm_paths::data_dir(), &event) {
        log::warn!("[plugin/audit] daemon-reloaded audit failed for {plugin}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_oversized_audit_log_rotates_instead_of_growing_without_bound() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let path = log_path(root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, vec![b'x'; (MAX_AUDIT_BYTES + 1) as usize]).unwrap();

        append_event(root, &json!({"kind": "test"})).unwrap();

        let rotated = path.with_extension("log.1");
        assert!(
            rotated.exists(),
            "the oversized generation must be kept as .1"
        );
        let live = fs::metadata(&path).unwrap().len();
        assert!(
            live < MAX_AUDIT_BYTES,
            "the live log must restart small after rotation, got {live} bytes"
        );
    }

    #[test]
    fn a_log_under_the_cap_is_never_rotated() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        append_event(root, &json!({"kind": "first"})).unwrap();
        append_event(root, &json!({"kind": "second"})).unwrap();

        let path = log_path(root);
        assert!(
            !path.with_extension("log.1").exists(),
            "a small log must not rotate"
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap().lines().count(),
            2,
            "both events stay in the live log"
        );
    }

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
