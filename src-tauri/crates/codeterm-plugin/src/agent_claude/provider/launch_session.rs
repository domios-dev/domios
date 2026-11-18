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
    let dir = registry_dir()?;
    let mut processes: Vec<_> = evidence.processes.iter().collect();
    processes.sort_by_key(|process| (process.depth, process.pid));
    processes
        .into_iter()
        .find_map(|process| {
            let mut file =
                crate::provider_account::open_read(dir.join(format!("{}.json", process.pid)))
                    .ok()?;
            let modified_ms = cfg!(target_os = "macos")
                .then(|| file.metadata().ok()?.modified().ok())
                .flatten()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|elapsed| i64::try_from(elapsed.as_millis()).ok());
            let mut raw = String::new();
            file.read_to_string(&mut raw).ok()?;
            registry_session_id(&raw, process.pid, &process.start_token, modified_ms)
        })
        .map(|session_id| LaunchSessionDetection {
            session_id,
            source: LaunchSessionSource::PidRegistry,
        })
}

fn registry_session_id(
    raw: &str,
    pid: u32,
    start_token: &str,
    modified_ms: Option<i64>,
) -> Option<String> {
    let record: serde_json::Value = serde_json::from_str(raw).ok()?;
    if record.get("pid")?.as_u64()? != u64::from(pid) {
        return None;
    }
    // A child Claude run by the agent itself is not this tab's interactive session.
    if record
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|kind| kind != "interactive")
    {
        return None;
    }
    let recorded_start = record.get("procStart")?.as_str()?.trim();
    if !registry_start_matches(recorded_start, start_token, modified_ms) {
        return None;
    }
    let session_id = record.get("sessionId")?.as_str()?.trim();
    (!session_id.is_empty()).then(|| session_id.to_string())
}

fn registry_start_matches(recorded: &str, token: &str, modified_ms: Option<i64>) -> bool {
    if token.is_empty() {
        return false;
    }
    if recorded == token {
        return true;
    }
    let Some(modified_ms) = modified_ms else {
        return false;
    };
    if !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    // A legacy local timestamp can coincide with the UTC second of a reused PID.
    token
        .parse::<i64>()
        .ok()
        .filter(|micros| modified_ms > micros / 1_000)
        .and_then(chrono::DateTime::from_timestamp_micros)
        .is_some_and(|start| recorded == start.format("%a %b %e %T %Y").to_string())
}

#[cfg(test)]
mod tests;
