use super::*;

#[test]
fn live_lazy_composer_supersedes_clean_wrapper_exit_on_the_same_launch() {
    use crate::service::launch_readiness::{
        LaunchAttempt, LaunchEvidence, LaunchExitProof, LaunchObservation, LaunchSettlement, arm,
        note_first_turn_input_ready, observe, settle_evidence, suspect_exit,
    };
    use crate::service::process_tree::{
        ProviderProcessTreeSnapshot, ProviderTreeKey, ProviderTreeWitnesses,
    };

    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("lazy-wrapper-exit-pty".into());
    tab.session_binding_nonce = Some("lazy-wrapper-exit-nonce".into());
    let unbound_tab = tab.clone();
    let attempt = LaunchAttempt {
        tab_id,
        pty_id: "lazy-wrapper-exit-pty".into(),
        binding_nonce: "lazy-wrapper-exit-nonce".into(),
    };
    let key = ProviderTreeKey {
        tab_id: attempt.tab_id.clone(),
        pty_id: attempt.pty_id.clone(),
        binding_nonce: Some(attempt.binding_nonce.clone()),
        executable_name: "fixture-agent".into(),
    };
    let absent =
        ProviderTreeWitnesses::default().record_for_test(&key, ProviderProcessTreeSnapshot::Absent);
    let clean_wrapper_exit =
        LaunchExitProof::from_finished_marker(&attempt, &absent, Some(0), true, None)
            .expect("the shell Finished marker reports a clean wrapper exit");
    assert!(clean_wrapper_exit.is_deferred());

    assert_eq!(
        decide_launch_input(
            LaunchInputObservation {
                provider_process_alive: false,
                ..input_observation()
            },
            LaunchInputMode::LazySessionStart,
        ),
        Ok(Some(LaunchInputEvidence::ProviderComposer)),
        "the declared structural composer is live in this attempt's own PTY",
    );

    arm(attempt.clone());
    assert!(suspect_exit(
        &attempt.tab_id,
        Some(&attempt.pty_id),
        Some(0),
        &clean_wrapper_exit,
    ));
    assert!(note_first_turn_input_ready(
        &attempt.tab_id,
        &attempt.pty_id,
        &attempt.binding_nonce,
    ));
    assert_eq!(
        observe(&attempt, Instant::now()),
        LaunchObservation::Evidence(LaunchEvidence::FirstTurnInputReady),
        "the structural composer clears only this attempt's deferred shell exit",
    );
    assert_eq!(
        settle_evidence(&unbound_tab, &attempt, LaunchEvidence::FirstTurnInputReady),
        LaunchSettlement::AwaitingFirstTurn,
        "composer readiness admits input without authenticating a session",
    );
}
