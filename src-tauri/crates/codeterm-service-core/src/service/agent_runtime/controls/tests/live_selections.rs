use super::*;
use crate::service::agent_runtime::declared_controls::{self, Selection};

#[test]
fn cli_uses_declared_model_and_effort_choices_and_persists_their_exact_values() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    let model = r#"["gateway","opaque-model"]"#;
    let controls: RuntimeSettings = serde_json::from_value(json!({
        "model":"prior","effort":"low","configOptions":[
            {"id":"engine","name":"Engine","category":"model","currentValue":"prior","options":[
                {"value":"prior","name":"Prior"},{"value":model,"name":"Selected"}]},
            {"id":"thinking","name":"Thinking","category":"thought_level","currentValue":"low","options":[
                {"value":"low","name":"Low"},{"value":"max","name":"Maximum"}]}
        ]
    })).unwrap();
    super::super::super::projections::controls(&fixture.service, &session, controls);
    fixture
        .service
        .agent_switch_model_with_options(&id, "", Some("max"), false)
        .unwrap();
    assert_eq!(
        session.context.lock().settings.effort.as_deref(),
        Some("max")
    );
    fixture
        .service
        .agent_switch_model_with_options(&id, "Selected", Some("Maximum"), false)
        .unwrap();
    for (model, effort) in [("not-offered", "max"), ("Selected", "high")] {
        assert!(fixture
            .service
            .agent_switch_model_with_options(&id, model, Some(effort), false)
            .is_err());
    }
    crate::service::session_persist::flush_session_persistence(&fixture.service).unwrap();
    let snapshot = crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
        .snapshot()
        .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    let tab = restored.get_tab(&id).unwrap();
    assert_eq!(tab.agent_runtime_config.as_ref().unwrap()["model"], model);
    assert_eq!(tab.agent_runtime_config.as_ref().unwrap()["effort"], "max");
    assert_eq!(tab.restorable_launch.model.as_deref(), Some(model));
    assert_eq!(
        tab.restorable_launch.requested_reasoning_effort.as_deref(),
        Some("max")
    );
}

fn mode_switch(
    fixture: &RuntimeFixture,
    id: &str,
    mode: &str,
) -> Result<crate::service::agent_restart::ModelSwitchOutcome, ModelSwitchError> {
    fixture.service.agent_switch_model_with_restart_fallback(
        id,
        "",
        None,
        Some(mode),
        crate::service::agent_runtime::model::ModelSwitchOptions {
            service_tier: None,
            force: false,
            restart_resume: false,
        },
    )
}

#[test]
fn a_runtime_mode_request_selects_the_advertised_session_mode_and_ends_a_named_access_failure() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    session.context.lock().provider_settings = json!({"fullAccessMode":"opaque-approve"});
    let controls: RuntimeSettings = serde_json::from_value(json!({
        "mode":"opaque-default","modes":[{"id":"opaque-default","name":"Default"},{"id":"opaque-approve","name":"Approve"}],
        "configOptions":[]
    }))
    .unwrap();
    super::super::super::projections::controls(&fixture.service, &session, controls);

    super::super::super::full_access::finish_load(&fixture.service, &session, None);
    engine::publish_controls(&fixture.service, &id, &session);
    let failure = fixture
        .service
        .get_tab_status(&id)
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.code.as_deref(), Some("full_access_unavailable"));
    assert!(
        failure.reason.contains("opaque-approve"),
        "{}",
        failure.reason
    );
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_runtime_state
            .as_ref()
            .unwrap()["accessFailure"],
        json!(failure.reason),
        "tab inspect serializes this runtime state"
    );

    let refused = mode_switch(&fixture, &id, "opaque-unadvertised").unwrap_err();
    assert!(matches!(
        refused,
        ModelSwitchError::UnsupportedRuntimeMode { ref supported, .. }
            if supported == &["opaque-default", "opaque-approve"]
    ));

    let outcome = mode_switch(&fixture, &id, "opaque-approve").unwrap();
    assert_eq!(
        session.context.lock().settings.mode.as_deref(),
        Some("opaque-approve")
    );
    let change = outcome
        .control_changes
        .iter()
        .find(|change| change.control == "mode")
        .expect("the mode change is named");
    assert_eq!(change.previous.as_deref(), Some("opaque-default"));
    assert_eq!(change.current.as_deref(), Some("opaque-approve"));
    engine::publish_controls(&fixture.service, &id, &session);
    assert!(fixture
        .service
        .get_tab_status(&id)
        .unwrap()
        .failure
        .is_none());
    assert!(session.state.lock().notice.is_none());
}

#[test]
fn a_mode_request_on_an_execution_sandbox_stays_refused() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    let controls: RuntimeSettings = serde_json::from_value(json!({
        "mode":"opaque-default","modes":[{"id":"opaque-default","name":"Default"},{"id":"opaque-approve","name":"Approve"}],
        "configOptions":[]
    }))
    .unwrap();
    super::super::super::projections::controls(&fixture.service, &session, controls);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .restorable_launch
        .execution_sandbox = Some(codeterm_types::plan::ExecutionSandboxPolicy {
        intent: crate::plan::ExecutionIntent::ReadOnly,
        cwd: String::new(),
        launch_args: Vec::new(),
        runtime_mode: Some("opaque-default".into()),
        writable_roots: Vec::new(),
        environment: Default::default(),
    });
    let refused = mode_switch(&fixture, &id, "opaque-approve").unwrap_err();
    assert!(matches!(
        refused,
        ModelSwitchError::RuntimeControlRefused(_)
    ));
    assert_eq!(
        session.context.lock().settings.mode.as_deref(),
        Some("opaque-default")
    );
}

