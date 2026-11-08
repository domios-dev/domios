use super::*;
use crate::{
    agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery},
    multiplexer::state::MultiplexerState,
    service::{agent_runtime::engine::test_support, test_fixture::ServiceFixture},
};
use codeterm_plugin::agent_runtime::AgentRuntimeCapability;
use std::{path::PathBuf, time::Duration};

struct Provider {
    id: String,
    stored: bool,
    refuse_resume: bool,
}

#[async_trait::async_trait]
impl AgentProvider for Provider {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        "Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
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
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        panic!("A snapshot must never search for another session")
    }
    fn session_file_path(&self, cwd: &str, session: &str) -> Option<PathBuf> {
        self.stored
            .then(|| PathBuf::from(cwd).join(format!("{session}.runtime")))
    }
    fn agent_runtime(&self) -> Option<AgentRuntimeCapability> {
        Some(
            serde_json::from_value(serde_json::json!({
                "storedHistory":self.stored, "runtime":"stdio-jsonrpc",
                "command":{"binary":"fixture-engine","args":[]},
                "requestMethods":{"initialize":["fixture/initialize"]},"notificationMethods":[]
            }))
            .unwrap(),
        )
    }
    async fn runtime_plan(
        &self,
        _: &RuntimeContext,
        _: &codeterm_plugin::agent_runtime::RuntimeOperation,
    ) -> Result<
        codeterm_plugin::agent_runtime::RuntimePlan,
        codeterm_plugin::agent_runtime::RuntimeFailure,
    > {
        Ok(
            serde_json::from_value(serde_json::json!({"action":{"kind":"local"},"state":null}))
                .unwrap(),
        )
    }
    async fn runtime_response(
        &self,
        _: &RuntimeContext,
        op: &codeterm_plugin::agent_runtime::RuntimeOperation,
        _: &serde_json::Value,
        _: &codeterm_plugin::agent_runtime::RpcReply,
    ) -> Result<
        codeterm_plugin::agent_runtime::RuntimeStep,
        codeterm_plugin::agent_runtime::RuntimeFailure,
    > {
        use codeterm_plugin::agent_runtime::*;
        let RuntimeOperation::ResumeThread {
            thread_id,
            settings,
            ..
        } = op
        else {
            panic!("Only resume is expected")
        };
        Ok(if self.refuse_resume {
            RuntimeStep::Refused {
                reason: codeterm_plugin::agent_runtime::RuntimeFailure::new(
                    RuntimeFailureCode::ProviderRefused,
                    "opaque resume cause",
                ),
                before_acceptance: true,
            }
        } else {
            RuntimeStep::Complete {
                result: RuntimeResult::ThreadLoaded {
                    thread_id: thread_id.clone(),
                    settings: settings.clone(),
                },
                events: None,
                notification: None,
            }
        })
    }
    fn runtime_stored_history_available(&self, cwd: &str, session: &str) -> bool {
        self.session_file_path(cwd, session)
            .is_some_and(|path| path.is_file())
    }
    fn runtime_stored_history(&self, session: &str, records: &str) -> Result<Vec<ChatMsg>, String> {
        let value: serde_json::Value =
            serde_json::from_str(records).map_err(|error| error.to_string())?;
        if value["session"].as_str() != Some(session) {
            return Err("Stored runtime transcript belongs to another session".into());
        }
        serde_json::from_value(value["messages"].clone()).map_err(|error| error.to_string())
    }
}

fn fixture(stored: bool) -> (ServiceFixture, tempfile::TempDir, String, String) {
    let cwd = tempfile::tempdir().unwrap();
    let provider = format!("snapshot-fixture-{}", uuid::Uuid::new_v4());
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let tab_id = mux.create_tab(&workspace).unwrap();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some(provider.clone());
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("own-nonce".into());
    tab.reported_session_binding_nonce = Some("own-nonce".into());
    tab.cwd = Some(cwd.path().to_string_lossy().into_owned());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: provider.clone(),
        session_id: "own-thread".into(),
    };
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(Provider {
            id: provider.clone(),
            stored,
            refuse_resume: false,
        }));
    (fixture, cwd, provider, tab_id)
}

