use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::*;
use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::agent::registry::AgentRegistry;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;

fn shared_mux() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    (Arc::new(TimedRwLock::new("mux-test", state)), tab_id)
}

struct HookProvider {
    id: &'static str,
    sessions: HashSet<&'static str>,
}

impl AgentProvider for HookProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        self.id
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        self.id.to_string()
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

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn session_exists(&self, _: &str, session_id: &str) -> bool {
        self.sessions.contains(session_id)
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn has_hooks(&self) -> bool {
        true
    }
}

fn registry(ids: &[&'static str]) -> AgentRegistry {
    registry_with_sessions(
        ids.iter()
            .map(|id| (*id, vec!["sid-a", "sid-b", "ses_shared"]))
            .collect(),
    )
}

fn registry_with_sessions(items: Vec<(&'static str, Vec<&'static str>)>) -> AgentRegistry {
    let registry = AgentRegistry::new();
    for (id, sessions) in items {
        registry.register_provider(Box::new(HookProvider {
            id,
            sessions: sessions.into_iter().collect(),
        }));
    }
    registry
}

#[test]
fn reported_session_rebinds_same_tab_for_new_conversation() {
    let (mux, tab_id) = shared_mux();
    mux.write().get_tab_mut(&tab_id).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry(&["codex"]);

    let first = bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-a", None).unwrap();
    assert_eq!(
        first,
        SessionStartClaim {
            prev: None,
            changed: true,
            nonce_verified: false,
        }
    );
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.agent_requested_model = Some("model-requested".into());
        tab.agent_provider_resolved_model = Some("model-resolved".into());
        tab.agent_session_model = Some("model-stale".into());
    }

    let second = bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-b", None).unwrap();
    assert_eq!(
        second,
        SessionStartClaim {
            prev: Some("sid-a".into()),
            changed: true,
            nonce_verified: false,
        }
    );
    {
        let g = mux.read();
        let tab = g.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_type.as_deref(), Some("codex"));
        assert_eq!(tab.agent_session_id.as_deref(), Some("sid-b"));
        assert_eq!(
            tab.agent_requested_model.as_deref(),
            Some("model-requested")
        );
        assert_eq!(
            tab.agent_provider_resolved_model.as_deref(),
            Some("model-resolved")
        );
        assert!(tab.agent_session_model.is_none());
        assert!(tab.agent_type_locked);
    }

    let unchanged =
        bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-b", None).unwrap();
    assert_eq!(
        unchanged,
        SessionStartClaim {
            prev: Some("sid-b".into()),
            changed: false,
            nonce_verified: false,
        }
    );
}

#[test]
fn reported_session_rejects_cross_tab_duplicate_claim() {
    let (mux, tab_a) = shared_mux();
    mux.write().get_tab_mut(&tab_a).unwrap().cwd = Some("/tmp/project".into());
    let tab_b = mux.write().create_tab();
    mux.write().get_tab_mut(&tab_b).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry(&["opencode"]);

    bind_reported_session(&mux, &registry, &tab_a, "opencode", "ses_shared", None).unwrap();
    let err = bind_reported_session(&mux, &registry, &tab_b, "opencode", "ses_shared", None)
        .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_b)
            .unwrap()
            .agent_session_id
            .as_deref(),
        None
    );
}

#[test]
fn reported_session_rejects_wrong_provider_for_locked_tab() {
    let (mux, tab_id) = shared_mux();
    mux.write().get_tab_mut(&tab_id).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry(&["claude", "codex"]);

    bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-a", None).unwrap();
    let err = bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-b", None).unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-a")
    );
}

#[test]
fn reported_session_enforces_binding_nonce() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("nonce-1".into());
    }
    let registry = registry(&["claude"]);

    // Wrong nonce → rejected, no binding.
    let err = bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-a", Some("nope"))
        .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        None
    );

    // Absent nonce → tolerated (un-upgraded hook).
    bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-a", None).unwrap();
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-a")
    );

    // Matching nonce → bound.
    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "claude",
        "sid-b",
        Some("nonce-1"),
    )
    .unwrap();
    assert!(claim.changed);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("sid-b")
    );
}

