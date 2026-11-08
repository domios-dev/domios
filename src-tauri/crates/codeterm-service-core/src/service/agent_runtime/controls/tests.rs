use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use serde_json::json;
use std::sync::Arc;

#[test]
fn routing_requires_both_runtime_capability_and_chat_backing() {
    let chat = TabBacking::ChatBackend {
        plugin_id: "arbitrary-provider".into(),
        session_id: "thread".into(),
    };
    assert_eq!(switch_route(true, &chat), SwitchRoute::Runtime);
    assert_eq!(switch_route(false, &chat), SwitchRoute::Pty);
    assert_eq!(switch_route(true, &TabBacking::Pty), SwitchRoute::Pty);
    assert_eq!(switch_route(false, &TabBacking::Pty), SwitchRoute::Pty);
}

#[test]
fn missing_or_partial_confirmation_cannot_confirm_requested_controls() {
    let requested = RuntimeSettings {
        model: Some("target".into()),
        effort: Some("high".into()),
        mode: Some("auto".into()),
        ..Default::default()
    };
    assert!(!confirms(&requested, &RuntimeSettings::default()));
    assert!(!confirms(
        &requested,
        &RuntimeSettings {
            model: Some("old".into()),
            ..requested.clone()
        }
    ));
    assert!(confirms(&requested, &requested));
}

const PROVIDER: &str = "settings-fixture";

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
const SOURCE: &str = r#"
module.exports = {
  runtimePlan(ctx, op) {
    if (op.kind === 'control' && op.value.serviceTier !== undefined) return {action:{kind:'request',call:{method:'fixture/settings',params:op}},state:null};
    const target = op.kind === 'interrupt' ? {threadId:op.threadId,...(op.activeTurnId ? {turnId:op.activeTurnId} : {})} : undefined;
    return {action:{kind:'request',call:{method:op.kind,params:op,...(target ? {target} : {})}},state:null};
  },
  runtimeResponse(ctx, op, state, reply) {
    let result = {kind:'done'};
    let events = [];
    if (op.kind === 'initialize') result = {kind:'initialized'};
    if (op.kind === 'resumeThread') result = {kind:'threadLoaded',threadId:op.threadId,settings:op.settings};
    if (op.kind === 'readHistory') result = {kind:'historyPage',threadId:op.threadId,items:[],openItems:[]};
    if (op.kind === 'submitTurn') {
      result = {kind:'turnAccepted',threadId:op.threadId,sendId:op.sendId,turnId:'restored-turn'};
      events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:reply.value.params.settings}];
    }
    if (op.kind === 'interrupt') {
      events = op.activeTurnId ? [{kind:'turnState',threadId:op.threadId,turnId:op.activeTurnId,state:'idle'}] : [];
      if (ctx.providerSettings && ctx.providerSettings.interruptProbe) events.push({kind:'controlsConfirmed',threadId:op.threadId,settings:{effort:'high'}});
    }
    if (op.kind === 'control') {
      if (op.value.model === 'refuse') return {kind:'refused',reason:{code:'provider_refused',message:'settings refused'}};
      if (op.action === 'mode') events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:{mode:op.value}}];
      else if (op.action === 'config') {
        if (op.value.value === 'refuse') return {kind:'refused',reason:{code:'provider_refused',message:'selection refused'}};
        const options = ctx.settings.configOptions.map(option => option.id !== op.value.id || op.value.value === 'unconfirmed' ? option : {...option,currentValue:op.value.value});
        const model = options.find(option => option.category === 'model').currentValue;
        const effort = options.find(option => option.category === 'thought_level').currentValue;
        events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:{model,effort,configOptions:options}}];
      } else if (op.value.serviceTier === undefined) events = [{kind:'controlsConfirmed',threadId:op.threadId,settings:op.value}];
    }
    return {kind:'complete',result,events};
  },
  runtimeReduce(ctx, state, frames) { return {state:null,events:frames.map(frame=>({kind:'controlsConfirmed',threadId:frame.params.threadId,settings:frame.params.settings}))}; }
};
"#;

