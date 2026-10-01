use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, PlanMode, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;
use codeterm_types::pty::BootStage;

/// The closed loop: with no session ever registered there is no lineage to
/// prove, so the repair could never run. Presenting the tab's own launch
/// nonce establishes the binding (#316).
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rebind_establishes_a_first_binding_from_the_tabs_own_nonce() {
    let (fixture, _plan_id, owner, _planner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    let caller = AuthenticatedPlanCaller {
        session_id: None,
        ..owner.clone()
    };
    let outcome = fixture
        .service
        .rebind_stale_planning_session(&caller)
        .expect("a tab presenting its own nonce can establish the first binding");
    assert!(
        outcome.resynced,
        "the binding was established, not already current"
    );
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(
        tab.reported_session_binding_nonce, tab.session_binding_nonce,
        "the tab attested its own launch nonce"
    );
    assert!(
        tab.agent_session_id.is_none(),
        "no provider session may be invented — only the attestation is recorded"
    );
}

/// The bootstrap must never mask a live provider session.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rebind_does_not_bootstrap_over_a_registered_session() {
    let (fixture, _plan_id, owner, _planner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = Some("live-session".into());
        tab.reported_session_binding_nonce = None;
    }
    let caller = AuthenticatedPlanCaller {
        session_id: None,
        ..owner.clone()
    };
    assert!(
        fixture
            .service
            .rebind_stale_planning_session(&caller)
            .is_err(),
        "a tab with a registered session must go through the lineage proof"
    );
}

/// A caller without this tab's launch nonce is still refused.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn rebind_refuses_a_bootstrap_from_a_foreign_nonce() {
    let (fixture, _plan_id, owner, _planner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    let caller = AuthenticatedPlanCaller {
        session_id: None,
        binding_nonce: Some("not-this-tabs-nonce".into()),
        ..owner.clone()
    };
    assert!(
        fixture
            .service
            .rebind_stale_planning_session(&caller)
            .is_err(),
        "the launch nonce is the whole proof — without it there is none"
    );
}

/// After the bootstrap the write gate accepts the tab, which is the point.
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn a_tab_attested_binding_authorizes_plan_writes() {
    let (fixture, plan_id, owner, _planner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
    }
    let caller = AuthenticatedPlanCaller {
        session_id: None,
        ..owner.clone()
    };
    assert!(
        fixture
            .service
            .authorize_draft_structure_mutation(&plan_id, &caller)
            .is_err(),
        "before the bootstrap the write is refused"
    );
    fixture
        .service
        .rebind_stale_planning_session(&caller)
        .expect("bootstrap");
    fixture
        .service
        .authorize_draft_structure_mutation(&plan_id, &caller)
        .expect("after the bootstrap the same caller may write");
}

/// A tab whose provider never registered a session is a dead end today: the
/// gate refuses it exactly like a foreign caller, and `plan rebind` — the
/// documented repair — needs the very binding that is missing. The refusal
/// stays; the diagnosis has to say which of the two it is (#316).
#[test]
fn an_unregistered_session_is_named_apart_from_a_foreign_caller() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.pty_id = Some("pty-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    // The hook never reported one, so there is nothing to authorize against.
    tab.agent_session_id = None;
    tab.reported_session_binding_nonce = None;
    let tab = &*tab;

    assert_eq!(
        binding_failure_class(tab, None, Some("nonce-1")),
        PlanAuthorityError::SessionUnregistered,
        "a tab presenting its own launch nonce is not a foreign caller"
    );
}

#[test]
fn plan_write_before_binding_reports_retryable_binding_pending() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider-neutral".into());
    tab.pty_id = Some("pty-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.boot_stage = Some(BootStage::ProviderReady);
    tab.agent_set_at = Some(std::time::Instant::now());
    let tab = &*tab;

    let error = binding_failure_class(tab, None, Some("nonce-1"));
    assert_eq!(error, PlanAuthorityError::SessionBindingPending);
    assert_eq!(error.diagnostic_type(), "session_binding_pending");
    assert!(error.is_retryable());
}