#[test]
fn reported_session_rejects_sid_not_owned_by_provider() {
    let (mux, tab_id) = shared_mux();
    mux.write().get_tab_mut(&tab_id).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry_with_sessions(vec![
        ("claude", vec!["claude-real"]),
        ("cursor-agent", vec!["cursor-foreign"]),
    ]);

    let err = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "claude",
        "cursor-foreign",
        Some("nonce-1"),
    )
    .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    let g = mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), None);
    assert_eq!(tab.agent_session_id.as_deref(), None);
}

#[test]
fn reported_session_binds_sid_owned_by_provider() {
    let (mux, tab_id) = shared_mux();
    mux.write().get_tab_mut(&tab_id).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry_with_sessions(vec![("claude", vec!["claude-real"])]);

    bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "claude",
        "claude-real",
        Some("nonce-1"),
    )
    .unwrap();

    let g = mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("claude-real"));
}

#[test]
fn fresh_launch_with_matching_nonce_accepts_unpersisted_session() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    // Provider knows of NO persisted session for this sid.
    let registry = registry_with_sessions(vec![("claude", vec![])]);

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "claude",
        "fresh-unpersisted-sid",
        Some("launch-nonce"),
    )
    .expect("fresh launch with matching nonce must be accepted");
    assert!(claim.changed);

    let g = mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(
        tab.agent_session_id.as_deref(),
        Some("fresh-unpersisted-sid")
    );
}

#[test]
fn reconnect_full_sequence_hook_accepted_with_chat_metadata() {
    use crate::service::boot_stage::{allows_shell_prompt_exit, BootStage};

    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_type_locked = false;
        tab.session_binding_nonce = Some("fresh-launch".into());
        tab.restored_from_snapshot = true;
    }

    for stage in [
        BootStage::PtyStarting,
        BootStage::PtySpawned,
        BootStage::ShellReady,
        BootStage::AgentLaunching,
    ] {
        mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(stage);
        assert!(!allows_shell_prompt_exit(Some(stage)));
        assert_eq!(
            mux.read().get_tab(&tab_id).unwrap().agent_type.as_deref(),
            Some("claude")
        );
    }

    let observed_stage = mux.read().get_tab(&tab_id).unwrap().boot_stage;
    mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::AgentReady);
    assert!(!allows_shell_prompt_exit(observed_stage));

    let registry = registry_with_sessions(vec![("claude", vec![])]);
    bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "claude",
        "fresh-session",
        Some("fresh-launch"),
    )
    .expect("fresh launch hook must retain and bind Tab identity");

    let snapshot = mux.read().snapshot();
    let tab = snapshot.tabs.get(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert_eq!(tab.agent_session_id.as_deref(), Some("fresh-session"));
}

#[test]
fn session_start_string_reads_codex_session_meta_payload() {
    let body = serde_json::json!({
        "type": "session_meta",
        "payload": {
            "session_id": "sid-from-payload",
            "id": "sid-from-id"
        }
    });

    assert_eq!(
        session_start_string(&body, &["payload", "session_id"]).as_deref(),
        Some("sid-from-payload")
    );

    let fallback = serde_json::json!({
        "type": "session_meta",
        "payload": { "id": "sid-from-id" }
    });
    assert_eq!(
        session_start_string(&fallback, &["payload", "id"]).as_deref(),
        Some("sid-from-id")
    );
}

#[test]
fn tab_with_nonce_but_nonceless_report_rejects_unpersisted_session() {
    // still gated by session_exists — a nonce-less report is not proof-of-ownership.
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let registry = registry_with_sessions(vec![("claude", vec![])]);

    let err = bind_reported_session(&mux, &registry, &tab_id, "claude", "unknown-sid", None)
        .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        None
    );
}

#[test]
fn matching_nonce_advances_lineage_over_materialized_parent() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_session_id = Some("parent-rollout".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let registry = registry_with_sessions(vec![("codex", vec!["parent-rollout"])]);

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "new-rollout",
        Some("launch-nonce"),
    )
    .expect("authenticated resume must advance lineage over a materialized parent");
    assert!(claim.changed);
    assert!(claim.nonce_verified);
    assert_eq!(claim.prev.as_deref(), Some("parent-rollout"));
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("new-rollout"),
    );
}

