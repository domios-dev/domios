use super::*;

#[tokio::test]
async fn runtime_launch_attach_before_configuration_does_not_cache_provider_defaults() {
    let (mut mux, _root) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("unprepared-runtime".into());
    tab.session_binding_nonce = Some("unprepared-nonce".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let result = super::super::ensure(&fixture.service, &id).await;
    assert!(
        matches!(result, Err(RuntimeFailure::Refused(reason)) if reason == "Runtime launch configuration is not committed")
    );
    assert!(super::super::for_tab(&id).is_none());
    assert!(fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .agent_runtime_config
        .is_none());
}

#[test]
fn runtime_launch_substitution_refuses_model_and_effort_before_binding() {
    let selected = RuntimeSettings {
        model: Some("selected-model".into()),
        effort: Some("selected-effort".into()),
        ..Default::default()
    };
    let equal = |requested: &str, reported: &str| requested == reported;
    assert!(validate_loaded_settings(&selected, &selected, equal).is_ok());
    assert!(validate_loaded_settings(&RuntimeSettings::default(), &selected, equal).is_ok());
    assert!(validate_loaded_settings(&selected, &RuntimeSettings::default(), equal).is_ok());
    assert!(validate_loaded_settings(
        &selected,
        &RuntimeSettings {
            model: Some("catalogue-alias".into()),
            ..selected.clone()
        },
        |_, _| true
    )
    .is_ok());
    for reported in [
        RuntimeSettings {
            model: Some("substituted-model".into()),
            ..selected.clone()
        },
        RuntimeSettings {
            effort: Some("substituted-effort".into()),
            ..selected.clone()
        },
    ] {
        let failure = validate_loaded_settings(&selected, &reported, equal).unwrap_err();
        assert!(matches!(failure, RuntimeFailure::Refused(_)));
        let mut state = super::super::RuntimeState::unloaded(None);
        let epoch = state.begin();
        state.degrade(epoch, &failure);
        assert_eq!(state.snapshot().state, "degraded");
        assert!(state
            .snapshot()
            .reason
            .unwrap()
            .contains("no substitution reason supplied"));
    }
}

fn declared_model_settings() -> RuntimeSettings {
    serde_json::from_value(serde_json::json!({
        "model":"agent-model",
        "configOptions":[{"id":"engine","name":"Engine","category":"model","currentValue":"agent-model","options":[]}]
    })).unwrap()
}

#[test]
fn declared_model_mismatch_is_accepted_with_a_host_notice() {
    let requested = RuntimeSettings {
        model: Some("requested-model".into()),
        ..Default::default()
    };
    let reported = declared_model_settings();
    let notice = validate_loaded_settings(&requested, &reported, |a, b| a == b).unwrap();
    let mut state = super::super::RuntimeState::unloaded(None);
    state.finish_load_notice(notice);
    assert_eq!(
        state.snapshot().notice.as_deref(),
        Some("Requested model requested-model, agent reports agent-model")
    );
}

#[test]
fn model_option_without_category_still_refuses_substitution() {
    let requested = RuntimeSettings {
        model: Some("requested-model".into()),
        ..Default::default()
    };
    let mut reported = declared_model_settings();
    reported.config_options.as_mut().unwrap()[0].category = None;
    assert!(validate_loaded_settings(&requested, &reported, |a, b| a == b).is_err());
}

#[test]
fn model_not_matching_the_declared_current_value_still_refuses_substitution() {
    let requested = RuntimeSettings {
        model: Some("requested-model".into()),
        ..Default::default()
    };
    let mut reported = declared_model_settings();
    reported.model = Some("unconfirmed-model".into());
    assert!(validate_loaded_settings(&requested, &reported, |a, b| a == b).is_err());
}

#[test]
fn load_notice_preserves_the_agent_reason_and_clears_previous_epoch_messages() {
    let mut state = super::super::RuntimeState::unloaded(None);
    state.notice = Some("old notice".into());
    state.pending_load_notice = Some("Requested model was not applied: Agent refusal".into());
    state.finish_load_notice(Some(
        "Requested model requested, agent reports actual".into(),
    ));
    let notice = state.snapshot().notice.unwrap();
    assert!(notice.contains("Agent refusal"));
    assert!(notice.contains("agent reports actual"));
    assert!(!notice.contains("old notice"));
    assert!(state.pending_load_notice.is_none());
    state.finish_load_notice(None);
    assert!(state.snapshot().notice.is_none());
}

#[tokio::test]
async fn refused_unbound_start_retires_process_and_remains_restartable() {
    for unbound in [false, true] {
        let (fixture, tab, _peer, _cleanup) = super::super::tests::sync_send::runtime_fixture();
        let session = super::super::for_tab(&tab).unwrap();
        let process = session.process.lock().clone().unwrap();
        session.context.lock().thread_id = None;
        if unbound {
            session.state.lock().thread_id = None;
        }
        session
            .thread_loaded
            .store(false, std::sync::atomic::Ordering::Release);
        {
            let mut mux = fixture.service.mux.write();
            let target = mux.get_tab_mut(&tab).unwrap();
            target.agent_session_id = None;
            target.reported_session_binding_nonce = None;
            if let crate::multiplexer::types::TabBacking::ChatBackend { session_id, .. } =
                &mut target.backing
            {
                session_id.clear();
            }
        }
        let reason = RuntimeFailure::Refused("opaque option refusal".into());
        retire_refused_start(&fixture.service, &session, &reason).await;
        assert!(process.terminated());
        assert!(session.process.lock().is_none());
        assert!(session.adapter.lock().is_some());
        super::super::validate_restart_binding(&fixture.service, &session).unwrap();
        let saved = serde_json::json!({"state":"degraded","failureCode":"runtime_start_unbound","reason":"opaque refusal"});
        assert!(refused_start(None, Some(&saved)).is_err());
        assert!(refused_start(Some("bound-thread"), Some(&saved)).is_ok());
        session
            .state
            .lock()
            .degrade(session.context.lock().epoch, &reason);
        let expected = if unbound {
            codeterm_plugin::agent_runtime::RuntimeFailureCode::RuntimeStartUnbound
        } else {
            codeterm_plugin::agent_runtime::RuntimeFailureCode::ProviderRefused
        };
        assert_eq!(session.state.lock().failure_code, Some(expected));
        let snapshot = serde_json::to_value(session.state.lock().snapshot()).unwrap();
        assert_eq!(
            refused_start(Some("bound-thread"), Some(&snapshot)).is_err(),
            !unbound
        );
        let result = super::super::ensure_locked(
            &fixture.service,
            &tab,
            &session.provider,
            &serde_json::json!({}),
            &session,
            None,
        )
        .await;
        assert!(
            matches!(result, Err(RuntimeFailure::Refused(message)) if message.contains("opaque option refusal") && !message.contains("already bound"))
        );
        session.state.lock().phase = super::super::RuntimePhase::Unloaded { reason: None };
        assert!(refused_start(
            None,
            Some(&serde_json::to_value(session.state.lock().snapshot()).unwrap())
        )
        .is_ok());
    }
}
