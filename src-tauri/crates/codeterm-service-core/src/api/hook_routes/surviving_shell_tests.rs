use std::sync::Arc;

use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;

const PROVIDER: &str = "surviving-shell-provider";
const SESSION: &str = "surviving-shell-session";
const EXITED_NONCE: &str = "surviving-shell-exited-nonce";

fn exited_shell_tab() -> (MuxState, String, AgentRegistry) {
    let (mut state, tab_id) = MultiplexerState::new();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    tab.cwd = Some("/tmp/surviving-shell".into());
    tab.pty_id = Some("surviving-shell-pty".into());
    tab.agent_type = Some(PROVIDER.into());
    tab.agent_session_id = Some(SESSION.into());
    tab.session_binding_nonce = Some(EXITED_NONCE.into());
    tab.reported_session_binding_nonce = Some(EXITED_NONCE.into());
    assert!(crate::service::tab::clear_agent_identity_for_lifecycle_exit(tab));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(MockProvider::with_session(PROVIDER, SESSION)));
    (
        Arc::new(TimedRwLock::new("mux-surviving-shell", state)),
        tab_id,
        registry,
    )
}

#[test]
fn manual_resume_in_shell_is_adopted() {
    let (mux, tab_id, registry) = exited_shell_tab();

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        PROVIDER,
        SESSION,
        Some(EXITED_NONCE),
    )
    .unwrap();

    assert!(claim.nonce_verified);
    assert!(claim.changed);
    let mut state = mux.write();
    let tab = state.get_tab_mut(&tab_id).unwrap();
    assert_eq!(tab.session_binding_nonce.as_deref(), Some(EXITED_NONCE));
    assert_eq!(
        tab.reported_session_binding_nonce.as_deref(),
        Some(EXITED_NONCE)
    );
    tab.agent_type = Some(PROVIDER.into());
    assert!(tab.launch_session_authenticated());
    assert!(crate::service::boot_stage::binding_observes_in_tab_launch(
        tab
    ));
}

#[test]
fn surviving_shell_adoption_needs_the_newest_retired_nonce_and_no_owned_launch() {
    let (mux, tab_id, registry) = exited_shell_tab();
    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        PROVIDER,
        SESSION,
        Some("surviving-shell-foreign-nonce"),
    )
    .unwrap();
    assert!(!claim.nonce_verified);
    assert_eq!(
        mux.read().get_tab(&tab_id).unwrap().session_binding_nonce,
        None
    );

    let (mux, tab_id, registry) = exited_shell_tab();
    mux.write().get_tab_mut(&tab_id).unwrap().agent_booting = true;
    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        PROVIDER,
        SESSION,
        Some(EXITED_NONCE),
    )
    .unwrap();
    assert!(!claim.nonce_verified);
    assert_eq!(
        mux.read().get_tab(&tab_id).unwrap().session_binding_nonce,
        None
    );
}
