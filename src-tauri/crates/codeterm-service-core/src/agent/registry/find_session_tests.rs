use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use super::*;

const OWNED_ID: &str = "aaaa1111-owned-4aaa-8aaa-aaaaaaaaaaaa";

#[derive(Default)]
struct Calls {
    enumerate: AtomicUsize,
    find: parking_lot::Mutex<Vec<String>>,
}

struct CountingProvider {
    id: &'static str,
    owns: Option<SessionMetadata>,
    listed: Vec<SessionMetadata>,
    calls: Arc<Calls>,
}

impl AgentProvider for CountingProvider {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.id
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::from_secs(0),
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
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
        self.calls.enumerate.fetch_add(1, Ordering::SeqCst);
        self.listed.clone()
    }
    fn find_session(&self, session_id: &str) -> Option<SessionMetadata> {
        self.calls.find.lock().push(session_id.to_string());
        self.owns.clone()
    }
}

fn record(provider: &str, session_id: &str) -> SessionMetadata {
    SessionMetadata {
        session_id: session_id.into(),
        provider: provider.into(),
        project_path: Some(format!("/work/{provider}")),
        title: None,
        summary: None,
        file_path: format!("/store/{provider}/{session_id}"),
        transcript_path: None,
        created_ts: None,
        modified_ts: Some(1),
    }
}

fn provider(
    id: &'static str,
    owns: Option<SessionMetadata>,
    listed: Vec<SessionMetadata>,
    calls: &Arc<Calls>,
) -> Box<dyn AgentProvider> {
    Box::new(CountingProvider {
        id,
        owns,
        listed,
        calls: calls.clone(),
    })
}

#[test]
fn find_session_resolves_the_owner_without_enumerating_any_provider() {
    let owner_calls = Arc::new(Calls::default());
    let other_calls = Arc::new(Calls::default());
    let registry = AgentRegistry::new();
    registry.register_provider(provider(
        "owner",
        Some(record("owner", OWNED_ID)),
        vec![record("owner", OWNED_ID)],
        &owner_calls,
    ));
    registry.register_provider(provider(
        "other",
        None,
        vec![record("other", "bbbb2222-other-4bbb-8bbb-bbbbbbbbbbbb")],
        &other_calls,
    ));

    let found = registry
        .find_session(OWNED_ID)
        .expect("owner claims its id");

    assert_eq!(found.provider, "owner");
    assert_eq!(found.project_path.as_deref(), Some("/work/owner"));
    assert_eq!(owner_calls.enumerate.load(Ordering::SeqCst), 0);
    assert_eq!(other_calls.enumerate.load(Ordering::SeqCst), 0);
    assert_eq!(owner_calls.find.lock().as_slice(), [OWNED_ID.to_string()]);
    assert!(other_calls
        .find
        .lock()
        .iter()
        .all(|asked| asked == OWNED_ID));
}

#[test]
fn find_session_never_falls_back_to_enumerated_history() {
    let calls = Arc::new(Calls::default());
    let registry = AgentRegistry::new();
    registry.register_provider(provider(
        "lister",
        None,
        vec![record("lister", OWNED_ID)],
        &calls,
    ));

    assert!(registry.find_session(OWNED_ID).is_none());
    assert_eq!(calls.enumerate.load(Ordering::SeqCst), 0);
    assert_eq!(calls.find.lock().len(), 1);
}

#[test]
fn find_session_rejects_a_record_for_a_different_id() {
    let calls = Arc::new(Calls::default());
    let registry = AgentRegistry::new();
    registry.register_provider(provider(
        "confused",
        Some(record("confused", "cccc3333-other-4ccc-8ccc-cccccccccccc")),
        Vec::new(),
        &calls,
    ));

    assert!(registry.find_session(OWNED_ID).is_none());
    assert_eq!(calls.enumerate.load(Ordering::SeqCst), 0);
}