fn attach(fixture: &ServiceFixture, tab_id: &str, content: &str) -> test_support::LiveRuntime {
    let tab = fixture.service.mux.read().get_tab(tab_id).unwrap().clone();
    let adapter = fixture
        .service
        .agent_registry
        .provider_arc(tab.agent_type.as_deref().unwrap());
    let runtime = test_support::loaded_runtime(&tab, adapter);
    let session = engine::for_tab(tab_id).unwrap();
    session.cache.lock().reconcile(ChatMsg {
        id: "own-reply".into(),
        kind: "assistant".into(),
        content: content.into(),
        ..Default::default()
    });
    runtime
}

#[test]
fn runtime_snapshot_reads_own_reply_without_jsonl_even_after_process_loss() {
    let (fixture, cwd, provider, tab_id) = fixture(false);
    let _runtime = attach(&fixture, &tab_id, "opaque own reply");
    let session = engine::for_tab(&tab_id).unwrap();
    let process = session.process.close().unwrap();
    crate::async_runtime::block_on(process.finish()).unwrap();
    assert_eq!(
        super::super::snapshot_source::for_tab(&fixture.service, &tab_id).unwrap(),
        super::super::snapshot_source::SnapshotSource::Stored
    );
    let (rows, count) = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        (rows[0].id.as_str(), rows[0].content.as_str()),
        ("own-reply", "opaque own reply")
    );
    let (rows, count) = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "own-thread",
        0,
    )
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(rows[0].content, "opaque own reply");
    let (rows, count) = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "own-thread",
        1,
    )
    .unwrap();
    assert_eq!(count, 1);
    assert!(rows.is_empty());
}

#[test]
fn runtime_snapshot_rejects_foreign_scope_nonce_and_backing_without_searching() {
    let (fixture, cwd, provider, tab_id) = fixture(false);
    let _runtime = attach(&fixture, &tab_id, "private reply");
    for (provider_id, path, session) in [
        (
            provider.as_str(),
            cwd.path().to_str().unwrap(),
            "foreign-thread",
        ),
        (provider.as_str(), "foreign-cwd", "own-thread"),
        (
            "foreign-provider",
            cwd.path().to_str().unwrap(),
            "own-thread",
        ),
    ] {
        assert!(super::super::session_chat_snapshot(
            &fixture.service,
            provider_id,
            path,
            session,
            0
        )
        .is_err());
    }
    let original = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    for changed in [
        Tab {
            reported_session_binding_nonce: Some("foreign-nonce".into()),
            ..original.clone()
        },
        Tab {
            session_binding_nonce: Some("new-nonce".into()),
            ..original.clone()
        },
        Tab {
            agent_session_id: Some("other-thread".into()),
            ..original.clone()
        },
        Tab {
            backing: TabBacking::ChatBackend {
                plugin_id: provider.clone(),
                session_id: "other-thread".into(),
            },
            ..original.clone()
        },
    ] {
        *fixture.service.mux.write().get_tab_mut(&tab_id).unwrap() = changed;
        assert!(super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).is_err());
    }
    *fixture.service.mux.write().get_tab_mut(&tab_id).unwrap() = original;
}

#[test]
fn runtime_session_snapshot_ignores_foreign_siblings_and_refuses_ambiguous_owners() {
    let (fixture, cwd, provider, tab_id) = fixture(false);
    let _own = attach(&fixture, &tab_id, "own reply");
    let original = fixture.service.mux.read().get_tab(&tab_id).unwrap().clone();
    let sibling = |session: &str, nonce: &str| {
        let mut mux = fixture.service.mux.write();
        let workspace = mux.find_workspace_for_tab(&tab_id).unwrap();
        let id = mux.create_tab(&workspace).unwrap();
        *mux.get_tab_mut(&id).unwrap() = Tab {
            id: id.clone(),
            agent_session_id: Some(session.into()),
            session_binding_nonce: Some(nonce.into()),
            reported_session_binding_nonce: Some(nonce.into()),
            backing: TabBacking::ChatBackend {
                plugin_id: provider.clone(),
                session_id: session.into(),
            },
            ..original.clone()
        };
        id
    };
    let foreign = sibling("foreign-thread", "foreign-nonce");
    let _foreign = attach(&fixture, &foreign, "foreign reply");
    let (rows, count) = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "own-thread",
        0,
    )
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(rows[0].content, "own reply");
    let duplicate = sibling("own-thread", "duplicate-nonce");
    let _duplicate = attach(&fixture, &duplicate, "ambiguous reply");
    let error = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "own-thread",
        0,
    )
    .unwrap_err();
    assert!(error.contains("multiple current tab owners"));
}

