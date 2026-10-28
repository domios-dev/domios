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
}

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
    assert_eq!(StoredHistoryFailure::from_error(&missing), Some(StoredHistoryFailure::Missing));
    std::fs::write(cwd.path().join("own-thread.runtime"), "not-json\n").unwrap();
    let corrupt = super::super::tab_chat_snapshot(&fixture.service, &tab_id, 0).unwrap_err();
    assert_eq!(StoredHistoryFailure::from_error(&corrupt), Some(StoredHistoryFailure::Corrupt));
}
