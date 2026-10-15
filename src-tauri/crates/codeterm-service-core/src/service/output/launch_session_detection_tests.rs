use super::trust_launch_detection::{
    heuristic_session_fallback_allowed, launch_session_detection_due,
};

#[test]
fn hooked_launch_record_detection_preserves_trust_and_deadline_gates() {
    assert!(launch_session_detection_due(false, true, true, false, true));
    assert!(launch_session_detection_due(false, true, true, true, false));
    assert!(!launch_session_detection_due(
        false, true, true, false, false
    ));
    assert!(!launch_session_detection_due(true, true, true, false, true));
    assert!(!launch_session_detection_due(
        false, false, false, false, true
    ));
}

#[test]
fn unhooked_exact_detection_precedes_heuristics_without_waiting_for_trust() {
    assert!(launch_session_detection_due(
        true, false, true, false, false
    ));
    assert!(launch_session_detection_due(true, false, true, true, false));
    assert!(!launch_session_detection_due(
        true, false, false, false, false
    ));
}

#[test]
fn null_exact_result_allows_heuristics_only_after_binding_deadline() {
    assert!(!heuristic_session_fallback_allowed(true, true, false));
    assert!(heuristic_session_fallback_allowed(true, true, true));
    assert!(heuristic_session_fallback_allowed(true, false, false));
    assert!(!heuristic_session_fallback_allowed(false, true, true));
}
