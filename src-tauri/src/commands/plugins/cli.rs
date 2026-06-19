use std::process::Command;

use serde_json::Value;

pub(super) fn run_plugin_cli(args: &[impl AsRef<str>]) -> Result<Value, String> {
    let data_dir = crate::paths::data_dir();
    let cli = cli_path(&data_dir);
    let output = Command::new(&cli)
        .args(args.iter().map(|a| a.as_ref()))
        .env("CODETERM_DATA_DIR", &data_dir)
        .env("CODETERM_VERSION", env!("CARGO_PKG_VERSION"))
        .output()
        .map_err(|e| format!("run {}: {e}", cli.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            format!("plugin command failed with status {}", output.status)
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim()).map_err(|e| format!("parse plugin CLI JSON: {e}"))
}

fn cli_path(data_dir: &std::path::Path) -> std::path::PathBuf {
    let bin = if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    };
    data_dir.join("bin").join(bin)
}

pub(super) fn read_audit_events(limit: usize) -> Vec<Value> {
    let path = crate::plugin::audit::log_path(&crate::paths::data_dir());
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut events: Vec<Value> = text
        .lines()
        .rev()
        .take(limit)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    events.reverse();
    events
}