#[test]
fn nonceless_report_cannot_evict_materialized_parent() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_session_id = Some("parent-rollout".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let registry = registry_with_sessions(vec![("codex", vec!["parent-rollout"])]);

    let err =
        bind_reported_session(&mux, &registry, &tab_id, "codex", "new-rollout", None).unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("parent-rollout"),
    );
}

#[test]
fn rotated_launch_nonce_advances_lineage_even_after_prior_acknowledgement() {
    // A rotated launch nonce remains fresh even when the prior nonce was acknowledged.
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_session_id = Some("parent-rollout".into());
        tab.session_binding_nonce = Some("nonce-new".into());
        tab.reported_session_binding_nonce = Some("nonce-old".into());
    }
    let registry = registry_with_sessions(vec![("codex", vec!["parent-rollout"])]);

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "fresh-rollout",
        Some("nonce-new"),
    )
    .expect("a rotated, unacknowledged launch nonce advances lineage");
    assert!(claim.changed);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("fresh-rollout"),
    );
}

#[test]
fn fresh_launch_with_matching_nonce_still_rejects_already_claimed_sid() {
    // still fire — two tabs cannot own the same live session.
    let (mux, tab_a) = shared_mux();
    mux.write().get_tab_mut(&tab_a).unwrap().cwd = Some("/tmp/project".into());
    let tab_b = mux.write().create_tab();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_b).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let registry = registry_with_sessions(vec![("claude", vec!["ses_shared"])]);

    bind_reported_session(&mux, &registry, &tab_a, "claude", "ses_shared", None).unwrap();
    let err = bind_reported_session(
        &mux,
        &registry,
        &tab_b,
        "claude",
        "ses_shared",
        Some("launch-nonce"),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_b)
            .unwrap()
            .agent_session_id
            .as_deref(),
        None
    );
}

#[test]
fn reported_session_without_nonce_rejects_foreign_sid() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("nonce-1".into());
    }
    let registry = registry_with_sessions(vec![
        ("claude", vec!["claude-real"]),
        ("cursor-agent", vec!["cursor-foreign"]),
    ]);

    let err = bind_reported_session(&mux, &registry, &tab_id, "claude", "cursor-foreign", None)
        .unwrap_err();

    assert_eq!(err.0, StatusCode::CONFLICT);
    let g = mux.read();
    let tab = g.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_type.as_deref(), None);
    assert_eq!(tab.agent_session_id.as_deref(), None);
}

// T11 — ownership boundary: a generic source=startup report carrying a random
// UUID (no rollout store) must not overwrite the tab's materialized binding.
#[test]
fn provisional_claim_cannot_evict_materialized_binding() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    // Only "materialized-sid" has a backing store; the rotated provisional does not.
    let registry = registry_with_sessions(vec![("codex", vec!["materialized-sid"])]);

    bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "materialized-sid",
        Some("launch-nonce"),
    )
    .expect("materialized session binds");

    let err = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "provisional-fileless-sid",
        Some("launch-nonce"),
    )
    .unwrap_err();
    assert_eq!(err.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("materialized-sid"),
        "materialized binding must survive a provisional claim"
    );
}

// Reproduces prod tab e3fa0754: both sids are fileless (no rollout store), so the
// last provisional claim wins — nothing materialized exists to protect.
#[test]
fn last_provisional_claim_wins_when_none_materialized() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let registry = registry_with_sessions(vec![("codex", vec![])]);

    bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "provisional-a",
        Some("launch-nonce"),
    )
    .expect("first provisional binds");
    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "provisional-b",
        Some("launch-nonce"),
    )
    .expect("second provisional wins");
    assert!(claim.changed);
    assert_eq!(claim.prev.as_deref(), Some("provisional-a"));
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("provisional-b")
    );
}

/// Provider whose `session_exists` records whether the mux write lock is held
/// when it runs — proves the probe happens outside the lock (task_hsc3uw).
struct LockProbeProvider {
    id: &'static str,
    sessions: HashSet<&'static str>,
    mux: Weak<TimedRwLock<MultiplexerState>>,
    probe_calls: Arc<AtomicUsize>,
    lock_held_during_probe: Arc<AtomicBool>,
}

