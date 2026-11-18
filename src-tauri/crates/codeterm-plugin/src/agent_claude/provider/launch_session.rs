//! Claude keeps one registry record per live process, keyed by pid. A record
//! binds this tab only when its pid is in the tab's own process tree and its
//! recorded process start equals the live process's start.

use std::{io::Read, path::PathBuf, time::UNIX_EPOCH};

use crate::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};

pub(crate) const PROCESS_REGISTRY_DIR_ENV: &str = "CODETERM_CLAUDE_PROCESS_REGISTRY_DIR";

fn registry_dir() -> Option<PathBuf> {
    match crate::provider_account::environment(PROCESS_REGISTRY_DIR_ENV) {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => super::read_home::config_root().map(|home| home.join("sessions")),
    }
}

pub(super) fn detect_launch_session(
    evidence: &LaunchSessionEvidence,
) -> Option<LaunchSessionDetection> {
    let Some(dir) = registry_dir() else {
        log::debug!("[claude-pid-registry] refused: registry_directory_unavailable");
        return None;
    };
    if evidence.processes.is_empty() {
        log::debug!("[claude-pid-registry] refused: process_tree_empty");
        return None;
    }
    let mut processes: Vec<_> = evidence.processes.iter().collect();
    processes.sort_by_key(|process| (process.depth, process.pid));
    processes
        .into_iter()
        .find_map(|process| {
            let pid = process.pid;
            match read_registry_session(&dir, process) {
                Ok(session) => Some(session),
                Err(reason) => {
                    log::debug!("[claude-pid-registry] pid={pid} refused: {reason}");
                    None
                }
            }
        })
        .map(|session_id| LaunchSessionDetection {
            session_id,
            source: LaunchSessionSource::PidRegistry,
        })
}

fn read_registry_session(
    dir: &std::path::Path,
    process: &crate::provider::LaunchProcess,
) -> Result<String, String> {
    let mut file = crate::provider_account::open_read(dir.join(format!("{}.json", process.pid)))
        .map_err(|error| format!("record_open: {}", error.kind()))?;
    let modified_ms = cfg!(target_os = "macos")
        .then(|| file.metadata().ok()?.modified().ok())
        .flatten()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok());
    let mut raw = String::new();
    file.read_to_string(&mut raw)
        .map_err(|error| format!("record_read: {}", error.kind()))?;
    validate_registry_session(&raw, process.pid, &process.start_token, modified_ms)
        .map_err(str::to_owned)
}

#[cfg(test)]
fn registry_session_id(
    raw: &str,
    pid: u32,
    start_token: &str,
    modified_ms: Option<i64>,
) -> Option<String> {
    validate_registry_session(raw, pid, start_token, modified_ms).ok()
}

fn validate_registry_session(
    raw: &str,
    pid: u32,
    start_token: &str,
    modified_ms: Option<i64>,
) -> Result<String, &'static str> {
    let record: serde_json::Value = serde_json::from_str(raw).map_err(|_| "invalid_json")?;
    if record.get("pid").and_then(serde_json::Value::as_u64) != Some(u64::from(pid)) {
        return Err("pid_mismatch");
    }
    // A child Claude run by the agent itself is not this tab's interactive session.
    if record
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind != "interactive")
    {
        return Err("non_interactive");
    }
    let recorded_start = record
        .get("procStart")
        .and_then(serde_json::Value::as_str)
        .ok_or("missing_recorded_start")?
        .trim();
    validate_registry_start(recorded_start, start_token, modified_ms)?;
    record
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|session| !session.is_empty())
        .map(str::to_owned)
        .ok_or("missing_session_id")
}

fn validate_registry_start(
    recorded: &str,
    token: &str,
    modified_ms: Option<i64>,
) -> Result<(), &'static str> {
    if token.is_empty() {
        return Err("missing_live_start");
    }
    if recorded == token {
        return Ok(());
    }
    let micros = token
        .parse::<i64>()
        .ok()
        .filter(|_| token.bytes().all(|byte| byte.is_ascii_digit()))
        .ok_or("invalid_live_start")?;
    let start = chrono::DateTime::from_timestamp_micros(micros).ok_or("invalid_live_start")?;
    if recorded != start.format("%a %b %e %T %Y").to_string() {
        return Err("start_mismatch");
    }
    // A legacy local timestamp can coincide with the UTC second of a reused PID.
    let modified_ms = modified_ms.ok_or("missing_record_mtime")?;
    if modified_ms <= micros / 1_000 {
        return Err("stale_record");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