#[test]
fn runtime_session_snapshot_reads_exact_closed_session_with_provider_projection() {
    let (fixture, cwd, provider, tab_id) = fixture(true);
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_session_id = None;
    let rows = vec![ChatMsg {
        id: "stored-reply".into(),
        kind: "assistant".into(),
        content: "opaque stored reply".into(),
        ..Default::default()
    }];
    let path = cwd.path().join("closed-thread.runtime");
    std::fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::json!({"session":"closed-thread","messages":rows})
        ),
    )
    .unwrap();
    let (rows, count) = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "closed-thread",
        0,
    )
    .unwrap();
    assert_eq!(count, 1);
    assert_eq!(
        (rows[0].id.as_str(), rows[0].content.as_str()),
        ("stored-reply", "opaque stored reply")
    );
    std::fs::write(
        &path,
        format!(
            "{}\n",
            serde_json::json!({"session":"foreign-thread","messages":rows})
        ),
    )
    .unwrap();
    assert!(super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "closed-thread",
        0,
    )
    .is_err());
    assert!(engine::for_tab(&tab_id).is_none());
}

#[test]
fn runtime_stored_history_missing_and_corrupt_are_distinct_from_fresh_empty() {
    use super::super::runtime_stored::StoredHistoryFailure;
    let (fixture, cwd, _, tab_id) = fixture(true);
    let missing = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap_err();
    assert_eq!(
        StoredHistoryFailure::from_error(&missing),
        Some(StoredHistoryFailure::Missing)
    );
    std::fs::write(cwd.path().join("own-thread.runtime"), "not-json\n").unwrap();
    let corrupt = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap_err();
    assert_eq!(
        StoredHistoryFailure::from_error(&corrupt),
        Some(StoredHistoryFailure::Corrupt)
    );
}

#[test]
fn failed_resume_keeps_restored_failed_turn_rows_without_claiming_loaded() {
    let (fixture, cwd, provider, tab_id) = fixture(false);
    let _runtime = attach(&fixture, &tab_id, "unused fixture row");
    let session = engine::for_tab(&tab_id).unwrap();
    *session.cache.lock() = Default::default();
    session.cache.lock().reconcile(ChatMsg {
        id: "saved-user".into(),
        kind: "user".into(),
        content: "opaque original prompt".into(),
        ..Default::default()
    });
    session.cache.lock().reconcile(ChatMsg {
        id: "saved-failure".into(),
        kind: "status".into(),
        content: "opaque quota failure".into(),
        ..Default::default()
    });
    session.thread_loaded.store(false, Ordering::Release);
    let epoch = session.context.lock().epoch;
    session.state.lock().degrade(
        epoch,
        &crate::service::agent_runtime::model::RuntimeFailure::Refused(
            "opaque resume refusal".into(),
        ),
    );
    let process = session.process.close().unwrap();
    crate::async_runtime::block_on(process.finish()).unwrap();
    let expected = session.cache.lock().snapshot();
    let (rows, count) = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap();
    assert_eq!(rows, expected);
    assert_eq!(count, 2);
    let (rows, count) = super::super::session_chat_snapshot(
        &fixture.service,
        &provider,
        cwd.path().to_str().unwrap(),
        "own-thread",
        0,
    )
    .unwrap();
    assert_eq!(rows, expected);
    assert_eq!(count, 2);
    assert!(!session.thread_loaded.load(Ordering::Acquire));
    assert!(matches!(
        session.state.lock().phase,
        crate::service::agent_runtime::model::RuntimePhase::Degraded { .. }
    ));
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .session_binding_nonce = Some("foreign-nonce".into());
    assert!(super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).is_err());
}

