use super::*;

fn clear_thread(fixture: &RuntimeFixture) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(&fixture.tab).unwrap();
    tab.agent_session_id = None;
    tab.reported_session_binding_nonce = None;
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: String::new(),
    };
    tab.agent_booting = true;
}

fn fresh_and_assert_loaded(fixture: &RuntimeFixture) {
    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(outcome.agent_ready);
    assert!(!outcome.force);
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert!(agent_runtime::engine::has_live_bound_session(&tab));
    assert_eq!(tab.agent_runtime_state.as_ref().unwrap()["state"], "loaded");
    assert!(tab.agent_session_id.is_some());
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "resumeThread")
            .count(),
        0
    );
}

#[test]
fn refused_unbound_start_fresh_restarts_without_force_into_a_loaded_runtime() {
    for retain_adapter in [true, false] {
        let source = SOURCE.replace("module.exports = {", "let refusedStart=false; module.exports = {")
        .replace("runtimeResponse(ctx, op, state, reply) {", r#"runtimeResponse(ctx, op, state, reply) {
            if (op.kind === 'openThread' && !refusedStart) {
                refusedStart=true;
                return {kind:'refused',reason:{code:'provider_refused',message:'opaque initial refusal'},beforeAcceptance:true};
            }"#);
        let fixture = RuntimeFixture::prepared(&source, true);
        clear_thread(&fixture);
        assert!(agent_runtime::transport::launch(
            &fixture.service,
            PROVIDER,
            &fixture.tab,
            json!({"systemPrompt":PROMPT,"model":"supplied-model"})
        )
        .is_err());
        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .clone();
        assert_eq!(
            tab.agent_runtime_state.as_ref().unwrap()["failureCode"],
            "runtime_start_unbound"
        );
        assert!(agent_runtime::start_failure::for_tab(&tab));
        if !retain_adapter {
            agent_runtime::engine::for_tab(&fixture.tab)
                .unwrap()
                .adapter
                .lock()
                .take();
        }
        fresh_and_assert_loaded(&fixture);
        assert_eq!(
            fixture
                .requests()
                .iter()
                .filter(|request| request["method"] == "openThread")
                .count(),
            2
        );
    }
}

#[test]
fn restored_unbound_start_remints_fresh_restart_authority() {
    let fixture = RuntimeFixture::prepared(SOURCE, true);
    clear_thread(&fixture);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_runtime_state = Some(json!({"state":"degraded","epoch":1,
            "failureCode":"runtime_start_unbound","reason":"Provider refused: ACP session is already bound"}));
        let bytes = serde_json::to_vec(&mux.to_session_snapshot()).unwrap();
        *mux = MultiplexerState::from_session_snapshot(&serde_json::from_slice(&bytes).unwrap());
    }
    assert!(agent_runtime::engine::for_tab(&fixture.tab).is_none());
    assert!(agent_runtime::restore::tab(&fixture.service, &fixture.tab).unwrap());
    let tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    assert_eq!(
        tab.agent_runtime_state.as_ref().unwrap()["failureCode"],
        "runtime_start_unbound"
    );
    assert!(!agent_runtime::authority::verifies(
        &fixture.service,
        &fixture.tab,
        PROVIDER,
        tab.session_binding_nonce.as_deref().unwrap()
    ));
    fresh_and_assert_loaded(&fixture);
    assert_eq!(
        fixture
            .requests()
            .iter()
            .filter(|request| request["method"] == "openThread")
            .count(),
        1
    );
}

#[test]
fn explicit_restart_retries_a_restored_refused_resume_that_automatic_restarts_leave_blocked() {
    const CAUSE: &str = "Fixture could not resume the session: opaque install refusal";
    let fixture = RuntimeFixture::prepared(SOURCE, true);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.restored_from_snapshot = true;
        tab.agent_runtime_state = Some(json!({"state":"degraded","epoch":1,
            "failureCode":"provider_refused","reason":format!("Provider refused: Provider refused: {CAUSE}")}));
    }
    assert!(agent_runtime::engine::for_tab(&fixture.tab).is_none());
    let retained = |fixture: &RuntimeFixture| {
        let mux = fixture.service.mux.read();
        let state = mux
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_state
            .clone()
            .unwrap();
        (
            state["state"].clone(),
            state["failureCode"].clone(),
            state["reason"].clone(),
        )
    };
    let clear_cooldown = |fixture: &RuntimeFixture| {
        fixture
            .service
            .ext::<crate::service::agent_restart::RestartTracker>()
            .lock()
            .last_restart
            .remove(&fixture.tab);
    };
    for _ in 0..2 {
        assert!(
            crate::async_runtime::block_on(agent_runtime::engine::ensure(
                &fixture.service,
                &fixture.tab
            ))
            .is_err()
        );
        clear_cooldown(&fixture);
        assert!(matches!(
            fixture.service.agent_restart_observed_with_options(&fixture.tab, RestartOptions::default()),
            Err(RestartAttemptError::Failed { ref reason, .. }) if *reason == format!("Provider refused: {CAUSE}")
        ));
        assert_eq!(
            retained(&fixture),
            (
                json!("degraded"),
                json!("provider_refused"),
                json!(format!("Provider refused: {CAUSE}"))
            )
        );
    }
    assert!(fixture.requests().is_empty());
    clear_cooldown(&fixture);
    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                retry_refused_start: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(outcome.agent_ready);
    assert_eq!(outcome.session_id.as_deref(), Some("own-thread"));
    let requests = fixture.requests();
    let resumed: Vec<_> = requests
        .iter()
        .filter(|frame| frame["method"] == "resumeThread")
        .collect();
    assert_eq!(resumed.len(), 1);
    assert_eq!(resumed[0]["params"]["threadId"], "own-thread");
    assert!(requests.iter().all(|frame| frame["method"] != "openThread"));
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    assert!(session
        .thread_loaded
        .load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(retained(&fixture).0, json!("loaded"));
}

#[test]
fn unbound_recovery_requires_exact_unbound_runtime_shape() {
    let fixture = RuntimeFixture::prepared(SOURCE, true);
    clear_thread(&fixture);
    let mut tab = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .clone();
    tab.agent_runtime_state =
        Some(json!({"state":"degraded","failureCode":"runtime_start_unbound"}));
    assert!(agent_runtime::start_failure::for_tab(&tab));
    let valid = tab.clone();
    tab.agent_runtime_state.as_mut().unwrap()["failureCode"] = json!("provider_refused");
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid.clone();
    tab.agent_session_id = Some("bound-thread".into());
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid.clone();
    tab.reported_session_binding_nonce = tab.session_binding_nonce.clone();
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid.clone();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "foreign".into(),
        session_id: String::new(),
    };
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid.clone();
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: "bound-thread".into(),
    };
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid.clone();
    tab.session_binding_nonce = None;
    assert!(!agent_runtime::start_failure::for_tab(&tab));
    tab = valid;
    tab.agent_runtime_state = Some(json!({"state":"loaded","failureCode":"runtime_start_unbound"}));
    assert!(!agent_runtime::start_failure::for_tab(&tab));
}
