use super::*;
use serde_json::json;

#[test]
fn last_declared_options_round_trip_and_bind_to_profile() {
    let root = tempfile::tempdir().unwrap();
    let profile: RuntimeLaunchProfile =
        serde_json::from_value(json!({"binary":"fixture-agent","args":[]})).unwrap();
    let settings: RuntimeSettings = serde_json::from_value(json!({"configOptions":[
        {"id":"model-choice","name":"Model","category":"model","currentValue":"M","options":[{"value":"M","name":"Model M"}]},
        {"id":"effort-choice","name":"Effort","category":"thought_level","currentValue":"E","options":[{"value":"E","name":"Effort E"}]},
        {"id":"other","name":"Other","currentValue":"x","options":[]}
    ]})).unwrap();
    let saved = snapshot(&profile, &settings).unwrap();
    write(&path(root.path(), "fixture").unwrap(), &saved).unwrap();
    let observed = read(root.path(), "fixture", &profile);
    assert_eq!(observed.len(), 2);
    assert_eq!(observed[0].id, "model-choice");
    assert_eq!(observed[1].options[0].name, "Effort E");
    let changed = RuntimeLaunchProfile {
        binary: "another-agent".into(),
        ..profile.clone()
    };
    assert!(read(root.path(), "fixture", &changed).is_empty());
    let cleared = snapshot(
        &profile,
        &RuntimeSettings {
            config_options: Some(Vec::new()),
            ..Default::default()
        },
    )
    .unwrap();
    write(&path(root.path(), "fixture").unwrap(), &cleared).unwrap();
    assert!(read(root.path(), "fixture", &profile).is_empty());
    assert!(snapshot(&profile, &RuntimeSettings::default()).is_none());
    assert!(path(root.path(), "../escape").is_none());
    assert!(path(root.path(), "NUL").is_none());
}

#[test]
fn forgetting_a_profile_drops_only_its_own_remembered_and_queued_options() {
    let root = tempfile::tempdir().unwrap();
    let profile: RuntimeLaunchProfile =
        serde_json::from_value(json!({"binary":"fixture-agent","args":[]})).unwrap();
    let settings: RuntimeSettings = serde_json::from_value(json!({"configOptions":[
        {"id":"model-choice","name":"Model","category":"model","currentValue":"M","options":[{"value":"M","name":"Model M"}]}
    ]})).unwrap();
    let saved = snapshot(&profile, &settings).unwrap();
    let (removed, kept) = (
        path(root.path(), "fixture").unwrap(),
        path(root.path(), "fixture-extra").unwrap(),
    );
    for path in [&removed, &kept] {
        write(path, &saved).unwrap();
        let mut writes = WRITES.lock();
        writes.latest.insert(path.clone(), saved.clone());
        writes.pending.insert(path.clone(), saved.clone());
    }
    forget(root.path(), "fixture").unwrap();
    assert!(!removed.exists());
    assert!(read(root.path(), "fixture", &profile).is_empty());
    {
        let writes = WRITES.lock();
        assert!(!writes.pending.contains_key(&removed) && !writes.latest.contains_key(&removed));
    }
    assert!(kept.exists());
    assert_eq!(read(root.path(), "fixture-extra", &profile).len(), 1);
    forget(root.path(), "fixture").unwrap();
    forget(root.path(), "fixture-extra").unwrap();
}

#[test]
fn profile_catalogue_resolves_only_exact_values_or_unique_declared_names() {
    let value = r#"["endpoint","model-a"]"#;
    let options: Vec<RuntimeConfigOption> = serde_json::from_value(json!([
        {"id":"engine","name":"Model","category":"model","currentValue":value,
         "options":[{"value":value,"name":"Model A"},{"value":"other","name":"Repeated"},{"value":"third","name":"Repeated"}]},
        {"id":"effort","name":"Effort","category":"thought_level","currentValue":"low",
         "options":[{"value":"low","name":"Low"}]}
    ])).unwrap();
    assert_eq!(resolve(&options, "model", value).unwrap(), value);
    assert_eq!(resolve(&options, "model", "Model A").unwrap(), value);
    assert_eq!(resolve(&options, "thought_level", "Low").unwrap(), "low");
    for requested in ["missing", "Repeated", "model a", "endpoint/model-a"] {
        let error = resolve(&options, "model", requested).unwrap_err();
        assert!(error.contains("Model A"));
        assert!(error.contains(value));
    }
    assert!(resolve(&options, "thought_level", "Model A").is_err());
    assert!(resolve(&[], "model", value).is_err());
}

