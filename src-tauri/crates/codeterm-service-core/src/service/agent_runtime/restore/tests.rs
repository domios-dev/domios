use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::Arc;

fn register_runtime(svc: &ServiceContext) {
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(
        &serde_json::json!({
            "id":"restore-runtime", "version":"1.0.0", "displayName":"Fixture",
            "reasoningEfforts":[{"id":"low","displayName":"Low"},{"id":"medium","displayName":"Medium"}],
            "capabilities":{"agentProvider":true,
                "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                    "agentRuntime":{"runtimeModes":[{"id":"auto","displayName":"Auto"}],
                "runtime":"stdio-jsonrpc", "command":{"binary":"fixture-cli","args":[]},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
            "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},"taskDelivery":"pty_input","verifySubmit":{"kind":"none"},"executionSandbox":{"platforms":["windows","linux","macos"],
                "readOnly":{"launchArgs":[],"runtimeMode":"read-only"},
                "mutating":{"launchArgs":[],"runtimeMode":"workspace-write"}}},
            "permissions":{"subprocess":{"allow":["fixture-cli"]}}
        })
        .to_string(),
    )
    .unwrap();
    let vm = Arc::new(
        codeterm_plugin::js_runtime::PluginVm::from_source(
            "restore-runtime",
            "module.exports={}".into(),
        )
        .unwrap(),
    );
    svc.agent_registry
        .register_provider(Box::new(codeterm_plugin::js_runtime::JsProvider::new(
            manifest, vm,
        )));
}

#[test]
fn restored_loaded_runtime_projects_ready_only_with_live_own_binding() {
    use codeterm_types::pty::BootStage;

    let (mut mux, id) = MultiplexerState::new();
    let nonce = uuid::Uuid::new_v4().to_string();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.session_binding_nonce = Some(nonce.clone());
    tab.reported_session_binding_nonce = Some(nonce.clone());
    tab.agent_runtime_state = Some(serde_json::json!({"state":"loaded"}));
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(
        &mux.to_session_snapshot(),
    ));
    register_runtime(&fixture.service);

    assert!(super::tab(&fixture.service, &id).unwrap());
    let prepared = fixture.service.mux.read().get_tab(&id).unwrap().clone();
    assert_eq!(prepared.boot_stage, Some(BootStage::AgentLaunching));
    assert!(prepared.agent_booting);
    assert!(matches!(prepared.backing, TabBacking::ChatBackend { .. }));
    let _live = super::super::engine::test_support::live_runtime(&prepared);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .reported_session_binding_nonce = Some("foreign-binding".into());
    assert!(super::tab(&fixture.service, &id).unwrap());
    assert_eq!(
        fixture.service.mux.read().get_tab(&id).unwrap().boot_stage,
        Some(BootStage::AgentLaunching)
    );

    let events = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = events.clone();
    fixture.ui.listen("tab-boot-stage", move |value| {
        captured.lock().push(value.clone());
    });
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&id)
        .unwrap()
        .reported_session_binding_nonce = Some(nonce);
    assert!(super::tab(&fixture.service, &id).unwrap());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.boot_stage, Some(BootStage::AgentReady));
    assert!(!tab.agent_booting);
    assert!(tab.pty_id.is_none());
    let events = events.lock();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["stage"], "agent_ready");
    assert_eq!(events[0]["session_bound"], true);
    assert_eq!(events[0]["agent_booting"], false);
}

#[test]
fn restored_unsupported_effort_uses_model_default_and_keeps_supported_mode() {
    let (mut mux, id) = MultiplexerState::new();
    let nonce = uuid::Uuid::new_v4().to_string();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.session_binding_nonce = Some(nonce);
    tab.agent_session_model = Some("saved-model".into());
    tab.agent_runtime_config = Some(serde_json::json!({
        "model":"saved-model",
        "effort":"qa-impossible",
        "mode":"auto"
    }));
    tab.reasoning_effort_identity = crate::multiplexer::types::ReasoningEffortIdentity {
        requested: Some("qa-impossible".into()),
        resolved: Some("qa-impossible".into()),
        actual: Some("qa-impossible".into()),
        actual_source: Some("session-owned".into()),
        ..Default::default()
    };
    tab.structured_runtime_mode = Some("auto".into());
    tab.restorable_launch.structured_runtime_mode = Some("auto".into());
    let snapshot = mux.to_session_snapshot();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    register_runtime(&fixture.service);

    assert!(super::tab(&fixture.service, &id).unwrap());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    let config = tab.agent_runtime_config.as_ref().unwrap();
    assert_eq!(config.get("effort"), None);
    assert_eq!(
        config.get("mode").and_then(|mode| mode.as_str()),
        Some("auto")
    );
    assert!(tab.reasoning_effort_identity.is_empty());
    assert_eq!(tab.structured_runtime_mode.as_deref(), Some("auto"));
    assert_eq!(
        tab.restorable_launch.structured_runtime_mode.as_deref(),
        Some("auto")
    );
    let settings = super::super::model::settings_from_config(config);
    assert_eq!(settings.effort, None);
    assert_eq!(settings.mode.as_deref(), Some("auto"));
}

