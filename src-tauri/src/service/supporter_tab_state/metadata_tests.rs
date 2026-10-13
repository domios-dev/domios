use super::*;

#[test]
fn tab_state_context_header_fields_present() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let pty_id = format!("tab-state-context-{}", uuid::Uuid::new_v4().simple());
    mux.link_pty(&tab_id, pty_id.clone());
    {
        let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
        tab.agent_type = Some("tab-state-provider".into());
        tab.agent_set_at = Some(Instant::now());
        tab.agent_requested_model = Some("requested-model".into());
        tab.agent_session_model = Some("session-model".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.role = Some(codeterm_types::agent::AgentRole::Worker);
        tab.spawned_by_tab_id = Some("creator-tab".into());
        tab.task_binding = Some(crate::multiplexer::types::TaskBinding {
            plan_id: Some("plan-id".into()),
            task_id: Some("task-id".into()),
            attempt_id: Some("attempt-id".into()),
            spawn_handle: None,
            bound_at: 1,
        });
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("tab-state-provider"),
    ));
    let model = StubModel::answering(Ok(choice(WORKING_NORMALLY, 0.91)));
    let (mut host, _, _) = StubHost::new("opaque terminal screen", model);
    host.metadata_service = Some(fixture.service.clone());
    let (request, metadata) = prepare_observation(
        &host,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "ready-verdict",
    )
    .expect("stub transport provides the screen and runtime context");
    assert_eq!(request.state["screen"], "opaque terminal screen");
    assert_eq!(host.reads.load(Ordering::SeqCst), 1);
    assert_eq!(host.writes.load(Ordering::SeqCst), 0);
    assert_eq!(request.state["context"], metadata.context);

    assert_eq!(metadata.context["provider_capability"]["transport"], "pty");
    assert_eq!(metadata.context["requested_model"], "requested-model");
    assert_eq!(metadata.context["session_model"], "session-model");
    assert_eq!(metadata.context["role"], "Worker");
    assert_eq!(metadata.context["plan_id"], "plan-id");
    assert_eq!(metadata.context["task_id"], "task-id");
    assert_eq!(metadata.context["task_binding"]["attempt_id"], "attempt-id");
    assert_eq!(metadata.context["spawning_tab_id"], "creator-tab");
    assert!(metadata.context["provider_capability"]["display_name"].is_string());
    for key in [
        "seconds_since_spawn",
        "seconds_since_last_output",
        "structural_verdict",
        "pending_prompt_kind",
        "screen_changed",
    ] {
        assert!(metadata.context.get(key).is_some(), "missing {key}");
    }
}

struct TranscriptBackend;

impl crate::chatbackend::ChatBackendProvider for TranscriptBackend {
    fn id(&self) -> &str {
        "tab-state-chat-fixture"
    }

    fn spawnable(&self) -> bool {
        true
    }

    fn open_session(&self, _config: Value) -> Result<String, crate::chatbackend::ChatBackendError> {
        Ok("session".into())
    }

    fn send_message(
        &self,
        _session_id: &str,
        _text: &str,
    ) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn poll(
        &self,
        _session_id: &str,
        _cursor: Option<&str>,
    ) -> Result<crate::agent::chat_types::StructuredChat, crate::chatbackend::ChatBackendError>
    {
        Ok(crate::agent::chat_types::StructuredChat {
            messages: vec![crate::agent::chat_types::ChatMsg {
                id: "assistant-message".into(),
                kind: "assistant".into(),
                content: "assistant transcript evidence".into(),
                ..Default::default()
            }],
            ..Default::default()
        })
    }

    fn close_session(&self, _session_id: &str) -> Result<(), crate::chatbackend::ChatBackendError> {
        Ok(())
    }

    fn list_models(
        &self,
    ) -> Result<Vec<crate::agent::provider::ModelInfo>, crate::chatbackend::ChatBackendError> {
        Ok(Vec::new())
    }
}

#[test]
fn tab_state_chat_backend_reads_transcript_by_capability() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    {
        let tab = mux.get_tab_mut(&tab_id).expect("fixture tab");
        tab.agent_type = Some("tab-state-chat-fixture".into());
        tab.agent_session_id = Some("session".into());
        tab.session_binding_nonce = Some("chat-session-nonce".into());
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "tab-state-chat-fixture".into(),
            session_id: "session".into(),
        };
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture
        .service
        .chatbackend_registry
        .register("tab-state-chat-fixture", Arc::new(TranscriptBackend));
    let screen = fixture
        .service
        .tab_transport(&tab_id)
        .expect("chat backend transport")
        .read_screen(false)
        .expect("transcript read")["text"]
        .as_str()
        .expect("transcript text")
        .to_owned();
    let metadata = <ServiceContext as ShadowHost>::metadata(
        &fixture.service,
        &tab_id,
        ObservationTrigger::SpawnReady,
        "ready-verdict",
        &screen,
    )
    .expect("runtime context");

    assert!(screen.contains("assistant transcript evidence"));
    assert_eq!(
        metadata.context["provider_capability"]["transport"],
        "chat_backend"
    );
}

#[test]
fn tab_state_never_answers_prompt() {
    let (mut mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    mux.get_tab_mut(&tab_id)
        .expect("fixture tab")
        .pending_prompt = Some(codeterm_types::pty::PendingPrompt {
        question: "opaque prompt".into(),
        options: vec![codeterm_types::pty::PromptOption {
            label: "Opaque option".into(),
            value: "1".into(),
        }],
        sections: None,
        multi_select: None,
        submit_available: None,
        selected_values: None,
    });
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    delivery::deliver(
        &fixture.service,
        &tab_id,
        &TabStateMetadata {
            declared_safe_choice: None,
            tab_snapshot: None,
            process_tree_key: None,
            envelope: None,
            context: Value::Null,
            session_key: "fixture-session".into(),
            title: "worker".into(),
            spawning_tab_id: None,
            parent_tab_id: None,
            plan_id: None,
            task_id: None,
            attempt_id: None,
            prompt_options: Vec::new(),
        },
        0,
        ObservationTrigger::BusyNoOutput,
        TRUST_OR_PERMISSION_PROMPT,
        0.92,
        SignalAction::StrongAction,
    );
    let mux = fixture.service.mux.read();
    assert!(mux
        .get_tab(&tab_id)
        .is_some_and(|tab| tab.pending_prompt.is_some()));
}
