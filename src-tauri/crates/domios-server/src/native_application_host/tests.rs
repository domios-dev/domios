use super::*;

#[tokio::test]
async fn missing_native_attachment_refuses_before_registering_continuation() {
    let (mux, _) = codeterm_service_core::multiplexer::state::MultiplexerState::new();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    let update_state = Arc::new(parking_lot::Mutex::new(codeterm_update::UpdateState {
        install_method: codeterm_update::InstallMethod::Nsis,
        last_check: None,
        last_check_time: None,
        channel: Default::default(),
        phase: UpdatePhase::Idle,
        last_error: None,
        phase_changed_at: None,
        downloaded_bytes: 0,
        download_total_bytes: None,
    }));
    let host = Arc::new(NativeApplicationHost {
        native: Arc::new(RwLock::new(None)),
        service: Arc::new(fixture.service.clone()),
        update_state,
    });
    assert!(!host.available());
    let result = host
        .check_and_apply(ApplyOptions {
            channel: Default::default(),
            manifest_url: None,
            install_method: codeterm_update::InstallMethod::Nsis,
            initiating_tab_id: Some("missing-caller".into()),
        })
        .await;
    assert_eq!(result, Err("native_capability_unavailable".into()));
    assert!(fixture
        .service
        .register_host_extension(Arc::new(
            codeterm_commands::application_host::ApplicationHostState(host)
        ))
        .is_ok());
    assert!(codeterm_commands::application_host::require(&fixture.service).is_err());
}

#[test]
fn persisted_relaunch_is_a_handoff_without_observing_the_dying_connection() {
    assert_eq!(
        terminal_outcome(&Phase::Relaunching),
        Some(Ok(ApplyResult::Restarting))
    );
    assert_eq!(terminal_outcome(&Phase::Applying), None);
    assert_eq!(terminal_outcome(&Phase::Admitted), None);
}
