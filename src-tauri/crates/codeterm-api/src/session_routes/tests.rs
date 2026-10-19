use super::*;

#[tokio::test]
async fn sessions_resume_refuses_a_degraded_bound_tab_with_a_recovery_command() {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use codeterm_service_core::multiplexer::{state::MultiplexerState, types::TabBacking};
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use serde_json::json;
    use std::sync::Arc;

    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("resume-fixture".into());
    tab.agent_session_id = Some("owned-session".into());
    tab.session_binding_nonce = Some("owned-nonce".into());
    tab.reported_session_binding_nonce = Some("owned-nonce".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "resume-fixture".into(),
        session_id: "owned-session".into(),
    };
    tab.boot_stage = Some(codeterm_types::pty::BootStage::SpawnFailed);
    tab.agent_runtime_state = Some(json!({"state":"degraded","epoch":5,"reason":"opaque failure"}));
    let fixture = ServiceFixture::new(mux);
    fixture.set_agent_full_access(false);
    let manifest = PluginManifest::from_json_str(
        &json!({
            "id":"resume-fixture","version":"1.0.0","displayName":"Fixture",
            "capabilities":{"agentProvider":true}, "permissions":{}
        })
        .to_string(),
    )
    .unwrap();
    let source = format!(
        "module.exports = {{findSession(id) {{return {{session_id:id,project_path:{},file_path:'opaque-session-record'}};}}}};",
        serde_json::to_string(&fixture.temp.path().display().to_string()).unwrap(),
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source("resume-fixture", source).unwrap()),
        )));
    let state = crate::test_support::app_state(&fixture);
    let error = resume_session_api(
        AxumState(state),
        HeaderMap::new(),
        Path("owned-session".into()),
        OptionalJson(None),
    )
    .await
    .unwrap_err();
    assert_eq!(error.0, StatusCode::CONFLICT);
    assert!(
        error
            .1
            .contains(&format!("codeterm agent restart --tab {tab_id}")),
        "{error:?}"
    );
    let mux = fixture.service.mux.read();
    let snapshot = mux.snapshot();
    assert_eq!(snapshot.tabs.len(), 1);
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("owned-session"));
    assert_eq!(
        tab.boot_stage,
        Some(codeterm_types::pty::BootStage::SpawnFailed)
    );
}

#[test]
fn sessions_resume_restores_role_without_replaying_starter() {
    let worker = ResumableIdentity {
        role: "Worker".into(),
        role_profile: None,
        parent_tab_id: Some("parent".into()),
        can_spawn: false,
    };
    assert!(restorable_identity(Some(worker.clone()), |parent| parent == "parent").is_some());
    assert!(restorable_identity(Some(worker), |_| false).is_none());

    let orchestrator = ResumableIdentity {
        role: "Orchestrator".into(),
        role_profile: None,
        parent_tab_id: None,
        can_spawn: true,
    };
    assert!(restorable_identity(Some(orchestrator), |_| false).is_some());
}

#[test]
fn restored_worker_identity_has_no_historical_task_input() {
    let params = codeterm_service_core::service::SpawnAgentParams {
        role: Some("Worker".into()),
        parent_tab_id: Some("orchestrator".into()),
        can_spawn: Some(false),
        task: None,
        starter_prompt: codeterm_service_core::agent::provider::StarterPrompt::NoStarter,
        task_id: None,
        plan_task: None,
        resume_session_id: Some("owned-session".into()),
        ..Default::default()
    };
    assert_eq!(params.task, None);
    assert_eq!(
        params.starter_prompt,
        codeterm_service_core::agent::provider::StarterPrompt::NoStarter
    );
    assert_eq!(params.task_id, None);
    assert_eq!(params.plan_task, None);
    let restored_context = codeterm_service_core::service::spawn_onboarding_task(&params).unwrap();
    assert!(!restored_context.contains("historical"));
    assert!(!restored_context.contains("one assigned brief"));

    let fresh = codeterm_service_core::service::SpawnAgentParams {
        role: Some("Worker".into()),
        task: Some("one assigned brief".into()),
        starter_prompt: codeterm_service_core::agent::provider::StarterPrompt::AssignedTask,
        ..Default::default()
    };
    let fresh_context = codeterm_service_core::service::spawn_onboarding_task(&fresh).unwrap();
    assert_eq!(fresh_context.matches("one assigned brief").count(), 1);
}

#[test]
fn roleless_resume_reports_exact_promotion_and_relink_actions() {
    let actions = roleless_actions("tab-resumed");
    assert_eq!(
        actions["promotion"],
        "codeterm agent set-orchestrator --tab tab-resumed"
    );
    assert_eq!(
        actions["relink"],
        "codeterm agent link --parent-tab <orchestrator-tab> --tab tab-resumed"
    );
}
