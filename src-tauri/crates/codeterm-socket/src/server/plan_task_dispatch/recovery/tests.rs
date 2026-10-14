use super::startup_recovery_event;

#[test]
fn startup_recovery_targets_the_attempt_and_reserves_hold_for_current_evidence() {
    let event = startup_recovery_event(
        "plan-recovery",
        "task-recovery",
        "worker-tab",
        "task-attempt",
        Some("pty-attempt"),
        "agent_ready",
    );

    assert_eq!(event["kind"], "worker_startup_recovery_needed");
    assert_eq!(event["action_needed"], true);
    assert_eq!(event["plan_id"], "plan-recovery");
    assert_eq!(event["task_id"], "task-recovery");
    assert_eq!(event["tab_id"], "worker-tab");
    assert_eq!(event["attempt_id"], "task-attempt");
    assert_eq!(event["startup_stage"], "agent_ready");
    assert_eq!(event["startup_attempt_id"], "pty-attempt");
    assert_eq!(
        event["startup_hold"]["command"],
        "codeterm agent startup-hold --tab worker-tab --attempt pty-attempt"
    );
    assert_eq!(
        event["startup_hold"]["requires_prompt_bound_evidence"],
        true
    );
    assert_eq!(event["task_prose_to_worker"], false);
    assert_eq!(event["ack_reminder_to_worker"], false);
    assert_eq!(event["raw_keys"], false);
    assert_eq!(event["auto_accept"], false);
    assert_eq!(event["unknown_prompt"]["state"], "blocked");
    assert_eq!(event["unknown_prompt"]["action_needed"], true);
    assert_eq!(event["unknown_prompt"]["respond"], false);
}

#[test]
fn startup_recovery_without_exact_pty_attempt_never_offers_a_hold_command() {
    let event = startup_recovery_event("plan", "task", "tab", "attempt", None, "session_binding");

    assert_eq!(event["action_needed"], true);
    assert!(event.get("startup_hold").is_none());
    assert!(event.get("startup_attempt_id").is_none());
    assert_eq!(event["raw_keys"], false);
    assert_eq!(event["auto_accept"], false);
}
