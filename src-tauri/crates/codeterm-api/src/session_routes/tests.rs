use super::*;

// The daemon serves REST on a multi-thread runtime; plugin dispatch refuses a current-thread one.
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
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
        "module.exports = {{findSession(id) {{return {{sessionId:id,projectPath:{},filePath:'opaque-session-record'}};}}}};",
        serde_json::to_string(&fixture.temp.path().display().to_string()).unwrap(),
    );
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source("resume-fixture", source).unwrap()),
        )));
    let registry = fixture.service.agent_registry.clone();
    let metadata =
        codeterm_plugin::js_runtime::spawn_blocking(move || registry.find_session("owned-session"))
            .await
            .unwrap()
            .expect("fixture session must resolve before testing resume refusal");
    assert_eq!(metadata.session_id, "owned-session");
    assert_eq!(metadata.provider, "resume-fixture");
    assert_eq!(
        metadata.project_path.as_deref(),
        Some(fixture.temp.path().to_str().unwrap())
    );
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
            .contains(&format!("domios agent restart --tab {tab_id}")),
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
        "domios agent set-orchestrator --tab tab-resumed"
    );
    assert_eq!(
        actions["relink"],
        "domios agent link --parent-tab <orchestrator-tab> --tab tab-resumed"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn search_sessions_api_matches_windows_leading_slash_and_backslash_project_paths() {
    use codeterm_service_core::agent::provider::{
        AgentLaunchParams, AgentProvider, ReadinessStrategy, SessionContent, SessionContentQuery,
        SessionContentSource, SessionMessage, SessionMetadata, TaskDelivery,
    };
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;

    struct WindowsPathStub;
    impl AgentProvider for WindowsPathStub {
        fn id(&self) -> &str {
            "win-path-stub"
        }
        fn display_name(&self) -> &str {
            "Windows Path Stub"
        }
        fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> ReadinessStrategy {
            ReadinessStrategy::FixedDelay {
                duration: std::time::Duration::from_secs(0),
            }
        }
        fn task_delivery(&self) -> TaskDelivery {
            TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn detect_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            None
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn enumerate_sessions(&self) -> Vec<SessionMetadata> {
            vec![SessionMetadata {
                session_id: "acp-win-session".into(),
                provider: self.id().into(),
                project_path: Some("/C:/Developer/codeterm/codeterm".into()),
                title: Some("Windows ACP session".into()),
                summary: None,
                file_path: String::new(),
                transcript_path: None,
                created_ts: Some(1_700_000_000),
                modified_ts: Some(1_700_000_000),
            }]
        }
        fn session_content_source(&self) -> SessionContentSource {
            SessionContentSource::SharedStore
        }
        fn read_session_content(&self, _: &str, _: &SessionContentQuery) -> Option<SessionContent> {
            Some(SessionContent {
                messages: vec![SessionMessage {
                    role: "user".into(),
                    text: "windows path normalization needle".into(),
                    ts: Some(1_700_000_000),
                    is_meta: false,
                }],
                truncated: false,
            })
        }
    }

    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(WindowsPathStub));
    let state = crate::test_support::app_state(&fixture);

    for project in [
        "C:/Developer/codeterm/codeterm",
        r"C:\Developer\codeterm\codeterm",
        "/C:/Developer/codeterm/codeterm",
    ] {
        let Json(body) = search_sessions_api(
            AxumState(state.clone()),
            HeaderMap::new(),
            axum::extract::Query(SearchSessionsQuery {
                q: "needle".into(),
                provider: Some("win-path-stub".into()),
                project: Some(project.into()),
                limit: Some(5),
                max_snippets: Some(3),
            }),
        )
        .await
        .expect("search_sessions_api should succeed");
        let hits = body["hits"].as_array().expect("hits array");
        assert_eq!(hits.len(), 1, "expected hit for project={project}");
        assert_eq!(hits[0]["meta"]["session_id"], "acp-win-session");
    }
}
