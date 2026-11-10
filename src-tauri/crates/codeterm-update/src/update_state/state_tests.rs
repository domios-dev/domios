use super::*;

thread_local! {
    static CAPTURED: std::cell::RefCell<Option<Vec<serde_json::Value>>> = const { std::cell::RefCell::new(None) };
}

pub(super) fn capture_status(status: &serde_json::Value) {
    CAPTURED.with_borrow_mut(|events| {
        if let Some(events) = events {
            events.push(status.clone());
        }
    });
}

#[test]
fn update_status_events_follow_state_transitions() {
    CAPTURED.set(Some(Vec::new()));
    let mut state = empty_state();
    state.set_phase(UpdatePhase::Downloading);
    state.record_download_chunk(25, Some(100));
    state.set_error("fixture-failure");
    let events = CAPTURED.take().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0]["phase"], "downloading");
    assert_eq!(events[1]["downloaded_bytes"], 25);
    assert_eq!(events[1]["download_progress"], 0.25);
    assert_eq!(events[2]["phase"], "error");
    assert_eq!(events[2]["last_error"], "fixture-failure");
}

fn empty_state() -> UpdateState {
    UpdateState {
        install_method: InstallMethod::Unknown,
        last_check: None,
        last_check_time: None,
        channel: Channel::Stable,
        phase: UpdatePhase::Idle,
        last_error: None,
        phase_changed_at: None,
        downloaded_bytes: 0,
        download_total_bytes: None,
    }
}

#[test]
fn set_error_marks_error_phase() {
    let mut s = empty_state();
    s.set_error("boom");
    assert_eq!(s.phase, UpdatePhase::Error);
    assert_eq!(s.last_error.as_deref(), Some("boom"));
    assert!(s.phase_changed_at.is_some());
}

#[test]
fn set_phase_updates_timestamp() {
    let mut s = empty_state();
    s.set_phase(UpdatePhase::Checking);
    assert_eq!(s.phase, UpdatePhase::Checking);
    assert!(s.phase_changed_at.is_some());
}

#[test]
fn clear_error_does_not_change_phase() {
    let mut s = empty_state();
    s.set_error("nope");
    s.clear_error();
    assert!(s.last_error.is_none());
    assert_eq!(s.phase, UpdatePhase::Error);
}

#[test]
fn phase_serializes_snake_case() {
    let v = serde_json::to_string(&UpdatePhase::Downloading).unwrap();
    assert_eq!(v, "\"downloading\"");
}

#[test]
fn download_progress_accumulates_and_caps_at_total() {
    let mut s = empty_state();
    s.set_phase(UpdatePhase::Downloading);
    s.record_download_chunk(40, Some(100));
    s.record_download_chunk(80, Some(100));
    assert_eq!(s.downloaded_bytes, 100);
    assert_eq!(s.download_total_bytes, Some(100));
}

#[test]
fn applying_marks_download_complete_when_total_is_known() {
    let mut s = empty_state();
    s.set_phase(UpdatePhase::Downloading);
    s.record_download_chunk(40, Some(100));
    s.set_phase(UpdatePhase::Applying);
    assert_eq!(s.downloaded_bytes, 100);
}

#[test]
fn runtime_status_reports_download_progress() {
    let mut s = empty_state();
    s.set_phase(UpdatePhase::Downloading);
    s.record_download_chunk(25, Some(100));

    let status = s.runtime_status();
    assert_eq!(status["phase"], "downloading");
    assert_eq!(status["downloaded_bytes"], 25);
    assert_eq!(status["download_total_bytes"], 100);
    assert_eq!(status["download_progress"], 0.25);
    assert_eq!(status.get("check_path"), Some(&serde_json::Value::Null));
    assert_eq!(status.get("last_error"), Some(&serde_json::Value::Null));
}

#[test]
fn retirement_uses_the_failure_revision_captured_before_other_checks_await() {
    let mut state = empty_state();
    state.set_error("same failure text");
    let first_at = std::time::Instant::now();
    state.phase_changed_at = Some(first_at);
    let first = state.error_revision().unwrap();
    state.set_error("same failure text");
    state.phase_changed_at = Some(first_at + std::time::Duration::from_secs(1));
    assert!(!state.retire_error(Some(&first)));
    assert_eq!(state.phase, UpdatePhase::Error);
    assert_eq!(state.last_error.as_deref(), Some("same failure text"));
    assert!(!state.retire_error(None));
    let current = state.error_revision().unwrap();
    assert!(state.retire_error(Some(&current)));
    assert_eq!(state.phase, UpdatePhase::Idle);
    assert!(state.last_error.is_none());
}