#[test]
fn disk_transcript_survives_restart_without_replay_and_with_resume_refusal() {
    for refuse_resume in [false, true] {
        let (original, cwd, provider, tab_id) = fixture(false);
        let runtime = attach(&original, &tab_id, "unused fixture row");
        let session = engine::for_tab(&tab_id).unwrap();
        *session.cache.lock() = Default::default();
        for (id, kind, content) in [
            ("original-user", "user", "opaque original CLI body"),
            ("original-outcome", "status", "opaque saved turn outcome"),
        ] {
            session.cache.lock().reconcile(ChatMsg {
                id: id.into(),
                kind: kind.into(),
                content: content.into(),
                inbound_kind: (kind == "user").then(|| "cli".into()),
                ..Default::default()
            });
        }
        let expected = session.cache.lock().snapshot();
        crate::async_runtime::block_on(engine::persist_transcript(&session));
        let path = super::super::chatbackend_history::runtime_path(&provider, "own-thread");
        assert!(path.is_file());
        let saved = original.service.mux.read().to_session_snapshot();
        drop(runtime);
        drop(session);
        drop(original);
        assert!(engine::for_tab(&tab_id).is_none());
        let restored = ServiceFixture::new(MultiplexerState::from_session_snapshot(&saved));
        restored
            .service
            .agent_registry
            .register_provider(Box::new(Provider {
                id: provider.clone(),
                stored: false,
                refuse_resume,
            }));
        let (rows, count) = super::super::tab_chat_snapshot(&restored.service, &tab_id, 0).unwrap();
        assert_eq!(rows, expected);
        assert_eq!(count, 2);
        let restored_tab = restored
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .clone();
        for changed in [
            Tab {
                session_binding_nonce: Some("foreign-nonce".into()),
                ..restored_tab.clone()
            },
            Tab {
                backing: TabBacking::ChatBackend {
                    plugin_id: provider.clone(),
                    session_id: "foreign-thread".into(),
                },
                ..restored_tab.clone()
            },
        ] {
            *restored.service.mux.write().get_tab_mut(&tab_id).unwrap() = changed;
            assert!(super::super::tab_chat_snapshot(&restored.service, &tab_id, 0).is_err());
        }
        *restored.service.mux.write().get_tab_mut(&tab_id).unwrap() = restored_tab.clone();
        let runtime = test_support::loaded_runtime(
            &restored_tab,
            restored.service.agent_registry.provider_arc(&provider),
        );
        let session = engine::for_tab(&tab_id).unwrap();
        session.thread_loaded.store(false, Ordering::Release);
        assert!(session.cache.lock().snapshot().is_empty());
        session.state.lock().agent_name = Some("Opaque agent".into());
        let process = session.process.lock().clone().unwrap();
        let result = crate::async_runtime::block_on(engine::resume_for_test(
            &restored.service,
            &provider,
            &session,
            &process,
            codeterm_plugin::agent_runtime::RuntimeOperation::ResumeThread {
                thread_id: "own-thread".into(),
                instructions: String::new(),
                settings: Default::default(),
            },
        ));
        if refuse_resume {
            let failure = result.unwrap_err();
            assert!(failure.to_string().contains(
                "Opaque agent could not resume the session: ProviderRefused: opaque resume cause"
            ));
            let epoch = session.context.lock().epoch;
            session.state.lock().degrade(epoch, &failure);
        } else {
            assert!(matches!(
                result.unwrap(),
                codeterm_plugin::agent_runtime::RuntimeResult::ThreadLoaded { .. }
            ));
            session.thread_loaded.store(true, Ordering::Release);
        }
        assert!(
            session.cache.lock().snapshot().is_empty(),
            "The agent must not replay any rows"
        );
        for result in [
            super::super::tab_chat_snapshot(&restored.service, &tab_id, 0),
            super::super::session_chat_snapshot(
                &restored.service,
                &provider,
                cwd.path().to_str().unwrap(),
                "own-thread",
                0,
            ),
        ] {
            let (rows, count) = result.unwrap();
            assert_eq!(rows, expected);
            assert_eq!(count, 2);
        }
        assert_eq!(
            session.thread_loaded.load(Ordering::Acquire),
            !refuse_resume
        );
        if refuse_resume {
            assert!(matches!(
                session.state.lock().phase,
                crate::service::agent_runtime::model::RuntimePhase::Degraded { .. }
            ));
        }
        crate::async_runtime::block_on(engine::restore_transcript(&session)).unwrap();
        assert_eq!(session.cache.lock().snapshot(), expected);
        restored
            .service
            .mux
            .write()
            .get_tab_mut(&tab_id)
            .unwrap()
            .session_binding_nonce = Some("foreign-nonce".into());
        assert!(super::super::tab_chat_snapshot(&restored.service, &tab_id, 0).is_err());
        drop(runtime);
        std::fs::remove_file(path).unwrap();
    }
}

