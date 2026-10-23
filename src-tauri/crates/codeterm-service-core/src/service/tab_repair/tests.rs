use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, TaskBinding};
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::NewTask;
use crate::session::lifecycle::{
    AgentTypeWriteAudit, CommittedAuthority, JournalRecord, LifecycleCheckpoint, LifecycleEpoch,
    LifecycleKey,
};

fn claimed_tab(provider: &str) -> Tab {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-live".into());
    tab.cwd = Some("D:/repo".into());
    tab.agent_type = Some(provider.into());
    tab.agent_type_locked = true;
    tab.agent_session_id = Some("session-live".into());
    tab.session_binding_nonce = Some("nonce-live".into());
    tab.reported_session_binding_nonce = Some("nonce-live".into());
    tab.clone()
}

fn repair_fixture(
    provider: &str,
) -> (
    crate::service::test_fixture::ServiceFixture,
    String,
    AuthenticatedPlanCaller,
) {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-live".into());
    tab.cwd = Some("D:/repo".into());
    tab.agent_type = Some(provider.into());
    tab.agent_type_locked = true;
    tab.agent_session_id = Some("session-live".into());
    tab.session_binding_nonce = Some("nonce-live".into());
    tab.reported_session_binding_nonce = Some("nonce-live".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("codex", "session-live"),
    ));
    let caller = AuthenticatedPlanCaller {
        tab_id: Some(tab_id.clone()),
        session_id: Some("session-live".into()),
        binding_nonce: Some("nonce-live".into()),
    };
    (fixture, tab_id, caller)
}

fn checkpoint(
    seq: u64,
    operation_id: &str,
    provider: &str,
    write: Option<AgentTypeWriteAudit>,
) -> JournalRecord {
    JournalRecord::Checkpoint {
        checkpoint: LifecycleCheckpoint {
            key: LifecycleKey::new("tab-live", LifecycleEpoch::first(), operation_id),
            committed_through: seq,
            authority: CommittedAuthority {
                provider: provider.into(),
                session_id: "session-live".into(),
                binding_nonce: format!("nonce-{seq}"),
                role: None,
                role_profile: None,
                team_id: None,
                parent_tab_id: None,
                can_spawn: false,
                depth: 0,
            },
            agent_type_write: write,
            prior_checkpoints: Vec::new(),
            sealed_at: format!("recorded-{seq}"),
        },
    }
}

#[test]
fn own_agent_authority_requires_its_live_session_and_nonce() {
    let tab = claimed_tab("claude");
    let own = AuthenticatedPlanCaller {
        tab_id: Some(tab.id.clone()),
        session_id: tab.agent_session_id.clone(),
        binding_nonce: tab.session_binding_nonce.clone(),
    };
    let foreign = AuthenticatedPlanCaller {
        tab_id: Some("foreign".into()),
        ..own.clone()
    };
    let nonce_only = AuthenticatedPlanCaller {
        session_id: None,
        ..own.clone()
    };

    assert!(caller_matches_tab(&tab, &own));
    assert!(caller_matches_tab(&tab, &nonce_only));
    assert!(!caller_matches_tab(&tab, &foreign));
}

#[test]
fn foreign_tab_without_plan_ownership_is_refused_by_constant() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.pty_id = Some("pty-live".into());
    tab.cwd = Some("D:/repo".into());
    tab.agent_type = Some("claude".into());
    tab.agent_session_id = Some("session-live".into());
    tab.session_binding_nonce = Some("nonce-live".into());
    tab.reported_session_binding_nonce = Some("nonce-live".into());
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    let foreign = AuthenticatedPlanCaller {
        tab_id: Some("foreign-tab".into()),
        session_id: Some("foreign-session".into()),
        binding_nonce: Some("foreign-nonce".into()),
    };

    let error = fixture
        .service
        .authorize_tab_repair(&tab_id, &foreign)
        .unwrap_err();

    assert_eq!(error.code(), TAB_REPAIR_NOT_AUTHORIZED);
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_type
            .as_deref(),
        Some("claude")
    );
}

