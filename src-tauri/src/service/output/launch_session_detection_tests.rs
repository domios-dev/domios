use super::trust_launch_detection::launch_session_detection_due;

#[test]
fn hooked_launch_record_detection_starts_when_folder_trust_clears() {
    assert!(launch_session_detection_due(false, true, false, true));
}

#[test]
fn launch_record_detection_keeps_deadline_fallback_and_provider_gates() {
    assert!(launch_session_detection_due(false, true, true, false));
    assert!(!launch_session_detection_due(true, true, false, true));
    assert!(!launch_session_detection_due(false, false, false, true));
}
