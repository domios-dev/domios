use super::*;

#[test]
fn unavailable_capture_keeps_decision_point_without_correlation() {
    let event = json!({"decision_point":"launch_ready", "observation_status":"unavailable", "phase":"classification_rejected"});
    let mut diagnostics = json!({});
    project_diagnostics(&event, &mut diagnostics);
    assert_eq!(diagnostics["decision_point"], "launch_ready");
    assert_eq!(diagnostics["observation_status"], "unavailable");
    assert_eq!(diagnostics["phase"], "classification_rejected");
    assert!(diagnostics.get("correlation").is_none());
}

#[test]
fn projection_rejects_untrusted_scope_and_model_verdict() {
    let mut event = json!({"observation_id":42, "decision_point":"prompt_candidate", "observation_status":"completed", "phase":"action_authorized", "correlation": {
        "version":2, "decision_point":"prompt_candidate", "producer":"prompt_confirmation", "parent_observation_id":41, "observation_id":42,
        "launch_id":"a".repeat(64), "session_id":"b".repeat(64), "occurrence_id":"c".repeat(64), "occurrence":1, "frame_hash":"d".repeat(16), "candidate_evidence":{"status":"unavailable"}
    }, "model_verdict":{"status":"observed", "label":"other", "confidence":0.95}});
    assert!(project(&event).is_some());
    event["correlation"]["parent_observation_id"] = json!(42);
    event["model_verdict"]["confidence"] = json!(1.5);
    let mut diagnostics = json!({});
    project_diagnostics(&event, &mut diagnostics);
    assert!(diagnostics.get("correlation").is_none());
    assert!(diagnostics.get("phase").is_none());
    assert_eq!(diagnostics["observation_status"], "unavailable");
    assert_eq!(
        diagnostics["model_verdict"],
        json!({"status":"unavailable"})
    );
}