#[tokio::test]
async fn live_plan_owner_may_repair_its_assigned_worker() {
    let (mut mux, owner_id) = MultiplexerState::new();
    mux.set_orchestrator_role(&owner_id).unwrap();
    let workspace_id = mux.list_workspaces()[0].id.clone();
    let worker_id = mux.create_tab(&workspace_id).unwrap();
    mux.set_tab_role(&worker_id, AgentRole::Worker, Some(&owner_id))
        .unwrap();
    for (tab_id, session_id, nonce) in [
        (&owner_id, "owner-session", "owner-nonce"),
        (&worker_id, "worker-session", "worker-nonce"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.pty_id = Some(format!("pty-{tab_id}"));
        tab.cwd = Some("D:/repo".into());
        tab.agent_type = Some(if tab_id == &worker_id {
            "claude".into()
        } else {
            "codex".into()
        });
        tab.agent_type_locked = tab_id == &worker_id;
        tab.agent_session_id = Some(session_id.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
    }
    let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session("codex", "worker-session"),
    ));
    let mut plan = Plan::new(
        "Repair authority".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&owner_id),
    );
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan.id,
            NewTask {
                name: "Worker repair".into(),
                kind: "implement".into(),
                body: "repair identity".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan.id, &task_id, &worker_id, None, 3)
        .unwrap();
    let task_binding = TaskBinding {
        plan_id: Some(plan.id),
        task_id: Some(task_id),
        attempt_id: None,
        spawn_handle: None,
        bound_at: 3,
    };
    fixture
        .service
        .mux
        .write()
        .get_tab_mut(&worker_id)
        .unwrap()
        .task_binding = Some(task_binding);
    let caller = AuthenticatedPlanCaller {
        tab_id: Some(owner_id),
        session_id: Some("owner-session".into()),
        binding_nonce: Some("owner-nonce".into()),
    };

    let authorized = fixture
        .service
        .authorize_tab_repair(&worker_id, &caller)
        .unwrap();
    let response = fixture
        .service
        .repair_tab_from_live_evidence(authorized, &caller, None)
        .await
        .unwrap();

    assert_eq!(response.provider.current.as_deref(), Some("codex"));
    let mux = fixture.service.mux.read();
    let worker = mux.get_tab(&worker_id).unwrap();
    assert_eq!(worker.agent_type.as_deref(), Some("codex"));
    assert_eq!(worker.agent_session_id.as_deref(), Some("worker-session"));
    assert!(!worker.agent_type_locked);
}

#[tokio::test]
async fn repair_from_live_evidence_corrects_locked_label_without_restart() {
    let (fixture, tab_id, caller) = repair_fixture("claude");
    let target = fixture
        .service
        .authorize_tab_repair(&tab_id, &caller)
        .unwrap();

    let response = fixture
        .service
        .repair_tab_from_live_evidence(target, &caller, None)
        .await
        .unwrap();

    assert_eq!(response.provider.status, RepairFieldStatus::Corrected);
    assert_eq!(response.parser_lock.current, Some(false));
    let mux = fixture.service.mux.read();
    let repaired = mux.get_tab(&tab_id).unwrap();
    assert_eq!(repaired.pty_id.as_deref(), Some("pty-live"));
    assert_eq!(repaired.agent_type.as_deref(), Some("codex"));
    assert_eq!(repaired.agent_session_id.as_deref(), Some("session-live"));
    assert!(!repaired.agent_type_locked);
}

#[tokio::test]
async fn asserted_provider_mismatch_is_refused_by_the_stable_constant() {
    let (fixture, tab_id, caller) = repair_fixture("claude");
    let target = fixture
        .service
        .authorize_tab_repair(&tab_id, &caller)
        .unwrap();

    let error = fixture
        .service
        .repair_tab_from_live_evidence(target, &caller, Some("claude"))
        .await
        .unwrap_err();

    assert_eq!(error.code(), TAB_REPAIR_VERIFIED_SESSION_REQUIRED);
    let tab = fixture.service.mux.read();
    let unchanged = tab.get_tab(&tab_id).unwrap();
    assert_eq!(unchanged.agent_type.as_deref(), Some("claude"));
    assert!(unchanged.agent_type_locked);
}

#[test]
fn repair_result_reports_locked_label_correction_without_a_restart() {
    let previous = TabAgentTypeState {
        provider: Some("claude".into()),
        locked: true,
    };
    let current = TabAgentTypeState {
        provider: Some("codex".into()),
        locked: false,
    };
    let response = TabRepairResponse::from_identity_write(
        "tab-live".into(),
        TabRepairSource::LiveEvidence,
        Some("session-live".into()),
        "session-live".into(),
        "codex".into(),
        true,
        AgentTypeWriteOutcome::Applied { previous, current },
        None,
    )
    .unwrap();

    assert_eq!(response.provider.status, RepairFieldStatus::Corrected);
    assert_eq!(response.provider.current.as_deref(), Some("codex"));
    assert_eq!(response.session.status, RepairFieldStatus::Kept);
    assert_eq!(response.parser_lock.current, Some(false));
}