#[test]
fn picker_model_and_effort_survive_desk_restore_and_reapply_without_launch_selections() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    let controls: RuntimeSettings = serde_json::from_value(json!({
        "model":"prior","effort":"low","configOptions":[
            {"id":"engine","name":"Engine","category":"model","currentValue":"prior","options":[
                {"value":"prior","name":"Prior"},{"value":"target","name":"Target"},
                {"value":"refuse","name":"Refused"},{"value":"unconfirmed","name":"Unconfirmed"}]},
            {"id":"thinking","name":"Thinking","category":"thought_level","currentValue":"low","options":[
                {"value":"low","name":"Low"},{"value":"high","name":"High"}]}
        ]
    })).unwrap();
    super::super::super::projections::controls(&fixture.service, &session, controls);
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert!(tab.restorable_launch.model.is_none());
        assert!(tab.restorable_launch.requested_reasoning_effort.is_none());
        assert!(tab
            .agent_runtime_config
            .as_ref()
            .is_none_or(|config| config["model"].is_null()));
    }
    let context = session.context.lock().clone();
    let select = |option: &str, value: &str, epoch| {
        crate::async_runtime::block_on(declared_controls::select(
            &fixture.service,
            &id,
            Selection {
                thread_id: context.thread_id.clone().unwrap(),
                binding_nonce: context.binding_nonce.clone(),
                epoch,
                id: option.into(),
                value: value.into(),
                mode: false,
            },
        ))
    };
    select("engine", "target", context.epoch).unwrap();
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["model"],
            "target"
        );
        assert!(tab.agent_runtime_config.as_ref().unwrap()["effort"].is_null());
    }
    assert!(select("engine", "refuse", context.epoch).is_err());
    assert!(select("engine", "unconfirmed", context.epoch).is_err());
    assert!(select("engine", "prior", context.epoch + 1).is_err());
    select("thinking", "high", context.epoch).unwrap();
    crate::service::session_persist::flush_session_persistence(&fixture.service).unwrap();
    let snapshot = crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
        .snapshot()
        .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&snapshot);
    {
        let tab = restored.get_tab(&id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
        assert_eq!(
            tab.restorable_launch.requested_model.as_deref(),
            Some("target")
        );
        assert_eq!(tab.restorable_launch.model.as_deref(), Some("target"));
        assert_eq!(
            tab.restorable_launch.requested_reasoning_effort.as_deref(),
            Some("high")
        );
        assert_eq!(tab.agent_runtime_config.as_ref().unwrap()["effort"], "high");
    }
    engine::stop_blocking(&fixture.service, &id).unwrap();
    *fixture.service.mux.write() = restored;
    assert!(super::super::super::restore::tab(&fixture.service, &id).unwrap());
    let resumed = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    let context = resumed.context.lock();
    assert_eq!(context.thread_id.as_deref(), Some("own-thread"));
    assert_eq!(context.settings.model.as_deref(), Some("target"));
    assert_eq!(context.settings.effort.as_deref(), Some("high"));
}

#[tokio::test]
async fn a_selection_on_a_held_gate_waits_its_bound_then_reports_busy() {
    use crate::service::agent_runtime::{
        engine::test_support::runtime_fixture_turn_scoped, turn_requests::tests as turn_fixture,
    };
    let (fixture, id, _peer, _cleanup) = runtime_fixture_turn_scoped(turn_fixture::SOURCE);
    let session = engine::for_tab(&id).unwrap();
    let context = session.context.lock().clone();
    let _held = session.gate.try_acquire().unwrap();
    tokio::time::pause();
    let outcome = declared_controls::select(
        &fixture.service,
        &id,
        Selection {
            thread_id: context.thread_id.clone().unwrap(),
            binding_nonce: context.binding_nonce.clone(),
            epoch: context.epoch,
            id: "engine".into(),
            value: "target".into(),
            mode: false,
        },
    )
    .await;
    assert_eq!(outcome.unwrap_err(), declared_controls::ControlError::Busy);
}

#[test]
fn a_selection_on_a_held_gate_applies_once_the_running_operation_finishes() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    let controls: RuntimeSettings = serde_json::from_value(json!({
        "model":"prior","effort":"low","configOptions":[
            {"id":"engine","name":"Engine","category":"model","currentValue":"prior","options":[
                {"value":"prior","name":"Prior"},{"value":"target","name":"Target"}]}
        ]
    }))
    .unwrap();
    super::super::super::projections::controls(&fixture.service, &session, controls);
    let context = session.context.lock().clone();
    let held = session.gate.try_acquire().unwrap();
    let (outcome, ()) = crate::async_runtime::block_on(async {
        tokio::join!(
            biased;
            declared_controls::select(
                &fixture.service,
                &id,
                Selection {
                    thread_id: context.thread_id.clone().unwrap(),
                    binding_nonce: context.binding_nonce.clone(),
                    epoch: context.epoch,
                    id: "engine".into(),
                    value: "target".into(),
                    mode: false,
                },
            ),
            async move { drop(held) },
        )
    });
    outcome.unwrap();
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap()["model"],
        "target"
    );
}