pub(super) struct RuntimeFixture {
    fixture: ServiceFixture,
    tab: String,
    storage: crate::storage::Storage,
}

impl std::ops::Deref for RuntimeFixture {
    type Target = ServiceFixture;

    fn deref(&self) -> &Self::Target {
        &self.fixture
    }
}

impl Drop for RuntimeFixture {
    fn drop(&mut self) {
        engine::stop_blocking(&self.fixture.service, &self.tab).unwrap();
        crate::service::session_persist::stop_worker(&self.fixture.service)
            .expect("stop runtime fixture persistence worker");
    }
}

pub(super) fn runtime_fixture() -> (RuntimeFixture, String) {
    runtime_fixture_with_interrupt(false)
}

pub(super) fn runtime_fixture_with_interrupt(before_first: bool) -> (RuntimeFixture, String) {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: "own-thread".into(),
    };
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.agent_session_model = Some("prior".into());
    tab.cwd = Some(
        crate::test_support::fixture_manifest_dir()
            .to_string_lossy()
            .into_owned()
            .into(),
    );
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    let storage = fixture.install_seeded_storage_desk();
    let methods = [
        "initialize",
        "resumeThread",
        "readHistory",
        "submitTurn",
        "control",
        "interrupt",
    ]
    .into_iter()
    .map(|kind| {
        (
            kind.to_owned(),
            if kind == "control" {
                json!([kind, "fixture/settings"])
            } else {
                json!([kind])
            },
        )
    })
    .collect::<serde_json::Map<String, serde_json::Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":PROVIDER,"version":"1.0.0","displayName":"Fixture",
        "reasoningEfforts":[{"id":"low","displayName":"Low"},{"id":"high","displayName":"High"}],
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"interrupt":if before_first {json!({"beforeFirstTurn":true})} else {serde_json::Value::Null},"runtimeModes":[{"id":"approval-required","displayName":"Approval required"},{"id":"auto-accept-edits","displayName":"Auto accept edits"},{"id":"auto","displayName":"Auto"},{"id":"full-access","displayName":"Full access"}],"serviceTiers":[{"id":"priority","name":"Fast","description":""}],"runtime":"stdio-jsonrpc","command":{"binary":"node","args":[format!("{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs",crate::test_support::fixture_manifest_dir().display())]},"requestMethods":methods,"notificationMethods":["fixture/settings/updated"]}},
        "permissions":{"subprocess":{"allow":["node"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, SOURCE.into()).unwrap()),
        )));
    super::super::authority::record_spawn(&fixture.service, &id, PROVIDER, "own-nonce").unwrap();
    (
        RuntimeFixture {
            fixture,
            tab: id.clone(),
            storage,
        },
        id,
    )
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
fn close_restore_reapplies_runtime_settings_to_the_exact_thread_on_first_turn() {
    let (mut fixture, original_id) = runtime_fixture();
    let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
    let config = json!({
        "model":"gpt-6-luna",
        "effort":"low",
        "serviceTier":{"id":"priority"},
        "mode":"auto",
        "confirmedServiceTier":{
            "selection":{"id":"priority"},
            "sessionId":"own-thread",
            "bindingNonce":"original-nonce"
        }
    });
    let launch = crate::multiplexer::types::RestorableLaunchParameters {
        captured: true,
        model: Some("gpt-6-luna".into()),
        requested_model: Some("gpt-6-luna".into()),
        requested_reasoning_effort: Some("low".into()),
        resolved_reasoning_effort: Some("low".into()),
        structured_runtime_mode: Some("auto".into()),
        ..Default::default()
    };
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&original_id).unwrap();
        tab.agent_runtime_config = Some(config);
        tab.restorable_launch = launch;
        tab.structured_runtime_mode = Some("auto".into());
        mux.close_tab(&original_id).unwrap();
    }
    let restored_id = fixture
        .service
        .mux
        .write()
        .reopen_closed_tab(&workspace, None)
        .unwrap();
    let restored_nonce = "restored-nonce";
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&restored_id).unwrap();
        assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
        tab.session_binding_nonce = Some(restored_nonce.into());
        tab.reported_session_binding_nonce = Some(restored_nonce.into());
    }
    super::super::authority::record_spawn(&fixture.service, &restored_id, PROVIDER, restored_nonce)
        .unwrap();
    fixture.tab = restored_id.clone();

    crate::service::agent_runtime::transport::send(
        &fixture.service,
        &restored_id,
        &crate::service::messenger::AgentInput::Human(
            crate::service::messenger::HumanInput::composer("first restored turn"),
        ),
        None,
    )
    .unwrap();

    let session = engine::for_tab(&restored_id).unwrap();
    let settings = session.context.lock().settings.clone();
    assert_eq!(settings.model.as_deref(), Some("gpt-6-luna"));
    assert_eq!(settings.effort.as_deref(), Some("low"));
    assert_eq!(settings.service_tier, Some(tier_selection(true)));
    assert_eq!(settings.mode.as_deref(), Some("auto"));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&restored_id).unwrap();
    assert!(matches!(
        &tab.backing,
        TabBacking::ChatBackend { session_id, .. } if session_id == "own-thread"
    ));
    assert_eq!(tab.structured_runtime_mode.as_deref(), Some("auto"));
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["confirmedServiceTier"],
        json!({"selection":{"id":"priority"},"sessionId":"own-thread","bindingNonce":restored_nonce})
    );
}

