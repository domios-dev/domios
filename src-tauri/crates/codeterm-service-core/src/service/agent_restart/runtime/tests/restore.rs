use super::*;
use crate::agent::chat_types::ChatMsg;

#[test]
fn fresh_after_load_refusal_preserves_profile_settings_and_archived_transcript() {
    let methods = [
        "initialize",
        "openThread",
        "resumeThread",
        "submitTurn",
        "control",
    ]
    .into_iter()
    .map(|kind| (kind.to_owned(), json!([kind])))
    .collect::<serde_json::Map<String, Value>>();
    let fixture = RuntimeFixture::prepared_with_runtime(
        SOURCE,
        true,
        json!({"requestMethods":methods,"launchProfiles":{"opaque-profile":{"binary":"node","args":[]}}}),
    );
    let old_thread = uuid::Uuid::new_v4().to_string();
    let config = json!({"systemPrompt":PROMPT,"model":"supplied-model","effort":"opaque-effort","launchProfile":"opaque-profile"});
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&fixture.tab).unwrap();
        tab.agent_session_id = Some(old_thread.clone());
        tab.backing = TabBacking::ChatBackend {
            plugin_id: PROVIDER.into(),
            session_id: old_thread.clone(),
        };
        tab.agent_runtime_config = Some(config.clone());
    }
    agent_runtime::transport::launch(&fixture.service, PROVIDER, &fixture.tab, config).unwrap();
    let session = agent_runtime::engine::for_tab(&fixture.tab).unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "saved-turn".into(),
        kind: "user".into(),
        content: "opaque earlier turn".into(),
        ..Default::default()
    });
    crate::async_runtime::block_on(agent_runtime::engine::persist_transcript(&session));
    let epoch = session.state.lock().epoch;
    session.state.lock().degrade(
        epoch,
        &agent_runtime::model::RuntimeFailure::SessionLoadRefused("opaque detail".into()),
    );
    agent_runtime::engine::publish(&fixture.service, &fixture.tab, &session);
    let before = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .agent_runtime_config
        .clone();
    let outcome = fixture
        .service
        .agent_restart_observed_with_options(
            &fixture.tab,
            RestartOptions {
                fresh: true,
                force: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(outcome.agent_ready);
    assert_ne!(outcome.session_id.as_deref(), Some(old_thread.as_str()));
    let requests = fixture.requests();
    let opens = requests
        .iter()
        .filter(|request| request["method"] == "openThread")
        .collect::<Vec<_>>();
    assert_eq!(opens.len(), 1);
    assert_eq!(opens[0]["params"]["settings"]["model"], "supplied-model");
    assert_eq!(opens[0]["params"]["settings"]["effort"], "opaque-effort");
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&fixture.tab)
            .unwrap()
            .agent_runtime_config,
        before
    );
    let (rows, _) =
        crate::service::tab_chat::tab_chat_snapshot(&fixture.service, &fixture.tab, 0).unwrap();
    assert_eq!(rows[0].content, "opaque earlier turn");
    assert!(rows[0]
        .id
        .starts_with(&format!("provider-session:{old_thread}:")));
    assert_eq!(
        rows[1].system_subtype.as_deref(),
        Some("context_epoch_boundary")
    );
    let archive = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .unwrap()
        .context_epoch_archives[0]
        .clone();
    assert_eq!(
        archive.transport,
        crate::multiplexer::types::ContextEpochTransport::ChatBackend
    );
}
