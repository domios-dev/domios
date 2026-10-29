use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn restored_shell_gets_current_credential_after_server_restart_and_nonce_rotation() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.session_binding_nonce = Some("saved-launch".into());
    tab.reported_session_binding_nonce = Some("saved-launch".into());
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    let previous = ServiceFixture::new(mux);
    let inherited = previous.service.frontend_pty_env(Some(&tab_id));
    let old_token = inherited["CODETERM_API_TOKEN"].clone();
    let snapshot = previous.service.mux.read().to_session_snapshot();
    drop(previous);
    let mux = MultiplexerState::from_session_snapshot(&snapshot);
    assert!(mux.get_tab(&tab_id).unwrap().restored_from_snapshot);
    assert_eq!(mux.get_tab(&tab_id).unwrap().agent_session_id.as_deref(), Some("saved-session"));
    let restarted = ServiceFixture::new(mux);
    assert!(restarted.service.identify_tab_credential(&old_token).is_none());
    let shell_env = restarted.service.frontend_pty_env(Some(&tab_id));
    let nonce = restarted.service.reserve_agent_launch_binding_nonce(&tab_id, Some("saved-session"));
    assert!(restarted.service.identify_tab_credential(&shell_env["CODETERM_API_TOKEN"]).is_none());
    let wrapped = wrap_command(&restarted.service, &tab_id, "opaque-provider", &nonce, "attempt", false).unwrap();
    let path = std::fs::read_dir(restarted.service.ensure_agent_pad(&tab_id).unwrap().path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with(".launch-credential-"))
        .unwrap();
    let current = std::fs::read_to_string(&path).unwrap();
    assert!(!wrapped.contains(&current));
    assert!(!wrapped.contains(&old_token));
    assert!(wrapped.contains(&nonce));
    let caller = restarted.service.identify_tab_credential(&current).unwrap();
    assert_eq!(caller.tab_id, tab_id);
    assert_eq!(caller.binding_nonce, nonce);
    for windows in [false, true] {
        let command = with_credential_file("opaque-provider", &path, windows);
        assert!(!command.contains(&current));
        assert!(command.contains("CODETERM_API_TOKEN"));
        assert!(command.contains("opaque-provider"));
    }
}
