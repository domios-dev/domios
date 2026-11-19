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

fn registry_start_matches(recorded: &str, token: &str, modified_ms: Option<i64>) -> bool {
    validate_registry_start(recorded, token, modified_ms).is_ok()
}

const START: &str = "134338511491237705";

#[test]
fn record_with_matching_process_start_names_its_session() {
    let raw = record(45044, START, "interactive");
    assert_eq!(
        registry_session_id(&raw, 45044, START, None).as_deref(),
        Some("dba70c68-0000-4000-8000-000000000001")
    );
}

#[test]
fn reused_pid_with_a_different_process_start_names_nothing() {
    let raw = record(45044, START, "interactive");
    assert_eq!(
        registry_session_id(&raw, 45044, "134338511491230000", None),
        None
    );
}

#[test]
fn record_for_another_pid_names_nothing() {
    let raw = record(48208, START, "interactive");
    assert_eq!(registry_session_id(&raw, 45044, START, None), None);
}

#[test]
fn non_interactive_record_names_nothing() {
    let raw = record(45044, START, "print");
    assert_eq!(registry_session_id(&raw, 45044, START, None), None);
}

#[test]
fn unknown_process_start_names_nothing() {
    let raw = record(45044, START, "interactive");
    assert_eq!(registry_session_id(&raw, 45044, "", None), None);
}

#[test]
fn numeric_process_start_is_not_a_start_token() {
    let raw = serde_json::json!({"pid": 45044, "sessionId": "sid", "procStart": 7}).to_string();
    assert_eq!(registry_session_id(&raw, 45044, "7", None), None);
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
    let later = start + 3_600_000_000;
    let modified_ms = Some(later / 1_000 + 1);
    assert!(registry_start_matches(
        recorded,
        &start.to_string(),
        modified_ms
    ));
    assert!(!registry_start_matches(
        recorded,
        &(start + 1_000_000).to_string(),
        modified_ms
    ));
    assert!(registry_start_matches(
        "Sun Nov  1 06:30:00 2026",
        &later.to_string(),
        modified_ms
    ));
    assert!(!registry_start_matches(
        recorded,
        &later.to_string(),
        modified_ms
    ));
    for token in [start.to_string(), later.to_string()] {
        for unpinned in ["Sun Nov  1 01:30:00 2026", "dim.  1 nov. 01:30:00 2026"] {
            assert!(!registry_start_matches(unpinned, &token, modified_ms));
        }
    }
    assert!(!registry_start_matches(recorded, &start.to_string(), None));
}

#[test]
fn macos_registry_rejects_a_stale_riga_timestamp_matching_the_reused_pid() {
    let start = chrono::DateTime::parse_from_rfc3339("2026-08-01T05:30:00.000789Z")
        .unwrap()
        .timestamp_micros();
    let recorded = "Sat Aug  1 05:30:00 2026";
    let riga_record_ms = start / 1_000 - 3 * 3_600_000 + 10;
    assert!(!registry_start_matches(
        recorded,
        &start.to_string(),
        Some(riga_record_ms)
    ));
    assert!(!registry_start_matches(
        recorded,
        &start.to_string(),
        Some(start / 1_000)
    ));
    assert!(registry_start_matches(
        recorded,
        &start.to_string(),
        Some(start / 1_000 + 1)
    ));
}
