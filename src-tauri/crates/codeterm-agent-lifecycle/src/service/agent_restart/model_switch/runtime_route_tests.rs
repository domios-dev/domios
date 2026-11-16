//! Runtime-routed model switches entered through the lifecycle crate; moved from the core
//! controls tests with the switch dispatch they cover.

use crate::multiplexer::state::MultiplexerState;
use crate::prelude::*;
use crate::service::agent_lifecycle::{ModelSwitchConfirmation, ModelSwitchError};
use crate::service::agent_runtime::controls::runtime_test_fixture::{
    runtime_fixture, tier_selection,
};
use crate::service::agent_runtime::engine;
use codeterm_plugin::agent_runtime::RuntimeSettings;
use serde_json::json;

#[test]
fn qualified_model_provider_mismatch_preserves_runtime_tab() {
    let (fixture, id) = runtime_fixture();
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("foreign-settings-fixture"),
    ));
    let error = fixture
        .service
        .agent_switch_model_with_options(&id, "foreign-settings-fixture:target", None, false)
        .unwrap_err();
    assert!(matches!(error, ModelSwitchError::InvalidModel(_)));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_model.as_deref(), Some("prior"));
    assert!(tab.pending_model_selection.is_none());
}
#[test]
fn runtime_control_confirmation_updates_recorded_model_and_next_turn_settings() {
    let (fixture, id) = runtime_fixture();
    let outcome = fixture
        .service
        .agent_switch_model_with_restart_fallback(
            &id,
            "settings-fixture:target",
            Some("high"),
            Some("auto"),
            crate::service::agent_runtime::model::ModelSwitchOptions {
                service_tier: Some(tier_selection(false)),
                force: false,
                restart_resume: false,
            },
        )
        .unwrap();
    assert_eq!(
        outcome.mechanism,
        crate::agent::provider::ModelSwitchMode::Live
    );
    assert!(
        matches!(outcome.confirmation, ModelSwitchConfirmation::Observed { model, .. } if model == "target")
    );
    {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert_eq!(tab.agent_session_model.as_deref(), Some("target"));
        assert_eq!(
            tab.reasoning_effort_identity.actual.as_deref(),
            Some("high")
        );
        assert_eq!(tab.structured_runtime_mode.as_deref(), Some("auto"));
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["serviceTier"],
            serde_json::json!({"id":null})
        );
        assert!(tab.pending_model_selection.is_none());
        assert_eq!(tab.restorable_launch.model.as_deref(), Some("target"));
        assert_eq!(
            tab.restorable_launch.requested_reasoning_effort.as_deref(),
            Some("high")
        );
        assert_eq!(
            tab.reasoning_effort_identity.requested.as_deref(),
            Some("high")
        );
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap()["model"],
            "target"
        );
    }
    crate::service::session_persist::flush_session_persistence(&fixture.service).unwrap();
    let persisted =
        crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
            .snapshot()
            .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    assert_eq!(
        restored
            .get_tab(&id)
            .unwrap()
            .restorable_launch
            .model
            .as_deref(),
        Some("target")
    );
    assert_eq!(
        restored
            .get_tab(&id)
            .unwrap()
            .agent_runtime_config
            .as_ref()
            .unwrap()["serviceTier"],
        serde_json::json!({"id":null})
    );
    let session = engine::for_tab(&id).unwrap();
    assert_eq!(
        session.context.lock().settings.service_tier,
        Some(tier_selection(false))
    );
    session.context.lock().active_turn_id = Some("working-turn".into());
    fixture
        .service
        .agent_switch_model_with_options(&id, "mid-turn", Some("low"), false)
        .unwrap();
    assert_eq!(
        session.context.lock().settings.model.as_deref(),
        Some("mid-turn")
    );
    let refusal = fixture
        .service
        .agent_switch_model_with_options(&id, "refuse", None, false)
        .unwrap_err();
    assert!(matches!(
        refusal,
        ModelSwitchError::RuntimeControlRefused(_)
    ));
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_model
            .as_deref(),
        Some("mid-turn")
    );
    assert_eq!(
        session.context.lock().settings.model.as_deref(),
        Some("mid-turn")
    );
    engine::stop_blocking(&fixture.service, &id).unwrap();
}

#[test]
fn model_switch_preserves_the_current_mode_and_clears_the_previous_notice() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    session.context.lock().settings.mode = Some("auto".into());
    session.state.lock().notice = Some("Requested model was not applied".into());
    let outcome = fixture
        .service
        .agent_switch_model_with_options(&id, "target", None, false)
        .unwrap();
    assert!(
        matches!(outcome.confirmation, ModelSwitchConfirmation::Observed { model, .. } if model == "target")
    );
    assert_eq!(
        session.context.lock().settings.mode.as_deref(),
        Some("auto")
    );
    assert!(session.state.lock().notice.is_none());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_model.as_deref(), Some("target"));
    assert_eq!(tab.restorable_launch.model.as_deref(), Some("target"));
    assert_eq!(
        tab.restorable_launch.structured_runtime_mode.as_deref(),
        Some("auto")
    );
    assert_eq!(
        tab.agent_runtime_state.as_ref().unwrap()["notice"],
        serde_json::Value::Null
    );
}

#[test]
fn model_switch_on_a_sleeping_runtime_tab_wakes_it_and_applies_the_selection() {
    let (fixture, id) = runtime_fixture();
    fixture.service.mux.write().get_tab_mut(&id).unwrap().frozen = true;
    let outcome = fixture
        .service
        .agent_switch_model_with_options(&id, "target", Some("high"), false)
        .unwrap();
    assert!(
        matches!(outcome.confirmation, ModelSwitchConfirmation::Observed { model, .. } if model == "target")
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(!tab.frozen);
    assert_eq!(tab.agent_session_model.as_deref(), Some("target"));
}

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
    crate::service::agent_runtime::projections::controls(&fixture.service, &session, controls);
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
