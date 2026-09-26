use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use super::*;
use crate::agent::provider::{
    AgentLaunchParams, AgentProvider, HookEventBinding, HookIngestionRoute, ProviderStateVerdict,
    ReadinessStrategy, TaskDelivery,
};
use crate::agent::registry::AgentRegistry;
use crate::multiplexer::commands::MuxState;
use crate::multiplexer::state::MultiplexerState;
use codeterm_util::lock_hold::TimedRwLock;

fn shared_mux() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    (Arc::new(TimedRwLock::new("mux-test", state)), tab_id)
}

#[test]
fn hook_refusal_tab_log_value_escapes_control_characters() {
    let value = escaped_hook_log_field("tab\r\nforged");

    assert_eq!(value, "\"tab\\r\\nforged\"");
    assert!(!value.contains('\r'));
    assert!(!value.contains('\n'));
}

fn unverified_evidence(
    session_id: &str,
) -> crate::agent::provider_identity::ProviderIdentityEvidence {
    crate::agent::provider_identity::ProviderIdentityEvidence {
        cwd: Some("/tmp/project".into()),
        session_id: Some(session_id.into()),
        configured_nonce: None,
        reported_nonce: None,
    }
}

fn current_identity_evidence(
    mux: &MuxState,
    tab_id: &str,
) -> crate::agent::provider_identity::ProviderIdentityEvidence {
    crate::agent::provider_identity::ProviderIdentityEvidence::capture(
        mux.read().get_tab(tab_id).unwrap(),
    )
}

struct HookProvider {
    id: &'static str,
    sessions: HashSet<&'static str>,
    materialization_root: Option<PathBuf>,
}

impl AgentProvider for HookProvider {
    fn id(&self) -> &str {
        self.id
    }

