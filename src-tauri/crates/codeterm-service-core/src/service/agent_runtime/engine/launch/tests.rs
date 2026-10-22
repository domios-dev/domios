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
