use super::{reject_non_shell_restart, SHELL_RESTART_AGENT_MEMBER};

#[test]
fn shell_restart_rejects_agent_and_chat_backend_tabs() {
    assert!(reject_non_shell_restart(Some("opaque-provider"), false, false).is_err());
    assert!(reject_non_shell_restart(None, false, true).is_err());
    assert!(reject_non_shell_restart(None, false, false).is_ok());
}

#[test]
fn shell_restart_refuses_exited_agent_member_tab() {
    let refusal = reject_non_shell_restart(None, true, false).unwrap_err();
    assert!(refusal.starts_with(SHELL_RESTART_AGENT_MEMBER));
}