#[test]
fn changed_evidence_is_a_typed_refusal() {
    let current = TabAgentTypeState {
        provider: Some("claude".into()),
        locked: true,
    };
    let evidence = ProviderIdentityEvidence::capture(&claimed_tab("claude"));
    let error = TabRepairResponse::from_identity_write(
        "tab-live".into(),
        TabRepairSource::LiveEvidence,
        Some("session-live".into()),
        "session-live".into(),
        "codex".into(),
        true,
        AgentTypeWriteOutcome::EvidenceChanged {
            expected: evidence.clone(),
            current_evidence: evidence,
            current,
        },
        None,
    )
    .unwrap_err();

    assert_eq!(error.code(), TAB_REPAIR_EVIDENCE_CHANGED);
}

#[test]
fn unverified_session_is_refused_by_the_stable_constant() {
    let tab = claimed_tab("claude");
    let mut evidence = ProviderIdentityEvidence::capture(&tab);
    evidence.reported_nonce = Some("foreign-nonce".into());

    let error = verified_session_query(&evidence).unwrap_err();

    assert_eq!(error.code(), TAB_REPAIR_VERIFIED_SESSION_REQUIRED);
}

#[test]
fn journal_restore_skips_newer_wrong_provider_and_names_the_source() {
    let load = JournalLoad {
        records: vec![
            checkpoint(10, "good-operation", "codex", None),
            checkpoint(20, "corrupt-operation", "claude", None),
        ],
        tail: TailStatus::Intact,
        duplicate_seqs: 0,
    };

    let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

    assert_eq!(plan.provider, "codex");
    assert_eq!(plan.report.restored_from.seq, 10);
    assert_eq!(plan.report.restored_from.operation_id, "good-operation");
    assert_eq!(plan.report.skipped.len(), 1);
    assert_eq!(plan.report.skipped[0].seq, 20);
    assert!(plan.report.skipped[0].reason.contains("does not own"));
}

#[test]
fn parser_origin_checkpoint_is_never_a_restore_source() {
    let parser_write = AgentTypeWriteAudit {
        origin: AgentTypeWriteOrigin::Parser,
        previous_provider: Some("codex".into()),
        new_provider: Some("codex".into()),
        attempted_provider: Some("claude".into()),
        caller_tab_id: None,
        session_id: Some("session-live".into()),
        nonce_verified: true,
        applied: false,
        recorded_at: "parser-write".into(),
    };
    let load = JournalLoad {
        records: vec![
            checkpoint(10, "good-operation", "codex", None),
            checkpoint(20, "parser-operation", "codex", Some(parser_write)),
        ],
        tail: TailStatus::Intact,
        duplicate_seqs: 0,
    };

    let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

    assert_eq!(plan.report.restored_from.seq, 10);
    assert_eq!(plan.report.skipped[0].seq, 20);
    assert!(plan.report.skipped[0].reason.contains("parser-origin"));
}

#[test]
fn nonce_verified_repair_checkpoint_is_a_restore_source() {
    let repair_write = AgentTypeWriteAudit {
        origin: AgentTypeWriteOrigin::Repair,
        previous_provider: Some("claude".into()),
        new_provider: Some("codex".into()),
        attempted_provider: Some("codex".into()),
        caller_tab_id: Some("tab-live".into()),
        session_id: Some("session-live".into()),
        nonce_verified: true,
        applied: true,
        recorded_at: "repair-write".into(),
    };
    let load = JournalLoad {
        records: vec![checkpoint(
            20,
            "repair-operation",
            "codex",
            Some(repair_write),
        )],
        tail: TailStatus::Intact,
        duplicate_seqs: 0,
    };

    let plan = select_journal_restore(&load, "tab-live", "session-live", "codex").unwrap();

    assert_eq!(plan.report.restored_from.seq, 20);
    assert_eq!(
        plan.report.restored_from.recorded_at.as_deref(),
        Some("repair-write")
    );
    assert!(plan.report.skipped.is_empty());
}