    fn display_name(&self) -> &str {
        &self.id
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

    fn session_file_path(&self, _: &str, session_id: &str) -> Option<PathBuf> {
        self.materialization_root
            .as_ref()
            .map(|root| root.join(format!("{session_id}.jsonl")))
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn session_exists(&self, _: &str, session_id: &str) -> bool {
        self.session_file_path("", session_id)
            .is_some_and(|path| path.is_file())
            || self.sessions.contains(session_id)
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn has_hooks(&self) -> bool {
        true
    }
}

mod launch_detection_tests;
mod permission_request_tests;
mod rebind_provider_tests;
mod roster_tests;
mod session_identity_tests;

fn registry(ids: &[&'static str]) -> AgentRegistry {
    registry_with_sessions(
        ids.iter()
            .map(|id| (*id, vec!["sid-a", "sid-b", "ses_shared"]))
            .collect(),
    )
}

fn registry_with_sessions(items: Vec<(&'static str, Vec<&'static str>)>) -> AgentRegistry {
    let registry = AgentRegistry::new();
    register_hook_providers(&registry, items);
    registry
}

fn register_hook_providers(
    registry: &AgentRegistry,
    items: Vec<(&'static str, Vec<&'static str>)>,
) {
    for (id, sessions) in items {
        registry.register_provider(Box::new(HookProvider {
            id,
            sessions: sessions.into_iter().collect(),
            materialization_root: None,
        }));
    }
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
            nonce_already_acknowledged: false,
            identity_evidence: unverified_evidence("sid-a"),
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
            nonce_already_acknowledged: false,
            identity_evidence: unverified_evidence("sid-b"),
        }
    );
    {
        let g = mux.read();
        let tab = g.get_tab(&tab_id).unwrap();
        assert_eq!(tab.agent_type, None);
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
        assert!(!tab.agent_type_locked);
    }

    let unchanged =
        bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-b", None).unwrap();
    assert_eq!(
        unchanged,
        SessionStartClaim {
            prev: Some("sid-b".into()),
            changed: false,
            nonce_verified: false,
            nonce_already_acknowledged: false,
            identity_evidence: unverified_evidence("sid-b"),
        }
    );
}

#[test]
fn repeated_verified_hook_reports_skip_side_effects_until_the_session_changes() {
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_session_id = Some("sid-a".into());
        tab.session_binding_nonce = Some("nonce-1".into());
    }
    let registry = registry(&["codex"]);
    let mut binding_side_effects = 0;

    let first = bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-a", Some("nonce-1"))
        .unwrap();
    assert!(!first.changed);
    assert!(first.nonce_verified);
    assert!(!first.nonce_already_acknowledged);
    assert!(!first.is_unchanged_acknowledged_report());
    binding_side_effects += 1;

    for _ in 0..8 {
        let repeated =
            bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-a", Some("nonce-1"))
                .unwrap();
        assert!(!repeated.changed);
        assert!(repeated.nonce_verified);
        assert!(repeated.nonce_already_acknowledged);
        assert!(can_short_circuit_session_start(&repeated, false, false));
        assert!(!can_short_circuit_session_start(&repeated, true, true));
        assert!(can_short_circuit_session_start(&repeated, true, false));
        let mut unverified_duplicate = repeated.clone();
        unverified_duplicate.nonce_verified = false;
        assert!(can_short_circuit_session_start(&unverified_duplicate, true, true));
        if !can_short_circuit_session_start(&repeated, false, false) {
            binding_side_effects += 1;
        }
    }
    assert_eq!(binding_side_effects, 1);

    let changed =
        bind_reported_session(&mux, &registry, &tab_id, "codex", "sid-b", Some("nonce-1"))
            .unwrap();
    assert!(changed.changed);
    assert!(changed.nonce_verified);
    assert!(changed.nonce_already_acknowledged);
    assert!(!changed.is_unchanged_acknowledged_report());
    binding_side_effects += 1;
    assert_eq!(binding_side_effects, 2);
}

#[test]
fn reported_session_rejects_cross_tab_duplicate_claim() {
    let (mux, tab_a) = shared_mux();
    mux.write().get_tab_mut(&tab_a).unwrap().cwd = Some("/tmp/project".into());
    let workspace_id = mux.read().find_workspace_for_tab(&tab_a).unwrap();
    let tab_b = mux.write().create_tab(&workspace_id).unwrap();
    mux.write().get_tab_mut(&tab_b).unwrap().cwd = Some("/tmp/project".into());
    let registry = registry(&["opencode"]);

    bind_reported_session(&mux, &registry, &tab_a, "opencode", "ses_shared", None).unwrap();
    let err =
        bind_reported_session(&mux, &registry, &tab_b, "opencode", "ses_shared", None).unwrap_err();
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
    let claim = bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-b", Some("nonce-1"))
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
fn reconnect_full_sequence_hook_accepted_with_chat_metadata() {
    use crate::service::boot_stage::BootStage;

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
        assert_eq!(
            mux.read().get_tab(&tab_id).unwrap().agent_type.as_deref(),
            Some("claude")
        );
    }

    mux.write().get_tab_mut(&tab_id).unwrap().boot_stage = Some(BootStage::AgentReady);

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
fn transcript_path_ingestion_round_trips_into_session_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    let supplied = tmp
        .path()
        .join("drifted-transcript-name.jsonl")
        .to_string_lossy()
        .to_string();
    let body = serde_json::json!({
        "session_id": "ingest-sid-1",
        "source": "startup",
        "transcript_path": supplied,
    });

    let reported = reported_transcript_path(&body).expect("field extracted from body");
    assert!(codeterm_plugin::session_evidence::record(
        "ingest-sid-1",
        &reported
    ));
    let stored = codeterm_plugin::session_evidence::stored_transcript_path("ingest-sid-1")
        .expect("evidence journaled for the session");
    assert_eq!(
        stored.to_string_lossy().replace('\\', "/"),
        supplied.replace('\\', "/"),
        "the supplied value lands in the journaled session metadata, separators normalized"
    );

    // Codex-shaped wrapped payload extracts through the same helper.
    let wrapped = serde_json::json!({
        "payload": { "session_id": "x", "transcript_path": "/tmp/wrapped.jsonl" }
    });
    assert_eq!(
        reported_transcript_path(&wrapped).as_deref(),
        Some("/tmp/wrapped.jsonl")
    );
}

#[test]
fn session_start_without_transcript_path_leaves_evidence_absent() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    let body = serde_json::json!({ "session_id": "plain-sid", "source": "startup" });
    assert_eq!(
        reported_transcript_path(&body),
        None,
        "payload without the field must not fabricate evidence"
    );
    assert_eq!(
        codeterm_plugin::session_evidence::stored_transcript_path("plain-sid"),
        None,
        "metadata unchanged — readers keep reconstruction, today's behavior"
    );
}

