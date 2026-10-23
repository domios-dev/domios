use serde_json::Value;
use std::ffi::OsString;
use std::process::Command;
use std::sync::OnceLock;

pub const SESSION_LIMIT: f64 = 85.0;
static CLI_ENV: OnceLock<Vec<(OsString, OsString)>> = OnceLock::new();

pub fn stash_cli_env() {
    let _ = CLI_ENV.set(
        std::env::vars_os()
            .filter(|(key, _)| key.to_string_lossy().starts_with("CODETERM_"))
            .collect(),
    );
}

#[derive(Clone, Debug)]
pub struct Paused {
    pub reason: String,
}

pub fn gate() -> Result<(), Paused> {
    let env = CLI_ENV.get().cloned().unwrap_or_default();
    let cli = env
        .iter()
        .find(|(key, _)| key == "CODETERM_CLI_PATH")
        .map(|(_, value)| value.clone())
        .unwrap_or_else(|| "codeterm".into());
    let output = Command::new(cli)
        .args(["usage", "latest", "--json"])
        .envs(env)
        .output()
        .map_err(|error| Paused {
            reason: format!("usage unreadable: {error}"),
        })?;
    let usage: Value = serde_json::from_slice(&output.stdout).map_err(|error| Paused {
        reason: format!("usage unreadable: {error}"),
    })?;
    let session = usage["session_pct"].as_f64().ok_or(Paused {
        reason: "usage has no session_pct".into(),
    })?;
    if session >= SESSION_LIMIT {
        let resets = usage["session_resets_at"]
            .as_i64()
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|at| at.to_rfc3339())
            .unwrap_or_else(|| "unknown".into());
        return Err(Paused {
            reason: format!("session quota {session}% >= {SESSION_LIMIT}%; resets {resets}"),
        });
    }
    Ok(())
}
