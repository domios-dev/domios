use super::*;
use crate::{multiplexer::state::MultiplexerState, service::test_fixture::ServiceFixture};
use std::sync::Arc;

fn register_runtime(svc: &ServiceContext) {
    let manifest = codeterm_plugin::js_runtime::PluginManifest::from_json_str(
        &serde_json::json!({
            "id":"restore-runtime", "version":"1.0.0", "displayName":"Fixture",
            "capabilities":{"agentProvider":true,
                "chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
                "agentRuntime":{
                "runtime":"stdio-jsonrpc", "command":{"binary":"fixture-cli","args":[]},
                "requestMethods":{"initialize":["handshake"]},"notificationMethods":[]}},
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