#[test]
fn refused_unbound_start_has_one_typed_snapshot_state_before_and_after_restore() {
    use crate::service::tab_chat::runtime_stored::StoredHistoryFailure;
    let (fixture, _cwd, _provider, tab_id) = fixture(false);
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.agent_runtime_state = Some(serde_json::json!({
            "state":"degraded","failureCode":"provider_refused","reason":"opaque option refusal"
        }));
        if let TabBacking::ChatBackend { session_id, .. } = &mut tab.backing {
            session_id.clear();
        }
    }
    let error = for_tab(&fixture.service, &tab_id).unwrap_err();
    assert_eq!(
        StoredHistoryFailure::from_error(&error),
        Some(StoredHistoryFailure::Unavailable)
    );
    {
        let mut mux = fixture.service.mux.write();
        *mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
    }
    let restored = for_tab(&fixture.service, &tab_id).unwrap_err();
    assert_eq!(
        StoredHistoryFailure::from_error(&restored),
        Some(StoredHistoryFailure::Unavailable)
    );
    assert!(engine::for_tab(&tab_id).is_none());
}

fn kept_row(id: &str) -> ChatMsg {
    ChatMsg {
        id: id.into(),
        kind: "user".into(),
        content: format!("opaque {id}"),
        ..Default::default()
    }
}

#[test]
fn attach_serves_the_kept_transcript_while_the_runtime_refuses_to_load() {
    use crate::service::agent_runtime::transport;
    let (fixture, _cwd, provider, tab_id) = fixture(false);
    super::super::chatbackend_history::ChatBackendHistory::runtime_shared(&provider, "own-thread")
        .lock()
        .snapshot(vec![kept_row("kept")])
        .unwrap();
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .agent_runtime_state = Some(serde_json::json!({"state":"degraded",
        "failureCode":"provider_refused","reason":"Provider refused: opaque; recovery required"}));
    assert!(transport::snapshot(&fixture.service, &tab_id).is_err());
    let attached = transport::attach_transcript(&fixture.service, &tab_id).unwrap();
    assert_eq!(attached.messages.len(), 1);
    assert_eq!(attached.messages[0].content, "opaque kept");
    assert_eq!(attached.thread_id.as_deref(), Some("own-thread"));
    assert_eq!(attached.binding_nonce.as_deref(), Some("own-nonce"));
}

#[test]
fn attach_prefers_the_degraded_sessions_own_rows() {
    use crate::service::agent_runtime::{model::RuntimeFailure, transport};
    let (fixture, _cwd, _provider, tab_id) = fixture(false);
    let _runtime = attach(&fixture, &tab_id, "opaque own reply");
    let session = engine::for_tab(&tab_id).unwrap();
    session.thread_loaded.store(false, Ordering::Release);
    let epoch = session.state.lock().epoch;
    session
        .state
        .lock()
        .degrade(epoch, &RuntimeFailure::Refused("opaque refusal".into()));
    engine::publish(&fixture.service, &tab_id, &session);
    assert!(transport::snapshot(&fixture.service, &tab_id).is_err());
    let attached = transport::attach_transcript(&fixture.service, &tab_id).unwrap();
    assert_eq!(attached.messages.len(), 1);
    assert_eq!(attached.messages[0].content, "opaque own reply");
}

#[test]
fn attach_with_no_kept_rows_is_empty_but_provider_held_history_still_refuses() {
    use crate::service::agent_runtime::transport;
    let (host_kept, _cwd, _provider, tab_id) = fixture(false);
    let attached = transport::attach_transcript(&host_kept.service, &tab_id).unwrap();
    assert!(attached.messages.is_empty());
    assert_eq!(attached.thread_id.as_deref(), Some("own-thread"));
    let (provider_held, _cwd, _provider, tab_id) = fixture(true);
    assert!(transport::attach_transcript(&provider_held.service, &tab_id).is_err());
}

#[test]
fn archived_session_that_never_settled_a_turn_reads_from_its_session_log() {
    use super::super::chatbackend_history::{archive_path, ChatBackendHistory};
    use crate::multiplexer::types::{
        ContextEpochArchive, ContextEpochBoundary, ContextEpochTransport,
    };
    let (fixture, _cwd, provider, _tab_id) = fixture(false);
    let archive = ContextEpochArchive {
        provider: provider.clone(),
        cwd: String::new(),
        session_id: "broken-session".into(),
        transport: ContextEpochTransport::ChatBackend,
        boundary: ContextEpochBoundary::ContextReset,
    };
    ChatBackendHistory::open_at(archive_path(&provider, "broken-session"))
        .try_upsert(&[kept_row("logged")])
        .unwrap();
    let rows = super::super::runtime_stored::archived_messages(&fixture.service, &archive)
        .unwrap()
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].content, "opaque logged");
}