#[test]
fn binding_pending_covers_agent_ready_but_not_a_settled_or_expired_launch() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("provider-neutral".into());
    tab.pty_id = Some("pty-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.agent_set_at = Some(std::time::Instant::now());
    tab.boot_stage = Some(BootStage::AgentReady);
    assert!(launch_binding_is_pending(tab));

    tab.boot_stage = Some(BootStage::LaunchDeadlineElapsed);
    tab.agent_set_at = Some(std::time::Instant::now());
    assert!(!launch_binding_is_pending(tab));
    assert_eq!(
        binding_failure_class(tab, None, Some("nonce-1")),
        PlanAuthorityError::SessionUnregistered
    );

    tab.boot_stage = Some(BootStage::LaunchBindingUnconfirmed);
    assert!(!launch_binding_is_pending(tab));
    assert_eq!(
        binding_failure_class(tab, None, Some("nonce-1")),
        PlanAuthorityError::SessionUnregistered
    );
    tab.boot_stage = Some(BootStage::AgentReady);
    tab.agent_set_at =
        Some(std::time::Instant::now() - crate::service::worker_ack::STARTUP_BINDING_DEADLINE);
    assert!(!launch_binding_is_pending(tab));
    assert_eq!(
        binding_failure_class(tab, None, Some("nonce-1")),
        PlanAuthorityError::SessionUnregistered
    );
}

/// The arm is reached only by proof of running inside the tab, so a caller
/// with someone else's nonce learns nothing new.
#[test]
fn a_foreign_nonce_still_reads_as_foreign() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_type = Some("claude".into());
    tab.pty_id = Some("pty-1".into());
    tab.session_binding_nonce = Some("nonce-1".into());
    tab.agent_session_id = None;
    let tab = &*tab;

    assert_eq!(
        binding_failure_class(tab, None, Some("someone-elses-nonce")),
        PlanAuthorityError::BindingForeign,
    );
    assert_eq!(
        binding_failure_class(tab, None, None),
        PlanAuthorityError::BindingForeign
    );
}

/// The remedy has to name the integration, because rebinding cannot fix it.
#[test]
fn the_unregistered_diagnosis_points_away_from_rebind() {
    let error = PlanAuthorityError::SessionUnregistered;
    assert_eq!(error.diagnostic_type(), "plan_binding_session_unregistered");
    assert!(!error.is_retryable());
}