#[test]
fn restored_read_only_runtime_mode_is_preserved_from_bound_sandbox_capability() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.agent_runtime_config = Some(serde_json::json!({
        "model":"saved-model",
        "mode":"read-only"
    }));
    tab.structured_runtime_mode = Some("read-only".into());
    tab.restorable_launch.structured_runtime_mode = Some("read-only".into());
    tab.restorable_launch.runtime_launch_options =
        Some(crate::multiplexer::types::RestorableRuntimeLaunchOptions {
            provider_id: "restore-runtime".into(),
            binding_nonce: "saved-binding".into(),
            mode: "read-only".into(),
            ..Default::default()
        });
    let snapshot = mux.to_session_snapshot();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    register_runtime(&fixture.service);

    assert!(super::tab(&fixture.service, &id).unwrap());
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(matches!(tab.backing, TabBacking::ChatBackend { .. }));
    assert_eq!(
        tab.agent_runtime_config.as_ref().unwrap()["mode"],
        "read-only"
    );
    assert_eq!(tab.structured_runtime_mode.as_deref(), Some("read-only"));
    assert_eq!(
        tab.restorable_launch.structured_runtime_mode.as_deref(),
        Some("read-only")
    );
    assert_eq!(
        tab.restorable_launch
            .runtime_launch_options
            .as_ref()
            .map(|options| options.mode.as_str()),
        Some("read-only")
    );
}

#[test]
fn restored_unsupported_runtime_mode_refuses_to_materialize_without_mutating_saved_identity() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.agent_runtime_config = Some(serde_json::json!({
        "model":"saved-model",
        "effort":"medium",
        "mode":"qa-impossible"
    }));
    tab.structured_runtime_mode = Some("qa-impossible".into());
    tab.restorable_launch.structured_runtime_mode = Some("qa-impossible".into());
    let snapshot = mux.to_session_snapshot();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    register_runtime(&fixture.service);
    let before = {
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        (
            tab.agent_runtime_config.clone(),
            tab.structured_runtime_mode.clone(),
            tab.restorable_launch.structured_runtime_mode.clone(),
            tab.restorable_launch.runtime_launch_options.clone(),
        )
    };

    let error = super::tab(&fixture.service, &id).unwrap_err();
    assert!(error.contains("qa-impossible"));
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert!(matches!(tab.backing, TabBacking::Pty));
    assert_eq!(
        (
            tab.agent_runtime_config.clone(),
            tab.structured_runtime_mode.clone(),
            tab.restorable_launch.structured_runtime_mode.clone(),
            tab.restorable_launch.runtime_launch_options.clone(),
        ),
        before
    );
}

#[test]
fn restoring_many_tabs_does_not_wait_for_desk_persistence_barrier() {
    let (mut mux, first_id) = MultiplexerState::new();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let mut ids = vec![first_id];
    for _ in 0..2 {
        ids.push(mux.create_tab(&workspace_id).unwrap());
    }
    for id in &ids {
        let tab = mux.get_tab_mut(id).unwrap();
        tab.agent_type = Some("restore-runtime".into());
        tab.agent_runtime_config = Some(serde_json::json!({
            "model":"saved-model",
            "effort":"qa-impossible"
        }));
    }
    let snapshot = mux.to_session_snapshot();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    register_runtime(&fixture.service);
    let (persistence_started, release_persistence, worker) =
        crate::service::session_persist::persistence_test_support::install_blocked_test_worker(
            &fixture.service,
        );
    let (restored_tx, restored_rx) = std::sync::mpsc::channel();
    let (restored_without_persistence, persistence_was_requested) = std::thread::scope(|scope| {
        let restore_thread = scope.spawn(|| {
            super::all(&fixture.service);
            let _ = restored_tx.send(());
        });

        let restored_without_persistence = restored_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .is_ok();
        let persistence_was_requested = persistence_started.try_recv().is_ok();
        let _ = release_persistence.send(());
        restore_thread.join().unwrap();
        (restored_without_persistence, persistence_was_requested)
    });
    crate::service::session_persist::persistence_test_support::stop_installed_test_worker(&worker);

    assert!(restored_without_persistence);
    assert!(!persistence_was_requested);
    let mux = fixture.service.mux.read();
    for id in ids {
        let tab = mux.get_tab(&id).unwrap();
        assert!(matches!(tab.backing, TabBacking::ChatBackend { .. }));
        assert_eq!(
            tab.agent_runtime_config.as_ref().unwrap().get("effort"),
            None
        );
    }
}