#[test]
fn mismatched_nonce_rejects_wholesale_before_transcript_path_persist() {
    let tmp = tempfile::tempdir().unwrap();
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    env.set(
        codeterm_plugin::session_evidence::EVIDENCE_DIR_ENV,
        tmp.path().join("store"),
    );

    let (mux, tab_id) = shared_mux();
    {
        let mut g = mux.write();
        let tab = g.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.session_binding_nonce = Some("nonce-1".into());
    }
    let registry = registry(&["claude"]);

    // The handler persists transcript evidence only after this claim succeeds;
    // a mismatched nonce rejects the report wholesale, new field included.
    let err = bind_reported_session(&mux, &registry, &tab_id, "claude", "sid-a", Some("wrong"))
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
    assert_eq!(
        codeterm_plugin::session_evidence::stored_transcript_path("sid-a"),
        None,
        "a rejected report must not leave transcript evidence behind"
    );
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
fn session_start_hook_binds_on_codex_0_157_1_payload() {
    let body = serde_json::json!({
        "type": "session_meta",
        "source": "startup",
        "payload": { "id": "sid-codex-01571" }
    });
    let sid = session_start_string(&body, &["payload", "id"]).unwrap();
    let provider_id = "codex-session-start-fixture";
    let registry = registry_with_sessions(vec![(provider_id, vec!["sid-codex-01571"])]);
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some(provider_id.into());
        tab.session_binding_nonce = Some("launch-nonce".into());
    }

    let claim = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        provider_id,
        &sid,
        Some("launch-nonce"),
    )
    .expect("Codex SessionStart payload binds to the launched tab");
    assert!(claim.changed);
    assert!(claim.nonce_verified);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some(sid.as_str())
    );

    crate::service::integration_silence::record_hook_binding_source(
        &registry,
        provider_id,
        &sid,
        crate::session::lifecycle::BindingSource::Hook,
        true,
    );
    let status = crate::service::integration_silence::integration_status_with_binding_fallback(
        serde_json::json!({ "session_start_hook": true }),
        provider_id,
        true,
        &[sid],
    );
    assert!(status.get("codeterm_binding_fallback_source").is_none());
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

    let err =
        bind_reported_session(&mux, &registry, &tab_id, "claude", "unknown-sid", None).unwrap_err();
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
    let snapshot = mux.read();
    let archive = snapshot
        .get_tab(&tab_id)
        .unwrap()
        .context_epoch_archives
        .first()
        .expect("prior provider context is archived");
    assert_eq!(archive.provider, "codex");
    assert_eq!(archive.cwd, "/tmp/project");
    assert_eq!(archive.session_id, "parent-rollout");
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
    let workspace_id = mux.read().find_workspace_for_tab(&tab_a).unwrap();
    let tab_b = mux.write().create_tab(&workspace_id).unwrap();
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
        &self.id
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

fn materializing_binding_fixture() -> (
    tempfile::TempDir,
    MuxState,
    String,
    Arc<AgentRegistry>,
    PathBuf,
) {
    let temp = tempfile::tempdir().unwrap();
    let (mux, tab_id) = shared_mux();
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(HookProvider {
        id: "codex",
        sessions: HashSet::new(),
        materialization_root: Some(temp.path().to_path_buf()),
    }));
    std::fs::write(temp.path().join("old.jsonl"), "{}").unwrap();
    mux.write().get_tab_mut(&tab_id).unwrap().pty_id = Some("surviving-pty".into());
    bind_reported_session(&mux, &registry, &tab_id, "codex", "old", None).unwrap();
    {
        let mut guard = mux.write();
        let tab = guard.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.reported_session_binding_nonce = Some("launch-nonce".into());
    }
    let new_path = temp.path().join("new.jsonl");
    (temp, mux, tab_id, registry, new_path)
}

#[test]
fn session_binding_deferral_refused_session_completes_on_materialization() {
    let (_temp, mux, tab_id, registry, new_path) = materializing_binding_fixture();
    let refusal = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "new",
        Some("launch-nonce"),
    )
    .unwrap_err();
    assert!(refusal.2.is_some());
    let pending =
        crate::service::session_binding_deferral::DeferredSessionBindingRegistry::default();
    let (tx, rx) = std::sync::mpsc::channel();
    let callback_mux = mux.clone();
    let callback_registry = registry.clone();
    pending
        .defer(
            &tab_id,
            "codex",
            "new",
            "old",
            new_path.clone(),
            Arc::new(move |tab, _| {
                let _ = tx.send(bind_reported_session(
                    &callback_mux,
                    &callback_registry,
                    &tab,
                    "codex",
                    "new",
                    Some("launch-nonce"),
                ));
            }),
        )
        .unwrap();
    std::fs::write(new_path, "{}").unwrap();
    rx.recv_timeout(Duration::from_secs(2))
        .expect("materialization event")
        .unwrap();
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("new")
    );
}

#[test]
fn session_binding_deferral_is_event_driven_and_newest_is_bounded() {
    let (temp, _mux, tab_id, _registry, new_path) = materializing_binding_fixture();
    let pending =
        crate::service::session_binding_deferral::DeferredSessionBindingRegistry::default();
    let (tx, rx) = std::sync::mpsc::channel();
    pending
        .defer(
            &tab_id,
            "codex",
            "older",
            "old",
            temp.path().join("older.jsonl"),
            Arc::new(|_, _| {}),
        )
        .unwrap();
    pending
        .defer(
            &tab_id,
            "codex",
            "new",
            "old",
            new_path.clone(),
            Arc::new(move |_, _| {
                let _ = tx.send(());
            }),
        )
        .unwrap();
    assert_eq!(pending.pending(&tab_id).unwrap().session_id, "new");
    std::fs::write(new_path, "{}").unwrap();
    rx.recv_timeout(Duration::from_secs(2))
        .expect("filesystem event drives completion");
}
