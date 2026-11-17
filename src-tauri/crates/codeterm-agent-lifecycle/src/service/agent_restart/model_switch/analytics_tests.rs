use super::*;
use crate::agent::test_provider::MockProvider;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

fn fixture() -> (ServiceFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("analytics-fixture".into());
    tab.agent_session_id = Some("session".into());
    tab.session_binding_nonce = Some("nonce".into());
    tab.reported_session_binding_nonce = Some("nonce".into());
    tab.agent_session_model = Some("prior".into());
    tab.cwd = Some(std::env::temp_dir().display().to_string());
    (ServiceFixture::new(mux), id)
}

fn outcome(tab: &str, mechanism: ModelSwitchMode) -> ModelSwitchOutcome {
    ModelSwitchOutcome {
        tab_id: tab.into(),
        provider: "analytics-fixture".into(),
        session_id: "session".into(),
        restart: None,
        mechanism,
        fallback_reason: None,
        previous_model: Some("prior".into()),
        requested_model: "target".into(),
        launched_model: "target".into(),
        confirmation_detail: None,
        confirmation: ModelSwitchConfirmation::Pending,
        control_changes: Vec::new(),
    }
}

#[tokio::test]
async fn completion_payloads_are_closed_and_fence_stale_restored_noop_and_optout() {
    let (fixture, id) = fixture();
    let recording = crate::test_support::record_product_analytics(
        &fixture.service,
        fixture.temp.path().join("analytics.json"),
    )
    .await;
    let _pty = fixture.bind_test_pty_session(&id, "session", "nonce");
    for mode in [
        ModelSwitchMode::Live,
        ModelSwitchMode::LiveCommand,
        ModelSwitchMode::NextTurn,
    ] {
        let mut result = outcome(&id, mode);
        result.confirmation = ModelSwitchConfirmation::Observed {
            model: "target".into(),
            evidence: ModelSwitchEvidence::LiveSessionRecord,
        };
        fixture
            .service
            .record_product_model_switch(&Ok(result), Some("nonce"));
    }
    let snap = fixture.service.snapshot_tab_for_restart(&id).unwrap();
    let mut restart = super::super::RestartOutcome::accepted(&snap, false, false);
    restart.agent_ready = true;
    restart.binding_restored = true;
    restart.new_pty_id = Some(snap.pty_id.clone());
    let mut result = outcome(&id, ModelSwitchMode::RestartResume);
    result.restart = Some(restart.clone());
    fixture
        .service
        .record_product_model_switch(&Ok(result.clone()), Some("retired-nonce"));
    let expected = ["live", "live", "next_turn", "restart"]
        .into_iter()
        .enumerate()
        .map(|(index, mode)| {
            serde_json::json!({"event_type":"agent_model_changed","event_properties":{
                "mode":mode,"outcome":if index==3 {"pending"} else {"observed"}
            }})
        })
        .collect::<Vec<_>>();
    assert_eq!(recording.events().await, expected);
    fixture.service.record_product_model_switch(
        &Err(ModelSwitchError::NotSwitchable("private refusal".into())),
        Some("nonce"),
    );
    let mut staged = outcome(&id, ModelSwitchMode::Live);
    staged.confirmation = ModelSwitchConfirmation::AfterTurn;
    fixture
        .service
        .record_product_model_switch(&Ok(staged), Some("nonce"));
    let mut noop = outcome(&id, ModelSwitchMode::Live);
    noop.previous_model = Some(noop.launched_model.clone());
    fixture
        .service
        .record_product_model_switch(&Ok(noop), Some("nonce"));
    fixture.service.record_product_model_switch(
        &Ok(outcome(&id, ModelSwitchMode::Live)),
        Some("stale-nonce"),
    );
    let mut stale = outcome(&id, ModelSwitchMode::Live);
    stale.session_id = "foreign-session".into();
    fixture
        .service
        .record_product_model_switch(&Ok(stale), Some("nonce"));
    for restore in [true, false] {
        let mut failed = result.clone();
        if restore {
            failed.restart.as_mut().unwrap().source_account_restored = true;
        } else {
            failed.restart.as_mut().unwrap().new_pty_id = Some("foreign-pty".into());
        }
        fixture
            .service
            .record_product_model_switch(&Ok(failed), Some("nonce"));
    }
    assert_eq!(recording.events().await, expected);
    recording.set_enabled(false).await;
    fixture
        .service
        .record_product_model_switch(&Ok(result), Some("nonce"));
    assert_eq!(recording.events().await, expected);
}

#[tokio::test]
async fn claimed_staged_execution_emits_once_and_stale_or_failed_claims_emit_nothing() {
    let (fixture, id) = fixture();
    let recording = crate::test_support::record_product_analytics(
        &fixture.service,
        fixture.temp.path().join("analytics.json"),
    )
    .await;
    let _pty = fixture.bind_test_pty_session(&id, "session", "nonce");
    let pty_id = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .pty_id
        .clone()
        .unwrap();
    fixture.service.agent_tracker.lock().set_idle(&pty_id);
    fixture.service.agent_registry.register_provider(Box::new(
        MockProvider::with_session("analytics-fixture", "session")
            .with_model_switch(ModelSwitchMode::NextTurn),
    ));
    let direct = fixture
        .service
        .agent_switch_model(&id, "direct-target")
        .unwrap();
    assert_eq!(direct.confirmation, ModelSwitchConfirmation::Pending);
    assert_eq!(recording.events().await.len(), 1);
    let snap = fixture.service.snapshot_tab_for_restart(&id).unwrap();
    let request = staging::SwitchRequest {
        id: "request".into(),
        tab_id: id.clone(),
        provider: "analytics-fixture".into(),
        session_id: "session".into(),
        pty_id: snap.pty_id,
        nonce: "nonce".into(),
        launch: fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .clone(),
        model: "target".into(),
        effort: None,
        force: false,
        restart_fallback: false,
        mechanism: ModelSwitchMode::NextTurn,
        deadline: Instant::now() + Duration::from_secs(60),
        completion_generation: 0,
        completion_epoch: 0,
    };
    let install = |request: &staging::SwitchRequest| {
        let (tx, _) = std::sync::mpsc::channel();
        fixture
            .service
            .ext::<super::super::RestartTracker>()
            .lock()
            .staged_models
            .replace(request.clone(), tx);
    };
    install(&request);
    assert!(fixture
        .service
        .settle_staged_model_switch(request.clone(), true));
    assert_eq!(
        recording.events().await,
        vec![
            serde_json::json!({
                "event_type":"agent_model_changed","event_properties":{"mode":"next_turn","outcome":"pending"}
            });
            2
        ]
    );
    assert!(fixture
        .service
        .settle_staged_model_switch(request.clone(), true));
    let mut stale = request.clone();
    stale.id = "stale".into();
    stale.nonce = "foreign".into();
    install(&stale);
    assert!(fixture.service.settle_staged_model_switch(stale, true));
    let mut failed = request.clone();
    failed.id = "failed".into();
    install(&failed);
    assert!(fixture.service.settle_staged_model_switch(failed, false));
    assert_eq!(recording.events().await.len(), 2);
    recording.set_enabled(false).await;
    let mut opted_out = request;
    opted_out.id = "off".into();
    opted_out.model = "off-target".into();
    opted_out.launch = fixture
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .restorable_launch
        .clone();
    install(&opted_out);
    assert!(fixture.service.settle_staged_model_switch(opted_out, true));
    assert_eq!(recording.events().await.len(), 2);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_requested_model
            .as_deref(),
        Some("off-target")
    );
}