impl AgentProvider for LockProbeProvider {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.id
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        self.id.to_string()
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn session_exists(&self, _: &str, session_id: &str) -> bool {
        self.probe_calls.fetch_add(1, Ordering::SeqCst);
        // If the bind path still holds the mux lock while probing, no writer
        // can acquire it — record that as the violation this test guards.
        if let Some(mux) = self.mux.upgrade() {
            if mux.try_write().is_none() {
                self.lock_held_during_probe.store(true, Ordering::SeqCst);
            }
        }
        self.sessions.contains(session_id)
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn has_hooks(&self) -> bool {
        true
    }
}

#[test]
fn session_exists_probe_never_runs_under_mux_lock() {
    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let lock_held = Arc::new(AtomicBool::new(false));
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(LockProbeProvider {
        id: "codex",
        sessions: HashSet::new(),
        mux: Arc::downgrade(&mux),
        probe_calls: probe_calls.clone(),
        lock_held_during_probe: lock_held.clone(),
    }));

    bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "fresh-sid",
        Some("launch-nonce"),
    )
    .expect("nonce-verified fresh launch binds");

    assert!(
        probe_calls.load(Ordering::SeqCst) > 0,
        "regression must actually exercise the session_exists probe"
    );
    assert!(
        !lock_held.load(Ordering::SeqCst),
        "session_exists must never be invoked while the mux lock is held"
    );
}

fn arm_hook_launch(
    mux: &MuxState,
    tab_id: &str,
    pty_id: &str,
    nonce: &str,
) -> crate::service::LaunchGeneration {
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.pty_id = Some(pty_id.to_string());
        tab.session_binding_nonce = Some(nonce.to_string());
        tab.reported_session_binding_nonce = None;
    }
    crate::service::arm_provider_session_for_test(tab_id, pty_id, nonce)
}

fn authenticated_claim(
    mux: &MuxState,
    registry: &AgentRegistry,
    tab_id: &str,
    nonce: &str,
) -> SessionStartClaim {
    bind_reported_session(mux, registry, tab_id, "codex", "sid-a", Some(nonce))
        .expect("authenticated session hook must bind")
}

#[test]
fn authenticated_hook_acknowledges_the_matching_launch_binding() {
    let (mux, tab_id) = shared_mux();
    let launch = arm_hook_launch(&mux, &tab_id, "pty-a", "nonce-a");
    let claim = authenticated_claim(&mux, &registry(&["codex"]), &tab_id, "nonce-a");

    assert!(acknowledge_authenticated_launch(
        &mux,
        &tab_id,
        &claim,
        Some("nonce-a")
    ));
    assert!(launch.await_provider_ack(Duration::ZERO));
}

#[test]
fn authenticated_hook_from_an_old_generation_on_the_same_pty_is_ignored() {
    let (mux, tab_id) = shared_mux();
    let _old = arm_hook_launch(&mux, &tab_id, "pty-a", "nonce-old");
    let claim = authenticated_claim(&mux, &registry(&["codex"]), &tab_id, "nonce-old");
    let current = arm_hook_launch(&mux, &tab_id, "pty-a", "nonce-current");

    assert!(!acknowledge_authenticated_launch(
        &mux,
        &tab_id,
        &claim,
        Some("nonce-old")
    ));
    assert!(!current.await_provider_ack(Duration::ZERO));
}

#[test]
fn authenticated_hook_from_a_replaced_pty_is_ignored() {
    let (mux, tab_id) = shared_mux();
    let _old = arm_hook_launch(&mux, &tab_id, "pty-old", "nonce-old");
    let claim = authenticated_claim(&mux, &registry(&["codex"]), &tab_id, "nonce-old");
    let current = arm_hook_launch(&mux, &tab_id, "pty-new", "nonce-current");

    assert!(!acknowledge_authenticated_launch(
        &mux,
        &tab_id,
        &claim,
        Some("nonce-old")
    ));
    assert!(!current.await_provider_ack(Duration::ZERO));
}

#[test]
fn hook_with_the_wrong_binding_nonce_cannot_acknowledge_launch_readiness() {
    let (mux, tab_id) = shared_mux();
    let launch = arm_hook_launch(&mux, &tab_id, "pty-a", "nonce-current");
    let result = bind_reported_session(
        &mux,
        &registry(&["codex"]),
        &tab_id,
        "codex",
        "sid-a",
        Some("nonce-wrong"),
    );

    assert_eq!(result.unwrap_err().0, StatusCode::CONFLICT);
    assert!(!launch.await_provider_ack(Duration::ZERO));
}
