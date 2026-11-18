//! Claude keeps one registry record per live process, keyed by pid. A record
//! binds this tab only when its pid is in the tab's own process tree and its
//! recorded process start equals the live process's start.

use std::path::PathBuf;

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
            let raw =
                crate::provider_account::read_to_string(dir.join(format!("{}.json", process.pid)))
                    .ok()?;
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
    if !registry_start_matches(recorded_start, start_token, cfg!(target_os = "macos")) {
        return None;
    }
    let session_id = record.get("sessionId")?.as_str()?.trim();
    (!session_id.is_empty()).then(|| session_id.to_string())
}

fn registry_start_matches(recorded: &str, token: &str, macos: bool) -> bool {
    if token.is_empty() {
        return false;
    }
    if recorded == token {
        return true;
    }
    if !macos || !token.bytes().all(|byte| byte.is_ascii_digit()) {
        return false;
    }
    // The launch pins C/UTC so ps lstart cannot repeat a DST fold.
    token
        .parse::<i64>()
        .ok()
        .and_then(chrono::DateTime::from_timestamp_micros)
        .is_some_and(|start| recorded == start.format("%a %b %e %T %Y").to_string())
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

    #[test]
    fn macos_registry_start_matches_the_utc_second_and_rejects_fold_and_localized_records() {
        use chrono::TimeZone;
        let start = chrono::Utc
            .with_ymd_and_hms(2026, 11, 1, 5, 30, 0)
            .single()
            .unwrap()
            .timestamp_micros()
            + 789;
        let recorded = "Sun Nov  1 05:30:00 2026";
        assert!(registry_start_matches(recorded, &start.to_string(), true));
        assert!(!registry_start_matches(
            recorded,
            &(start + 1_000_000).to_string(),
            true
        ));
        let later = (start + 3_600_000_000).to_string();
        assert!(registry_start_matches(
            "Sun Nov  1 06:30:00 2026",
            &later,
            true
        ));
        assert!(!registry_start_matches(recorded, &later, true));
        for token in [start.to_string(), later] {
            for unpinned in ["Sun Nov  1 01:30:00 2026", "dim.  1 nov. 01:30:00 2026"] {
                assert!(!registry_start_matches(unpinned, &token, true));
            }
        }
        assert!(!registry_start_matches(recorded, &start.to_string(), false));
    }
}