#[test]
fn rejected_effort_and_runtime_mode_leave_inspected_and_persisted_settings_unchanged() {
    let (fixture, id) = runtime_fixture();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_runtime_config = Some(json!({
            "model":"prior",
            "effort":"medium",
            "mode":"auto"
        }));
        tab.reasoning_effort_identity = crate::multiplexer::types::ReasoningEffortIdentity {
            actual: Some("medium".into()),
            resolved: Some("medium".into()),
            actual_source: Some("session-owned".into()),
            ..Default::default()
        };
        tab.structured_runtime_mode = Some("auto".into());
        tab.restorable_launch.structured_runtime_mode = Some("auto".into());
    }
    crate::service::session_persist::write_session_snapshot(&fixture.service, &fixture.service.mux)
        .unwrap();
    let persisted_before =
        crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
            .snapshot()
            .unwrap();
    let persisted_before = MultiplexerState::from_session_snapshot(&persisted_before);
    let before = {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        (
            tab.agent_runtime_config.clone(),
            tab.reasoning_effort_identity.clone(),
            tab.structured_runtime_mode.clone(),
            tab.restorable_launch.structured_runtime_mode.clone(),
        )
    };

    let effort_error = fixture
        .service
        .agent_switch_model_with_restart_fallback(
            &id,
            "prior",
            Some("qa-impossible"),
            None,
            crate::service::agent_runtime::model::ModelSwitchOptions {
                service_tier: None,
                force: false,
                restart_resume: false,
            },
        )
        .unwrap_err();
    assert!(matches!(
        effort_error,
        ModelSwitchError::UnsupportedReasoningEffort { ref supported, .. }
            if supported == &["low", "high"]
    ));

    let mode_error = fixture
        .service
        .agent_switch_model_with_restart_fallback(
            &id,
            "prior",
            None,
            Some("qa-impossible"),
            crate::service::agent_runtime::model::ModelSwitchOptions {
                service_tier: None,
                force: false,
                restart_resume: false,
            },
        )
        .unwrap_err();
    assert!(matches!(
        mode_error,
        ModelSwitchError::UnsupportedRuntimeMode { ref supported, .. }
            if supported == &["approval-required", "auto-accept-edits", "auto", "full-access"]
    ));

    let after = {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        (
            tab.agent_runtime_config.clone(),
            tab.reasoning_effort_identity.clone(),
            tab.structured_runtime_mode.clone(),
            tab.restorable_launch.structured_runtime_mode.clone(),
        )
    };
    assert_eq!(after, before);
    crate::service::session_persist::flush_session_persistence(&fixture.service).unwrap();
    let persisted =
        crate::session::store::StorageDeskAdapter::from_storage(fixture.storage.clone())
            .snapshot()
            .unwrap();
    let restored = MultiplexerState::from_session_snapshot(&persisted);
    let restored = restored.get_tab(&id).unwrap();
    let persisted_before = persisted_before.get_tab(&id).unwrap();
    assert_eq!(
        restored.agent_runtime_config,
        persisted_before.agent_runtime_config
    );
    assert_eq!(
        restored.reasoning_effort_identity,
        persisted_before.reasoning_effort_identity
    );
    assert_eq!(
        restored.structured_runtime_mode,
        persisted_before.structured_runtime_mode
    );
    assert_eq!(
        restored.restorable_launch.structured_runtime_mode,
        persisted_before.restorable_launch.structured_runtime_mode
    );
}

