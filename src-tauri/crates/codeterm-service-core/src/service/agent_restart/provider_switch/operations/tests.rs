use super::*;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn self_switch_is_owned_by_daemon_after_callers_identity_is_retired() {
    for general in [false, true] {
        let (mut mux, mut id) = crate::multiplexer::state::MultiplexerState::new();
        if general {
            crate::service::general_agent::ensure_general_workspace(&mut mux, false);
            id = mux.resolve_tab_id("general").unwrap();
        }
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("source-token".into());
        tab.session_binding_nonce = Some("nonce-token".into());
        let fixture = ServiceFixture::new(mux);
        let accepted = fixture
            .service
            .accept_provider_switch_with(
                &id,
                "target-token".into(),
                Some(&id),
                Some("nonce-token"),
                |svc, tab, target, own| {
                    assert!(own);
                    svc.mux
                        .write()
                        .get_tab_mut(tab)
                        .unwrap()
                        .session_binding_nonce = None;
                    Ok(ProviderSwitchOutcome {
                        tab_id: tab.into(),
                        provider: target.into(),
                        session_id: "new-token".into(),
                        previous_provider: "source-token".into(),
                        previous_session_id: Some("old-token".into()),
                        previous_history_path: "archive-token".into(),
                        binding_restored: true,
                        handoff_delivered: true,
                    })
                },
            )
            .unwrap();
        let operation = accepted.status.operation_id.clone();
        assert_eq!(
            fixture
                .service
                .provider_switch_status(&id, &operation, false)
                .unwrap()
                .phase,
            "pending"
        );
        assert_eq!(
            fixture
                .service
                .mux
                .read()
                .get_tab(&id)
                .unwrap()
                .session_binding_nonce
                .as_deref(),
            Some("nonce-token")
        );
        accepted.response_complete.send(()).unwrap();
        let status = fixture
            .service
            .provider_switch_status(&id, &operation, true)
            .unwrap();
        assert_eq!(status.phase, "completed");
        assert!(status.outcome.unwrap().handoff_delivered);
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .session_binding_nonce
            .is_none());
    }
}

#[test]
fn stale_self_identity_refuses_and_a_disconnected_response_does_not_cancel_accepted_work() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().session_binding_nonce = Some("own-token".into());
    let fixture = ServiceFixture::new(mux);
    assert!(fixture
        .service
        .accept_provider_switch(
            &id,
            ProviderSwitchRequest {
                provider: "target".into(),
                model: None,
                reasoning_effort: None,
                force: false
            },
            Some(&id),
            Some("stale-token")
        )
        .is_err());
    let accepted = fixture
        .service
        .accept_provider_switch_with(
            &id,
            "target".into(),
            Some(&id),
            Some("own-token"),
            |_, _, _, own| {
                assert!(own);
                Err("opaque-runtime-refusal".into())
            },
        )
        .unwrap();
    let operation = accepted.status.operation_id.clone();
    drop(accepted);
    let status = fixture
        .service
        .provider_switch_status(&id, &operation, true)
        .unwrap();
    assert_eq!(status.phase, "failed");
    assert_eq!(status.error.as_deref(), Some("opaque-runtime-refusal"));
    assert!(fixture
        .service
        .provider_switch_notice(&id)
        .unwrap()
        .content
        .contains("opaque-runtime-refusal"));
}

#[test]
fn slow_switch_preflight_leaves_runtime_and_mux_responsive() {
    use std::{sync::mpsc, time::Duration};
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&id).unwrap().agent_type = Some("source".into());
    let fixture = ServiceFixture::new(mux);
    let (entered, started) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let accepted = fixture
        .service
        .accept_provider_switch_with(&id, "target".into(), None, None, move |_, _, _, _| {
            entered.send(()).unwrap();
            resume
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| "fixture preflight was not released".to_string())?;
            Err("provider_switch_auth_timeout: fixture".into())
        })
        .unwrap();
    let operation = accepted.status.operation_id.clone();
    accepted.response_complete.send(()).unwrap();
    let entered_result = started.recv_timeout(Duration::from_secs(5));
    let (sent, received) = mpsc::channel();
    fixture.service.rt.spawn(async move {
        sent.send(()).unwrap();
    });
    let runtime_result = received.recv_timeout(Duration::from_secs(5));
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_type
        .clone();
    release.send(()).unwrap();
    let status = fixture
        .service
        .provider_switch_status(&id, &operation, true)
        .unwrap();
    entered_result.unwrap();
    runtime_result.unwrap();
    assert_eq!(current.as_deref(), Some("source"));
    assert_eq!(status.phase, "failed");
    assert!(status
        .error
        .unwrap()
        .starts_with("provider_switch_auth_timeout:"));
}
