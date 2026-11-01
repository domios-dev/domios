use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::agent::registry::AgentRegistry;

struct StoredSessionProvider {
    path: PathBuf,
}

#[test]
fn durable_general_record_restores_shared_database_without_a_mux_binding() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("shared.db");
    std::fs::write(&db, b"SQLite format 3\0").unwrap();
    let record = dir.path().join("general-session.json");
    std::fs::write(
        &record,
        r#"{"provider":"stored-fixture","session_id":"owned-session"}"#,
    )
    .unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(StoredSessionProvider { path: db }));
    let (mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab(&id).unwrap();
    assert!(tab.agent_type.is_none());
    assert!(tab.agent_session_id.is_none());
    let saved =
        load_session_record_from(&registry, "stored-fixture", "fixture-cwd", &[], &record).unwrap();
    assert!(session_id_is_owned(
        &registry,
        "stored-fixture",
        "fixture-cwd",
        &saved.session_id,
        (tab.agent_type.as_deref(), tab.agent_session_id.as_deref()),
        Some(&saved)
    ));
    assert_eq!(saved.session_id, "owned-session");
    assert!(
        load_session_record_from(&registry, "foreign-provider", "fixture-cwd", &[], &record)
            .is_none()
    );
    assert!(load_session_record_from(
        &registry,
        "stored-fixture",
        "fixture-cwd",
        &["owned-session".into()],
        &record
    )
    .is_none());
    std::fs::write(
        &record,
        r#"{"provider":"stored-fixture","session_id":"missing-session"}"#,
    )
    .unwrap();
    assert!(
        load_session_record_from(&registry, "stored-fixture", "fixture-cwd", &[], &record)
            .is_none()
    );
}

impl AgentProvider for StoredSessionProvider {
    fn id(&self) -> &str {
        "stored-fixture"
    }
    fn display_name(&self) -> &str {
        self.id()
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::PollShellPrompt {
            timeout: Duration::from_secs(1),
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        Some(self.path.clone())
    }
    fn session_exists(&self, _: &str, session: &str) -> Option<bool> {
        Some(session == "owned-session" && self.path.is_file())
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        panic!("ownership cannot discover a replacement session")
    }
    fn build_resume_command(&self, session: &str, _: bool) -> Option<String> {
        Some(format!("fixture --session {session}"))
    }
}

#[test]
fn main_owned_binding_restores_file_and_shared_database_sessions() {
    let dir = tempfile::tempdir().unwrap();
    for (name, bytes) in [
        ("session.jsonl", b"{\"role\":\"user\"}\n".as_slice()),
        ("shared.db", b"SQLite format 3\0".as_slice()),
    ] {
        let path = dir.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(StoredSessionProvider { path }));
        let owned = |provider, session, binding| {
            session_id_is_owned(&registry, provider, "fixture-cwd", session, binding, None)
        };
        assert!(
            owned(
                "stored-fixture",
                "owned-session",
                (Some("stored-fixture"), Some("owned-session"))
            ),
            "{name}"
        );
        assert!(!owned(
            "stored-fixture",
            "foreign-session",
            (Some("stored-fixture"), Some("owned-session"))
        ));
        assert!(!owned(
            "stored-fixture",
            "owned-session",
            (Some("other-provider"), Some("owned-session"))
        ));
        assert!(!owned(
            "stored-fixture",
            "owned-session",
            (Some("stored-fixture"), None)
        ));
        assert!(!owned(
            "stored-fixture",
            "missing-session",
            (Some("stored-fixture"), Some("missing-session"))
        ));
    }
}