#[test]
fn tab_stop_interrupts_a_working_runtime_and_keeps_the_tab_open() {
    let (fixture, id) = runtime_fixture();
    crate::async_runtime::block_on(async {
        let session = engine::ensure(&fixture.service, &id).await.unwrap();
        let epoch = session.context.lock().epoch;
        super::super::subscriber::publish_events(
            &fixture.service,
            &session,
            epoch,
            vec![
                codeterm_plugin::agent_runtime::AgentRuntimeEvent::TurnState {
                    thread_id: "own-thread".into(),
                    turn_id: Some("stop-target".into()),
                    parent_thread_id: None,
                    state: codeterm_plugin::agent_runtime::RuntimeTurnState::Working,
                    started_at: None,
                    completed_at: None,
                    detail: None,
                    status_line: None,
                },
            ],
            1,
        )
        .await
        .unwrap();
        session
            .context
            .lock()
            .active_turns
            .iter_mut()
            .find(|turn| turn.thread_id == "own-thread")
            .unwrap()
            .turn_id = "stale-stop-target".into();
        assert!(matches!(
            fixture.service.stop_decision(&id),
            crate::service::tab_stop::StopDecision::Interrupt { .. }
        ));
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .pty_id
            .is_none());
        let stopped =
            crate::service::stop_execution::stop_agent_turn(fixture.service.clone(), id.clone())
                .await
                .unwrap();
        assert_eq!(
            stopped,
            json!({"ok": true, "stopped": true, "reason": "stopped", "route": "runtime"})
        );
        assert!(session
            .completions
            .lock()
            .contains("own-thread", "stop-target"));
        assert!(session.context.lock().active_turn_id.is_none());
        {
            let mux = fixture.service.mux.read();
            let tab = mux.get_tab(&id).unwrap();
            assert!(matches!(&tab.backing, TabBacking::ChatBackend { .. }));
            assert_eq!(tab.agent_session_id.as_deref(), Some("own-thread"));
            assert!(Arc::ptr_eq(&session, &engine::for_tab(&id).unwrap()));
        }
        let idle =
            crate::service::stop_execution::stop_agent_turn(fixture.service.clone(), id.clone())
                .await
                .unwrap();
        assert_eq!(idle["stopped"], false);
        assert_eq!(idle["reason"], "not_working");
    });
}

#[test]
fn service_tier_requires_explicit_matching_runtime_confirmation() {
    for value in [true, false] {
        let requested = RuntimeSettings {
            service_tier: Some(tier_selection(value)),
            ..Default::default()
        };
        assert!(!confirms(&requested, &RuntimeSettings::default()));
        assert!(!confirms(
            &requested,
            &RuntimeSettings {
                service_tier: Some(tier_selection(!value)),
                ..Default::default()
            }
        ));
        assert!(confirms(&requested, &requested));
    }
}

fn tier_selection(fast: bool) -> codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
    codeterm_plugin::agent_runtime::RuntimeServiceTierSelection {
        id: fast.then(|| "priority".into()),
    }
}