#[test]
fn legacy_pty_snapshot_runtime_materializes_same_thread_with_exact_authority_for_all_roles() {
    use crate::multiplexer::types::AgentRole;
    for (role, profile) in [
        (None, None),
        (Some(AgentRole::Manager), Some("general")),
        (Some(AgentRole::Manager), Some("orchestrator")),
        (Some(AgentRole::Manager), Some("planner")),
        (Some(AgentRole::Worker), None),
    ] {
        let (mut mux, id) = MultiplexerState::new();
        let nonce = uuid::Uuid::new_v4().to_string();
        let tab = mux.get_tab_mut(&id).unwrap();
        tab.agent_type = Some("restore-runtime".into());
        tab.agent_session_id = Some("saved-thread".into());
        tab.session_binding_nonce = Some(nonce.clone());
        tab.role = role;
        tab.role_profile = profile.map(str::to_string);
        let snapshot = mux.to_session_snapshot();
        let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
        register_runtime(&fixture.service);
        assert!(matches!(
            fixture.service.mux.read().get_tab(&id).unwrap().backing,
            TabBacking::Pty
        ));
        assert!(super::tab(&fixture.service, &id).unwrap());
        let mux = fixture.service.mux.read();
        let tab = mux.get_tab(&id).unwrap();
        assert!(
            matches!(&tab.backing, TabBacking::ChatBackend { plugin_id, session_id } if plugin_id == "restore-runtime" && session_id == "saved-thread")
        );
        assert_eq!(tab.agent_session_id.as_deref(), Some("saved-thread"));
        assert_eq!(tab.session_binding_nonce.as_deref(), Some(nonce.as_str()));
        assert_eq!(tab.role, role);
        assert_eq!(tab.role_profile.as_deref(), profile);
        assert!(tab.pty_id.is_none());
        drop(mux);
        assert!(super::super::authority::verifies(
            &fixture.service,
            &id,
            "restore-runtime",
            &nonce
        ));
        assert!(!super::super::authority::verifies(
            &fixture.service,
            &id,
            "restore-runtime",
            "foreign-nonce"
        ));
        super::super::authority::revoke(&id);
    }
}

#[test]
fn restore_does_not_convert_undeclared_or_live_launches() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    assert!(!materialize(tab, true));
    tab.restored_from_snapshot = true;
    assert!(!materialize(tab, false));
    assert!(matches!(tab.backing, TabBacking::Pty));
}

#[test]
fn retained_authority_for_same_tab_id_does_not_collide_across_distinct_bindings() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    let snapshot = mux.to_session_snapshot();
    let first = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    let second = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    let first_nonce = uuid::Uuid::new_v4().to_string();
    let second_nonce = uuid::Uuid::new_v4().to_string();
    for (fixture, nonce) in [(&first, &first_nonce), (&second, &second_nonce)] {
        register_runtime(&fixture.service);
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .session_binding_nonce = Some(nonce.clone());
        assert!(super::tab(&fixture.service, &id).unwrap());
        fixture
            .service
            .mux
            .write()
            .get_tab_mut(&id)
            .unwrap()
            .restored_from_snapshot = false;
    }
    for (fixture, nonce, foreign) in [
        (&first, &first_nonce, &second_nonce),
        (&second, &second_nonce, &first_nonce),
    ] {
        assert!(super::super::authority::verifies(
            &fixture.service,
            &id,
            "restore-runtime",
            nonce
        ));
        assert!(!super::super::authority::verifies(
            &fixture.service,
            &id,
            "restore-runtime",
            foreign
        ));
    }
    super::super::authority::revoke(&id);
}

#[test]
fn recovered_hidden_worker_materializes_without_changing_worker_identity() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("worker-thread".into());
    tab.worker_id = Some("exact-worker-id".into());
    tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
    tab.hidden = true;
    tab.restored_from_snapshot = true;
    assert!(materialize(tab, true));
    assert!(tab.hidden);
    assert_eq!(tab.worker_id.as_deref(), Some("exact-worker-id"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("worker-thread"));
    assert_eq!(tab.role, Some(crate::multiplexer::types::AgentRole::Worker));
}

#[test]
fn restored_legacy_worker_materialization_preserves_saved_attempt_binding() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("restore-runtime".into());
    tab.agent_session_id = Some("saved-thread".into());
    tab.role = Some(crate::multiplexer::types::AgentRole::Worker);
    let binding = crate::multiplexer::types::TaskBinding {
        plan_id: Some("saved-plan".into()),
        task_id: Some("saved-task".into()),
        attempt_id: Some("saved-attempt".into()),
        ..Default::default()
    };
    tab.task_binding = Some(binding.clone());
    let snapshot = mux.to_session_snapshot();
    let fixture = ServiceFixture::new(MultiplexerState::from_session_snapshot(&snapshot));
    register_runtime(&fixture.service);
    assert!(super::tab(&fixture.service, &id).unwrap());
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .task_binding
            .as_ref(),
        Some(&binding)
    );
    super::super::authority::revoke(&id);
}

#[test]
fn cloud_launch_record_never_materializes_a_local_runtime() {
    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("opaque-provider".into());
    tab.restored_from_snapshot = true;
    tab.restorable_launch.execution_mode = Some("opaque-cloud".into());
    assert!(!materialize(tab, true));
    assert_eq!(
        tab.restorable_launch.execution_mode.as_deref(),
        Some("opaque-cloud")
    );
}
