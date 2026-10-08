use super::trust_launch_detection::{
    should_detect_unbound_session, should_heuristically_detect_session,
};
use super::{
    decide_agent_output_lifecycle, screen_ends_in_shell_prompt, AgentOutputLifecycleDecision,
};

#[test]
fn shell_prompt_recognized_only_on_bare_prompt_lines() {
    assert!(screen_ends_in_shell_prompt("user@host ~ %"));
    assert!(screen_ends_in_shell_prompt("C:\\Users\\me>"));
    assert!(screen_ends_in_shell_prompt("$"));
    assert!(screen_ends_in_shell_prompt("output\n# "));
    // A running agent TUI footer is not a shell prompt.
    assert!(!screen_ends_in_shell_prompt(
        "auto mode on (shift+tab to cycle)"
    ));
    assert!(!screen_ends_in_shell_prompt(""));
}

#[test]
fn shell_prompt_without_markers_is_diagnostic_only() {
    assert_eq!(
        decide_agent_output_lifecycle("PS D:\\Developer\\codeterm>", false),
        AgentOutputLifecycleDecision::PreserveIdentityAndLogShellLikeFrame
    );
    assert_eq!(
        decide_agent_output_lifecycle("PS D:\\Developer\\codeterm>", true),
        AgentOutputLifecycleDecision::PreserveIdentity
    );
}

#[test]
fn session_detection_never_replaces_a_bound_session() {
    assert!(should_detect_unbound_session(None));
    assert!(!should_detect_unbound_session(Some("session-owned-by-tab")));
}

#[test]
fn structured_sessions_require_authoritative_provider_binding() {
    assert!(!should_heuristically_detect_session(true, true, true));
    assert!(!should_heuristically_detect_session(true, false, true));
    assert!(should_heuristically_detect_session(true, false, false));
}

#[test]
fn restored_session_is_reauthenticated_by_exact_launch_detection() {
    use super::trust_launch_detection::session_detection_allowed;
    assert!(session_detection_allowed(
        Some("saved-own-session"),
        false,
        true
    ));
    assert!(!session_detection_allowed(
        Some("saved-own-session"),
        true,
        true
    ));
    assert!(!session_detection_allowed(
        Some("saved-own-session"),
        false,
        false
    ));
    assert!(session_detection_allowed(None, false, false));
}