fn fixture() -> (
    ServiceFixture,
    String,
    AuthenticatedPlanCaller,
    AuthenticatedPlanCaller,
) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let planner = mux.create_tab(&workspace).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    for (tab_id, session, nonce) in [
        (&owner, "owner-session", "owner-nonce"),
        (&planner, "planner-session", "planner-nonce"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        // Make the tab pass `tab_has_live_agent` so the stale-after-relaunch
        // proof (which requires a genuinely live tab) is exercised.
        tab.agent_type = Some("provider-neutral".into());
        tab.chat_view = true;
        tab.pty_id = Some(format!("pty-{tab_id}"));
    }
    {
        let tab = mux.get_tab_mut(&planner).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("planner".into());
        tab.parent_tab_id = Some(owner.clone());
        tab.team_id = Some(team_id.clone());
    }
    let fixture = ServiceFixture::new(mux);
    let mut plan = Plan::new(
        "Draft graph".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    let caller = |tab_id: String, session: &str, nonce: &str| AuthenticatedPlanCaller {
        tab_id: Some(tab_id),
        session_id: Some(session.into()),
        binding_nonce: Some(nonce.into()),
    };
    (
        fixture,
        plan.id,
        caller(planner, "planner-session", "planner-nonce"),
        caller(owner, "owner-session", "owner-nonce"),
    )
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[tokio::test(flavor = "current_thread")]
async fn http_plan_authority_wait_does_not_block_another_request() {
    let (fixture, plan_id, _, owner) = fixture();
    let owner_tab_id = owner.tab_id.clone().unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&owner_tab_id).unwrap();
        tab.agent_session_id = None;
        tab.reported_session_binding_nonce = None;
        tab.boot_stage = Some(BootStage::ProviderReady);
        tab.agent_set_at = Some(
            std::time::Instant::now() - crate::service::worker_ack::STARTUP_BINDING_DEADLINE
                + std::time::Duration::from_secs(3),
        );
    }
    let pending_service = std::sync::Arc::new(fixture.service.clone());
    let pending_plan = plan_id.clone();
    let pending = tokio::spawn(async move {
        crate::api::plan_routes::authorize_plan_orchestrator_http(
            &pending_service,
            &pending_plan,
            owner,
        )
        .await
    });
    tokio::task::yield_now().await;
    assert!(!pending.is_finished());

    crate::api::plan_routes::authorize_plan_orchestrator_http(
        &std::sync::Arc::new(fixture.service.clone()),
        &plan_id,
        AuthenticatedPlanCaller::default(),
    )
    .await
    .expect("a concurrent user plan write can complete while the launch binding is pending");

    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&owner_tab_id).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    crate::service::general_agent::binding_wait::notify_binding_state_changed();
    pending
        .await
        .expect("pending HTTP authorization task must finish")
        .expect("binding event must release the pending authorization");
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn only_live_plan_owner_can_grant_taskless_evidence_capability() {
    let (fixture, plan_id, planner, owner) = fixture();
    let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
    let reporter = fixture.service.mux.write().create_tab(&workspace).unwrap();
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(&reporter).unwrap();
        tab.agent_session_id = Some("evidence-session".into());
        tab.session_binding_nonce = Some("evidence-nonce".into());
        tab.reported_session_binding_nonce = Some("evidence-nonce".into());
    }

    let refused = fixture.service.grant_plan_evidence_reporter(
        &plan_id,
        &reporter,
        crate::plan::PlanEvidenceClass::Review,
        &planner,
        2,
    );
    assert!(refused.is_err());
    assert!(fixture
        .service
        .plan_store
        .get(&plan_id)
        .unwrap()
        .evidence_reporters
        .is_empty());

    let granted = fixture
        .service
        .grant_plan_evidence_reporter(
            &plan_id,
            &reporter,
            crate::plan::PlanEvidenceClass::CanonicalQa,
            &owner,
            3,
        )
        .unwrap();
    assert!(granted.created);
    assert_eq!(granted.grant.tab_id, reporter);
    assert_eq!(granted.grant.session_id, "evidence-session");
    assert_eq!(
        granted.grant.session_binding_digest,
        crate::service::plan::review_gate::plan_evidence_binding_digest("evidence-nonce")
    );

    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "bound reporter".into(),
                ..Default::default()
            },
            4,
        )
        .unwrap();
    fixture
        .service
        .plan_store
        .assign_task(&plan_id, &task_id, &reporter, None, 5)
        .unwrap();
    assert!(fixture
        .service
        .grant_plan_evidence_reporter(
            &plan_id,
            &reporter,
            crate::plan::PlanEvidenceClass::CanonicalQa,
            &owner,
            6,
        )
        .is_err());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn draft_before_approval_is_planner_writable_but_not_dispatchable() {
    let (fixture, plan_id, planner, _) = fixture();
    assert!(matches!(
        fixture
            .service
            .authorize_draft_structure_mutation(&plan_id, &planner),
        Ok(PlanningAuthor::Planner { .. })
    ));
    fixture
        .service
        .plan_store
        .add_section(&plan_id, "Phase", "Draft", None, &[], None, None, 2)
        .unwrap();
    fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Draft task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            3,
        )
        .unwrap();
    assert!(matches!(
        fixture.service.ensure_task_dispatchable(&plan_id),
        Err(PlanError::TaskDispatchRequiresApproval)
    ));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn bound_nonce_resolves_live_planner_session_when_shell_env_lacks_session_id() {
    let (fixture, plan_id, mut planner, _) = fixture();
    planner.session_id = None;
    assert!(matches!(
        fixture
            .service
            .authorize_draft_structure_mutation(&plan_id, &planner),
        Ok(PlanningAuthor::Planner { .. })
    ));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn orchestrator_approval_atomically_activates_draft_graph() {
    let (fixture, plan_id, planner, owner) = fixture();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Draft task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert!(fixture
        .service
        .approve_and_activate_plan(&plan_id, &planner, None, 3)
        .is_err());
    fixture
        .service
        .approve_and_activate_plan(&plan_id, &owner, None, 4)
        .unwrap();
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.planning_state, PlanningState::Approved);
    assert_eq!(plan.mode, PlanMode::Approved);
    assert_eq!(plan.planning_log.len(), 1);
    assert_eq!(
        plan.tasks
            .iter()
            .find(|task| task.id == task_id)
            .unwrap()
            .status,
        TaskStatus::Pending
    );
    assert!(fixture.service.ensure_task_dispatchable(&plan_id).is_ok());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn planner_cannot_approve_draft() {
    let (fixture, plan_id, planner, _) = fixture();

    let result = fixture
        .service
        .approve_and_activate_plan(&plan_id, &planner, None, 3);

    assert!(result.is_err());
    let plan = fixture.service.plan_store.get(&plan_id).unwrap();
    assert_eq!(plan.planning_state, PlanningState::Drafting);
    assert_eq!(plan.mode, PlanMode::Planning);
    assert!(plan.planning_log.is_empty());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn planner_cannot_add_orchestrator_heartbeat_learning() {
    let (fixture, plan_id, planner, _) = fixture();

    let result = fixture.service.add_orchestrator_heartbeat_learning(
        &plan_id,
        &planner,
        "opaque-learning",
        None,
        3,
    );

    assert!(matches!(
        result,
        Err(OrchestratorLearningError::Authority(_))
    ));
    assert!(fixture
        .service
        .plan_store
        .get(&plan_id)
        .unwrap()
        .heartbeat
        .learnings
        .is_empty());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn draft_execution_actions_and_mode_activation_fail_before_approval() {
    let (fixture, plan_id, _, owner) = fixture();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Assignment target".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert!(fixture
        .service
        .authorize_execution_action(&plan_id, &owner)
        .is_err());
    assert!(fixture
        .service
        .enter_execution_mode(&plan_id, &owner, 2)
        .is_err());
    assert!(fixture
        .service
        .authorize_task_execution_action(&task_id, &owner)
        .is_err());
    assert_eq!(
        fixture.service.plan_store.get(&plan_id).unwrap().mode,
        PlanMode::Planning
    );
}

#[test]
fn planner_task_updates_reject_execution_lifecycle_fields() {
    let author = PlanningAuthor::Planner {
        tab_id: "planner".into(),
    };
    for field in [
        "status",
        "status_label",
        "lane",
        "assign_tab",
        "assigned_tab_id",
        "tab",
        "personality_provenance",
        "attempt",
        "attempt_id",
        "attempts",
        "competition",
        "review_stage",
        "qa_state",
    ] {
        let refusal = ensure_planner_task_fields_are_structural(
            &author,
            &serde_json::json!({ (field): "forbidden" }),
        )
        .expect_err("execution lifecycle field must stay Orchestrator-owned");
        assert!(refusal.contains(field), "refusal must name the field");
    }
    assert!(ensure_planner_task_fields_are_structural(
        &author,
        &serde_json::json!({ "name": "Structural", "depends_on": [] })
    )
    .is_ok());
}

/// Routing IS planning: a draft Planner cuts the task and its worker
/// persona in one call, so these fields are outside the refusal list.
#[test]
fn planner_task_writes_accept_routing_fields() {
    let author = PlanningAuthor::Planner {
        tab_id: "planner".into(),
    };
    for field in ["executor_role", "personality", "role_guidance"] {
        assert!(
            ensure_planner_task_fields_are_structural(
                &author,
                &serde_json::json!({ (field): "routing" })
            )
            .is_ok(),
            "draft Planner must be able to set {field}"
        );
    }
    assert!(ensure_planner_task_fields_are_structural(
        &author,
        &serde_json::json!({
            "name": "Routed",
            "executor_role": "prompt-methodology-dev",
            "personality": "Nora",
            "role_guidance": "You are Nora.",
        })
    )
    .is_ok());
}

/// The refusal list is Planner-scoped only. The Plan Orchestrator classifies
/// as `User`, so its authority over every field is unchanged.
#[test]
fn orchestrator_authority_over_task_fields_is_unrestricted() {
    let author = PlanningAuthor::User;
    for field in [
        "status",
        "lane",
        "assign_tab",
        "attempt_id",
        "qa_state",
        "executor_role",
        "personality",
        "role_guidance",
        "personality_provenance",
    ] {
        assert!(ensure_planner_task_fields_are_structural(
            &author,
            &serde_json::json!({ (field): "allowed" })
        )
        .is_ok());
    }
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn task_take_and_drop_require_approved_graph_and_live_orchestrator_identity() {
    let (fixture, plan_id, _, owner) = fixture();
    let task_id = fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Take or drop".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            2,
        )
        .unwrap();
    assert!(fixture
        .service
        .authorize_task_execution_action(&task_id, &owner)
        .is_err());
    fixture
        .service
        .approve_and_activate_plan(&plan_id, &owner, None, 3)
        .unwrap();
    let stale = AuthenticatedPlanCaller {
        binding_nonce: Some("replayed-nonce".into()),
        ..owner.clone()
    };
    assert!(fixture
        .service
        .authorize_task_execution_action(&task_id, &stale)
        .is_err());
    assert!(fixture
        .service
        .authorize_task_execution_action(&task_id, &owner)
        .is_ok());
}

fn is_binding_rule(error: PlanAuthorityError) -> bool {
    matches!(
        error,
        PlanAuthorityError::BindingStaleAfterRelaunch | PlanAuthorityError::BindingForeign
    )
}

/// Relaunch = resume the SAME session on a fresh nonce; the prior nonce is
/// retired into lineage tagged with the (unchanged) current session.
fn relaunch_tab(fixture: &ServiceFixture, tab_id: &str, new_nonce: &str) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.remember_superseded_binding_nonce();
    tab.session_binding_nonce = Some(new_nonce.into());
    tab.reported_session_binding_nonce = Some(new_nonce.into());
}

/// Genuine session transition: retire the current nonce under the OLD session,
/// then move the live tab onto a brand-new session id + nonce.
fn transition_session(fixture: &ServiceFixture, tab_id: &str, new_session: &str, new_nonce: &str) {
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(tab_id).unwrap();
    tab.remember_superseded_binding_nonce();
    tab.agent_session_id = Some(new_session.into());
    tab.session_binding_nonce = Some(new_nonce.into());
    tab.reported_session_binding_nonce = Some(new_nonce.into());
}

fn current_binding(fixture: &ServiceFixture, tab_id: &str) -> (Option<String>, Option<String>) {
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(tab_id).unwrap();
    (
        tab.session_binding_nonce.clone(),
        tab.reported_session_binding_nonce.clone(),
    )
}

fn sessionless(caller: &AuthenticatedPlanCaller) -> AuthenticatedPlanCaller {
    AuthenticatedPlanCaller {
        session_id: None,
        ..caller.clone()
    }
}

// Mirror the W3-C task gate so both authority paths are proven on one input.
fn w3c_task_binding_matches(fixture: &ServiceFixture, caller: &AuthenticatedPlanCaller) -> bool {
    let mux = fixture.service.mux.read();
    let tab = mux.get_tab(caller.tab_id.as_deref().unwrap()).unwrap();
    let nonce = caller.binding_nonce.as_deref();
    tab.agent_session_id.as_deref() == caller.session_id.as_deref()
        && tab.session_binding_nonce.as_deref() == nonce
        && tab.reported_session_binding_nonce.as_deref() == nonce
}

#[test]
fn lineage_is_session_scoped_bounded_and_deduped() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.agent_session_id = Some("S".into());
    tab.session_binding_nonce = Some("n0".into());
    tab.remember_superseded_binding_nonce();
    tab.session_binding_nonce = Some("n1".into());
    tab.remember_superseded_binding_nonce();
    tab.session_binding_nonce = Some("n0".into());
    tab.remember_superseded_binding_nonce();
    // Under the SAME current session, prior and current nonces prove lineage.
    assert!(tab.binding_nonce_in_current_session_lineage("n0"));
    assert!(tab.binding_nonce_in_current_session_lineage("n1"));
    assert!(!tab.binding_nonce_in_current_session_lineage("never-issued"));
    assert_eq!(
        tab.prior_session_binding_nonces
            .iter()
            .filter(|entry| entry.nonce == "n0")
            .count(),
        1,
        "re-adopting a nonce must not duplicate its lineage entry"
    );
    // A genuine transition: the SAME nonces no longer prove lineage, because
    // their recorded session != the tab's new current session.
    tab.agent_session_id = Some("S2".into());
    tab.session_binding_nonce = Some("n2".into());
    assert!(!tab.binding_nonce_in_current_session_lineage("n0"));
    assert!(!tab.binding_nonce_in_current_session_lineage("n1"));
    // No current session proves nothing.
    tab.agent_session_id = None;
    assert!(!tab.binding_nonce_in_current_session_lineage("n2"));
    // Bounded under a single session.
    tab.agent_session_id = Some("S".into());
    for index in 0..20 {
        tab.session_binding_nonce = Some(format!("m{index}"));
        tab.remember_superseded_binding_nonce();
    }
    assert!(tab.prior_session_binding_nonces.len() <= 8);
    assert!(!tab.binding_nonce_in_current_session_lineage("m0"));
    assert!(tab.binding_nonce_in_current_session_lineage("m19"));
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn sessionless_relaunch_caller_recovers_mutation_approve_dispatch_through_rebind() {
    let (fixture, plan_id, _, owner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    relaunch_tab(&fixture, &tab_id, "owner-nonce-relaunched");
    // The real relaunched CLI environment has the nonce but no session id;
    // daemon-tagged lineage proves continuity.
    let caller = sessionless(&owner);
    assert!(matches!(
        fixture.service.classify_planning_caller(&plan_id, &caller),
        Err(PlanAuthorityError::BindingStaleAfterRelaunch)
    ));
    let outcome = fixture
        .service
        .rebind_stale_planning_session(&caller)
        .expect("a provable same-session relaunch caller can rebind without a session id");
    assert!(outcome.resynced);
    // Mutation, approve, and dispatch all funnel through the authority gate,
    // so the single rebind recovers every one of them without a restart.
    assert!(matches!(
        fixture.service.classify_planning_caller(&plan_id, &caller),
        Ok(PlanningAuthor::User)
    ));
    assert!(fixture
        .service
        .approve_and_activate_plan(&plan_id, &caller, None, 5)
        .is_ok());
    assert!(fixture
        .service
        .authorize_execution_action(&plan_id, &caller)
        .is_ok());
    // A second rebind by the now-current caller is an idempotent no-op.
    let repeat = fixture
        .service
        .rebind_stale_planning_session(&caller)
        .expect("idempotent rebind");
    assert!(!repeat.resynced);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn sessionless_stale_nonce_is_refused_after_a_genuine_session_transition() {
    let (fixture, plan_id, _, owner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    // owner-nonce is retired under the ORIGINAL session, then the tab moves to
    // a genuinely NEW session — the retired nonce is now cross-session.
    transition_session(&fixture, &tab_id, "owner-session-2", "owner-nonce-2");
    let caller = sessionless(&owner); // still presents the old owner-nonce
    assert!(matches!(
        fixture.service.classify_planning_caller(&plan_id, &caller),
        Err(PlanAuthorityError::BindingForeign)
    ));
    let before = current_binding(&fixture, &tab_id);
    assert!(fixture
        .service
        .rebind_stale_planning_session(&caller)
        .is_err());
    // A refusal must never mutate the live current/reported binding.
    assert_eq!(current_binding(&fixture, &tab_id), before);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn mismatched_provided_session_is_refused_even_with_a_lineage_nonce() {
    let (fixture, plan_id, _, owner) = fixture();
    let tab_id = owner.tab_id.clone().unwrap();
    relaunch_tab(&fixture, &tab_id, "owner-nonce-relaunched");
    // A provided-but-wrong session is refused: it is not laundered, and it is
    // not silently dropped in favour of the lineage proof either.
    let wrong_session = AuthenticatedPlanCaller {
        session_id: Some("some-other-session".into()),
        ..owner.clone()
    };
    assert!(matches!(
        fixture
            .service
            .classify_planning_caller(&plan_id, &wrong_session),
        Err(PlanAuthorityError::BindingForeign)
    ));
    let before = current_binding(&fixture, &tab_id);
    assert!(fixture
        .service
        .rebind_stale_planning_session(&wrong_session)
        .is_err());
    assert_eq!(current_binding(&fixture, &tab_id), before);
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn foreign_and_cross_tab_callers_are_refused_and_rebind_rejects_them() {
    let (fixture, plan_id, planner, owner) = fixture();
    relaunch_tab(&fixture, owner.tab_id.as_deref().unwrap(), "owner-nonce-2");
    // A nonce this tab never held has no lineage — foreign, and unrebindable.
    let foreign = AuthenticatedPlanCaller {
        binding_nonce: Some("never-issued-nonce".into()),
        ..owner.clone()
    };
    assert!(matches!(
        fixture.service.classify_planning_caller(&plan_id, &foreign),
        Err(PlanAuthorityError::BindingForeign)
    ));
    assert!(fixture
        .service
        .rebind_stale_planning_session(&foreign)
        .is_err());
    // Another tab cannot launder the owner's stale nonce because its lineage
    // never held that nonce.
    let cross_tab = AuthenticatedPlanCaller {
        tab_id: planner.tab_id.clone(),
        session_id: Some("planner-session".into()),
        binding_nonce: Some("owner-nonce".into()),
    };
    assert!(matches!(
        fixture
            .service
            .classify_planning_caller(&plan_id, &cross_tab),
        Err(PlanAuthorityError::BindingForeign)
    ));
    assert!(fixture
        .service
        .rebind_stale_planning_session(&cross_tab)
        .is_err());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn planner_structure_authority_survives_approval() {
    // Andrey's ruling (rel#54): a bound Planner MAY mutate structure after
    // approval. Only approval/dispatch stay Orchestrator-exclusive.
    let (fixture, plan_id, planner, owner) = fixture();
    fixture
        .service
        .approve_and_activate_plan(&plan_id, &owner, None, 3)
        .unwrap();
    assert!(matches!(
        fixture
            .service
            .authorize_draft_structure_mutation(&plan_id, &planner),
        Ok(PlanningAuthor::Planner { .. })
    ));
    fixture
        .service
        .plan_store
        .add_section(
            &plan_id,
            "Post-approval phase",
            "Added late",
            None,
            &[],
            None,
            None,
            5,
        )
        .expect("a bound Planner may add sections after approval");
    fixture
        .service
        .plan_store
        .add_task(
            &plan_id,
            NewTask {
                name: "Post-approval task".into(),
                kind: "implement".into(),
                ..Default::default()
            },
            6,
        )
        .expect("a bound Planner may add tasks after approval");
    assert!(
        fixture
            .service
            .authorize_plan_orchestrator(&plan_id, &planner)
            .is_err(),
        "approval/dispatch authority stays Orchestrator-exclusive for a Planner"
    );
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn binding_rule_and_role_rule_are_mechanically_distinct() {
    let (fixture, plan_id, planner, owner) = fixture();
    // Role rule: a live, correctly-bound tab that is neither the plan
    // owner nor a `planner`-profiled Manager fails the ROLE rule.
    {
        let mut mux = fixture.service.mux.write();
        let tab = mux.get_tab_mut(planner.tab_id.as_deref().unwrap()).unwrap();
        tab.role = None;
        tab.role_profile = None;
    }
    let role_err = match fixture.service.classify_planning_caller(&plan_id, &planner) {
        Err(error) => error,
        Ok(_) => panic!("a non-owner, non-planner caller must fail the role rule"),
    };
    assert_eq!(role_err, PlanAuthorityError::RoleUnauthorized);
    // Binding rule: a foreign nonce is refused by the BINDING rule.
    let foreign = AuthenticatedPlanCaller {
        binding_nonce: Some("foreign".into()),
        ..owner.clone()
    };
    let binding_err = match fixture.service.classify_planning_caller(&plan_id, &foreign) {
        Err(error) => error,
        Ok(_) => panic!("a foreign nonce must fail the binding rule"),
    };
    // Mechanism, not prose: distinct variants AND distinct diagnostic types.
    assert!(is_binding_rule(binding_err));
    assert!(!is_binding_rule(role_err));
    assert_ne!(role_err.diagnostic_type(), binding_err.diagnostic_type());
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn one_rebind_heals_both_the_plan_gate_and_the_w3c_task_gate() {
    let (fixture, plan_id, _, owner) = fixture();
    relaunch_tab(&fixture, owner.tab_id.as_deref().unwrap(), "owner-nonce-3");
    // Before rebind BOTH gates reject the same stale caller — one coherent
    // stale-binding verdict, not two fighting predicates.
    let before = match fixture.service.classify_planning_caller(&plan_id, &owner) {
        Err(error) => error,
        Ok(_) => panic!("stale caller must be rejected before rebind"),
    };
    assert!(is_binding_rule(before));
    assert!(!w3c_task_binding_matches(&fixture, &owner));
    fixture
        .service
        .rebind_stale_planning_session(&owner)
        .unwrap();
    // After ONE rebind BOTH gates accept the same caller.
    assert!(matches!(
        fixture.service.classify_planning_caller(&plan_id, &owner),
        Ok(PlanningAuthor::User)
    ));
    assert!(w3c_task_binding_matches(&fixture, &owner));
}

/// The person at the machine outranks every agent role. Identity here is built
/// from headers an agent sets and the request has already cleared the API
/// token, so a caller presenting no binding at all is the owner working through
/// the UI or the CLI — not an agent whose binding went missing. Without this
/// the owner could not approve their own draft from the app at all (#517).
#[test]
#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
fn an_unbound_authenticated_caller_is_the_owner() {
    let (fixture, plan_id, _owner, planner) = fixture();
    let unbound = AuthenticatedPlanCaller {
        tab_id: None,
        session_id: None,
        binding_nonce: None,
    };

    assert_eq!(
        fixture
            .service
            .classify_planning_caller(&plan_id, &unbound)
            .expect("the owner is authorized"),
        crate::plan::PlanningAuthor::User,
    );
    fixture
        .service
        .authorize_plan_orchestrator(&plan_id, &unbound)
        .expect("and holds execution authority, which approval requires");

    // A caller that does present a tab still proves that binding, so the
    // unbound path cannot be used to launder a half-presented agent identity.
    let half_presented = AuthenticatedPlanCaller {
        binding_nonce: None,
        ..planner.clone()
    };
    assert!(
        fixture
            .service
            .classify_planning_caller(&plan_id, &half_presented)
            .is_err(),
        "a tab without its binding proof is still refused"
    );
}

mod lifecycle;
