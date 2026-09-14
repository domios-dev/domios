//! Claude keeps one registry record per live process, keyed by pid. A record
//! binds this tab only when its pid is in the tab's own process tree and its
//! recorded process start equals the live process's start.

use std::path::PathBuf;

use crate::provider::{LaunchSessionDetection, LaunchSessionEvidence, LaunchSessionSource};

pub(crate) const PROCESS_REGISTRY_DIR_ENV: &str = "CODETERM_CLAUDE_PROCESS_REGISTRY_DIR";

fn registry_dir() -> Option<PathBuf> {
    match std::env::var_os(PROCESS_REGISTRY_DIR_ENV) {
        Some(dir) if !dir.is_empty() => Some(PathBuf::from(dir)),
        _ => dirs::home_dir().map(|home| home.join(".claude").join("sessions")),
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
            let raw = std::fs::read_to_string(dir.join(format!("{}.json", process.pid))).ok()?;
            registry_session_id(&raw, process.pid, &process.start_token)
        })
        .map(|session_id| LaunchSessionDetection {
            session_id,
            source: LaunchSessionSource::PidRegistry,
        })
}

fn registry_session_id(raw: &str, pid: u32, start_token: &str) -> Option<String> {
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
    if start_token.is_empty() || recorded_start != start_token {
        return None;
    }
    let session_id = record.get("sessionId")?.as_str()?.trim();
    (!session_id.is_empty()).then(|| session_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32, proc_start: &str, kind: &str) -> String {
        serde_json::json!({
            "pid": pid,
            "sessionId": "dba70c68-0000-4000-8000-000000000001",
            "procStart": proc_start,
            "kind": kind,
        })
        .to_string()
    }

    const START: &str = "134338511491237705";

    #[test]
    fn record_with_matching_process_start_names_its_session() {
        let raw = record(45044, START, "interactive");
        assert_eq!(
            registry_session_id(&raw, 45044, START).as_deref(),
            Some("dba70c68-0000-4000-8000-000000000001")
        );
    }

    #[test]
    fn reused_pid_with_a_different_process_start_names_nothing() {
        let raw = record(45044, START, "interactive");
        assert_eq!(registry_session_id(&raw, 45044, "134338511491230000"), None);
    }

    #[test]
    fn record_for_another_pid_names_nothing() {
        let raw = record(48208, START, "interactive");
        assert_eq!(registry_session_id(&raw, 45044, START), None);
    }

    #[test]
    fn non_interactive_record_names_nothing() {
        let raw = record(45044, START, "print");
        assert_eq!(registry_session_id(&raw, 45044, START), None);
    }

    #[test]
    fn unknown_process_start_names_nothing() {
        let raw = record(45044, START, "interactive");
        assert_eq!(registry_session_id(&raw, 45044, ""), None);
    }

    #[test]
    fn numeric_process_start_is_not_a_start_token() {
        let raw = serde_json::json!({"pid": 45044, "sessionId": "sid", "procStart": 7}).to_string();
        assert_eq!(registry_session_id(&raw, 45044, "7"), None);
    }
}