#[test]
fn service_tier_availability_is_explicit_and_model_scoped() {
    use codeterm_plugin::agent_runtime::{RuntimeServiceTier, RuntimeServiceTierSelection};
    let selection = RuntimeServiceTierSelection {
        id: Some("advertised-tier".into()),
    };
    let mut settings = RuntimeSettings {
        model: Some("selected".into()),
        ..Default::default()
    };
    assert!(!tier_available(&settings, &selection));
    assert!(tier_available(
        &settings,
        &RuntimeServiceTierSelection { id: None }
    ));
    settings.service_tier_availability = Some(std::collections::BTreeMap::from([(
        "selected".into(),
        vec![RuntimeServiceTier {
            id: "advertised-tier".into(),
            name: "Tier".into(),
            description: String::new(),
            unavailable_reason: None,
        }],
    )]));
    assert!(tier_available(&settings, &selection));
    settings.model = Some("foreign-model".into());
    assert!(!tier_available(&settings, &selection));
    settings.model = Some("selected".into());
    settings
        .service_tier_availability
        .as_mut()
        .unwrap()
        .get_mut("selected")
        .unwrap()[0]
        .unavailable_reason = Some("blocked".into());
    assert!(!tier_available(&settings, &selection));
}

#[test]
fn unchanged_settings_confirm_without_a_new_provider_event() {
    let current = RuntimeSettings {
        model: Some("current".into()),
        service_tier: Some(tier_selection(true)),
        ..Default::default()
    };
    assert!(unchanged_settings(&current, &current).is_some());
    for changed in [
        RuntimeSettings {
            model: Some("other".into()),
            ..current.clone()
        },
        RuntimeSettings {
            service_tier: Some(tier_selection(false)),
            ..current.clone()
        },
        RuntimeSettings {
            effort: Some("high".into()),
            ..current.clone()
        },
        RuntimeSettings {
            mode: Some("auto".into()),
            ..current.clone()
        },
    ] {
        assert!(unchanged_settings(&changed, &current).is_none());
    }
    assert!(unchanged_settings(&RuntimeSettings::default(), &RuntimeSettings::default()).is_none());
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
fn control_changes_name_only_confirmed_non_model_controls_that_changed() {
    let declared = |effort: &str| codeterm_plugin::agent_runtime::RuntimeConfigOption {
        category: Some("thought_level".into()),
        id: "opaque-effort".into(),
        name: "Effort".into(),
        current_value: effort.into(),
        options: Vec::new(),
    };
    let before = RuntimeSettings {
        model: Some("same".into()),
        config_options: Some(vec![declared("medium")]),
        mode: Some("auto".into()),
        ..Default::default()
    };
    let after = RuntimeSettings {
        config_options: Some(vec![declared("high")]),
        service_tier: Some(tier_selection(true)),
        ..before.clone()
    };
    let changes = control_changes(&before, &after);
    assert_eq!(
        changes
            .iter()
            .map(|change| (
                change.control,
                change.previous.as_deref(),
                change.current.as_deref()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("effort", Some("medium"), Some("high")),
            ("speed", None, tier_selection(true).id.as_deref()),
        ]
    );
    assert!(control_changes(&before, &before).is_empty());
    let partial = RuntimeSettings {
        effort: Some("high".into()),
        ..Default::default()
    };
    let partial = control_changes(
        &RuntimeSettings {
            config_options: None,
            ..before.clone()
        },
        &partial,
    );
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].control, "effort");
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
fn confirmed_model_change_clears_a_stale_load_notice() {
    let (fixture, id) = runtime_fixture();
    let session = crate::async_runtime::block_on(engine::ensure(&fixture.service, &id)).unwrap();
    session.state.lock().notice = Some("Old requested model was not applied".into());
    super::super::projections::controls(
        &fixture.service,
        &session,
        RuntimeSettings {
            model: Some("reported-change".into()),
            ..Default::default()
        },
    );
    assert!(session.state.lock().notice.is_none());
    let mux = fixture.service.mux.read();
    assert_eq!(
        mux.get_tab(&id)
            .unwrap()
            .agent_runtime_state
            .as_ref()
            .unwrap()["notice"],
        serde_json::Value::Null
    );
}
#[path = "tests/live_selections.rs"]
mod live_selections;