#[test]
fn profile_selections_are_resolved_before_spawn_creates_a_tab() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use std::sync::Arc;
    let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let provider = format!("profile-selection-{}", uuid::Uuid::new_v4());
    let manifest = PluginManifest::from_json_str(
        &json!({
            "id":provider,"version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true,
                "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},"agentRuntime":{
                "runtime":"stdio-jsonrpc","command":{"binary":"fixture","args":[]},
                "launchProfiles":{"selected":{"binary":"fixture","args":[]}},
                "requestMethods":{"initialize":["initialize"]},"notificationMethods":[]}},
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}},
            "permissions":{"subprocess":{"allow":["fixture"]}}
        })
        .to_string(),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(&provider, "module.exports = {};".into()).unwrap()),
        )));
    let profile =
        super::super::launch_profile_install::declaration(&fixture.service, &provider, "selected")
            .unwrap();
    let settings: RuntimeSettings = serde_json::from_value(json!({"configOptions":[
        {"id":"model","name":"Model","category":"model","currentValue":"opaque-value",
         "options":[{"value":"opaque-value","name":"Friendly"}]},
        {"id":"effort","name":"Effort","category":"thought_level","currentValue":"low",
         "options":[{"value":"low","name":"Low"}]}
    ]}))
    .unwrap();
    let path = path(
        &super::super::launch_profile_install::profile_root(&provider).unwrap(),
        "selected",
    )
    .unwrap();
    WRITES
        .lock()
        .latest
        .insert(path.clone(), snapshot(&profile, &settings).unwrap());
    let before = fixture.service.mux.read().snapshot().tabs.len();
    let base = crate::service::SpawnAgentParams {
        agent: Some(provider.clone()),
        launch_profile: Some("selected".into()),
        model: Some("Friendly".into()),
        reasoning_effort: Some("low".into()),
        ..Default::default()
    };
    for (model, effort, offered) in [
        ("missing", "low", "Friendly"),
        ("Friendly", "invalid", "Low"),
    ] {
        let error = fixture
            .service
            .spawn_agent_sync(crate::service::SpawnAgentParams {
                model: Some(model.into()),
                reasoning_effort: Some(effort.into()),
                ..base.clone()
            })
            .unwrap_err();
        assert!(error.contains("offered choices"), "{error}");
        assert!(error.contains(offered), "{error}");
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    }
    let mut selected = base.clone();
    fixture
        .service
        .resolve_spawn_profile_options(&provider, &mut selected)
        .unwrap();
    assert_eq!(selected.model.as_deref(), Some("opaque-value"));
    assert_eq!(selected.reasoning_effort.as_deref(), Some("low"));
    let mut spec = crate::service::spawn::SpawnTabSpec::from_agent_params(selected);
    spec.route_agent_to_chatbackend().unwrap();
    let crate::service::spawn::TabCommand::ChatBackend { config, .. } = spec.command else {
        panic!("runtime route missing")
    };
    assert_eq!(config["model"], "opaque-value");
    let models_only: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../../../../plugins/provider-acp/tests/fixtures/runtime/session-models-only.json"
    ))
    .unwrap();
    let settings: RuntimeSettings = serde_json::from_value(
        models_only["expected"]["results"][1]["step"]["result"]["settings"].clone(),
    )
    .unwrap();
    WRITES
        .lock()
        .latest
        .insert(path.clone(), snapshot(&profile, &settings).unwrap());
    let catalogue = read(path.parent().unwrap(), "selected", &profile);
    assert_eq!(catalogue.len(), 1);
    assert_eq!(catalogue[0].options[1].value, "wire-two");
    assert_eq!(catalogue[0].options[1].name, "Second declared");
    let mut selected = crate::service::SpawnAgentParams {
        model: Some("Second declared".into()),
        reasoning_effort: None,
        ..base.clone()
    };
    fixture
        .service
        .resolve_spawn_profile_options(&provider, &mut selected)
        .unwrap();
    assert_eq!(selected.model.as_deref(), Some("wire-two"));
    for (model, effort, message) in [
        ("unknown", None, "Requested model"),
        ("wire-two", Some("low"), "Requested reasoning effort"),
    ] {
        let error = fixture
            .service
            .spawn_agent_sync(crate::service::SpawnAgentParams {
                model: Some(model.into()),
                reasoning_effort: effort.map(str::to_owned),
                ..base.clone()
            })
            .unwrap_err();
        assert!(error.contains(message), "{error}");
        assert_eq!(fixture.service.mux.read().snapshot().tabs.len(), before);
    }
    WRITES.lock().latest.remove(&path);
}
