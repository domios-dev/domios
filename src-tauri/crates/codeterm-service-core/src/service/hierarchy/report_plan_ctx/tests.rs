//! A stale task id must never transition the wrong task.
//!
//! The resolver cases below fix the predicate; the `ServiceContext` cases fix the
//! observable end state — that a refused report writes NO status and NO milestone
//! while the report itself still reaches the parent.

use super::{
    resolve_plan_ctx, PLANNER_AUTHORITY_REFUSAL, PLAN_EVIDENCE_AUTHORITY_REFUSAL,
    STALE_TASK_HINT_REFUSAL,
};
use crate::agent::reports::{ReportDetails, ReportStatus, BLOCK_PROPOSAL_NOTICE};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{
    AgentRef, OrchestratorRef, Plan, PlanEvidenceClass, PlannerRef, Task, TaskStatus,
};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::test_fixture::{RecordedAgentDeliveries, ServiceFixture};
use crate::service::AuthenticatedPlanCaller;
use codeterm_types::agent::ReportOutcomeClass;

mod desktop_reports;
mod plan_evidence_owner_test;
mod plan_evidence_role_test;
mod plan_evidence_session_test;
mod plan_evidence_status_test;

fn add_task(store: &PlanStore, plan_id: &str, name: &str, now_ms: u64) -> String {
    store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["binding is validated".into()],
                test_plan: Some("focused regression".into()),
                ..Default::default()
            },
            now_ms,
        )
        .unwrap()
}

// ── resolver predicate ──────────────────────────────────────────────────────

struct Holdings {
    _dir: tempfile::TempDir,
    store: PlanStore,
    plan_id: String,
    held: String,
    other: String,
}

/// `worker` holds `held`. `other` belongs to a different tab — the shape a
/// reporting shell's stale `CODETERM_TASK_ID` points at once the Orchestrator
/// has moved that work elsewhere.
fn holdings(worker: &str, other_was_ours: bool) -> Holdings {
    let dir = tempfile::tempdir().unwrap();
    let store = PlanStore::new_for_test(dir.path());
    let mut plan = Plan::new(
        "stale hint".into(),
        dir.path().display().to_string(),
        OrchestratorRef::from_tab("orch"),
    );
    store.save(&mut plan, 1).unwrap();
    let other = add_task(&store, &plan.id, "moved on", 2);
    let held = add_task(&store, &plan.id, "actually held", 2);
    if other_was_ours {
        store
            .assign_task(&plan.id, &other, worker, None, 3)
            .unwrap();
    }
    store
        .assign_task(&plan.id, &other, "someone-else", None, 4)
        .unwrap();
    store.assign_task(&plan.id, &held, worker, None, 5).unwrap();
    Holdings {
        _dir: dir,
        store,
        plan_id: plan.id,
        held,
        other,
    }
}

#[test]
fn stale_task_report_hint_for_a_task_the_tab_lost_is_refused() {
    let fx = holdings("worker", false);
    let mut notices = Vec::new();
    let error = resolve_plan_ctx(
        &fx.store,
        "worker",
        Some(&fx.plan_id),
        Some(&fx.other),
        &mut notices,
    )
    .unwrap_err();

    assert!(error.contains(STALE_TASK_HINT_REFUSAL));
    assert!(
        error.contains(&fx.other),
        "must name the stale task: {error}"
    );
    assert!(error.contains("worker"), "must name the caller: {error}");
    assert!(
        error.contains(&fx.held),
        "must name what the tab actually holds: {error}"
    );
}

#[test]
fn stale_task_report_hint_from_a_tab_with_no_binding_is_refused() {
    let fx = holdings("worker", false);
    let mut notices = Vec::new();
    let error = resolve_plan_ctx(
        &fx.store,
        "promoted-orchestrator",
        Some(&fx.plan_id),
        Some(&fx.held),
        &mut notices,
    )
    .unwrap_err();

    assert!(error.contains(STALE_TASK_HINT_REFUSAL));
    assert!(error.contains(&fx.held));
    assert!(error.contains("promoted-orchestrator"));
}

/// A reassignment used to push a new attempt without closing the old one, leaving
/// the tab an open attempt on a task it no longer holds. Reassignment now closes
/// it, and the refusal must hold either way.
#[test]
fn stale_task_report_lingering_open_attempt_cannot_launder_a_hint() {
    let fx = holdings("worker", true);
    let other = fx
        .store
        .get(&fx.plan_id)
        .unwrap()
        .tasks
        .into_iter()
        .find(|task| task.id == fx.other)
        .unwrap();
    // Reassignment closes the outgoing attempt, so the residue this refusal was
    // built to survive no longer exists: the leak is gone at its source.
    assert!(
        !other
            .execution_attempts
            .iter()
            .any(|attempt| { attempt.executor_tab_id == "worker" && attempt.closed_at.is_none() }),
        "reassignment must not leave the previous tab an open attempt"
    );
    assert_eq!(other.assigned_tab_id.as_deref(), Some("someone-else"));

    let mut notices = Vec::new();
    let error = resolve_plan_ctx(
        &fx.store,
        "worker",
        Some(&fx.plan_id),
        Some(&fx.other),
        &mut notices,
    )
    .unwrap_err();
    assert!(error.contains(STALE_TASK_HINT_REFUSAL));
}

#[test]
fn stale_task_report_correct_hint_resolves_unchanged() {
    let fx = holdings("worker", false);
    let mut notices = Vec::new();
    let (plan, task) = resolve_plan_ctx(
        &fx.store,
        "worker",
        Some(&fx.plan_id),
        Some(&fx.held),
        &mut notices,
    )
    .unwrap();

    assert_eq!(plan.as_deref(), Some(fx.plan_id.as_str()));
    assert_eq!(task.as_deref(), Some(fx.held.as_str()));
    assert!(notices.is_empty());
}

#[test]
fn stale_task_report_absent_hint_paths_are_unchanged() {
    let fx = holdings("worker", false);

    let mut notices = Vec::new();
    let (plan, task) =
        resolve_plan_ctx(&fx.store, "worker", Some(&fx.plan_id), None, &mut notices).unwrap();
    assert_eq!(plan.as_deref(), Some(fx.plan_id.as_str()));
    assert_eq!(task, None, "a plan hint without a task hint is untouched");
    assert!(notices.is_empty());

    let mut notices = Vec::new();
    let (plan, task) =
        resolve_plan_ctx(&fx.store, "unbound-tab", None, None, &mut notices).unwrap();
    assert_eq!((plan, task), (None, None));
    assert!(notices.is_empty());

    // A plan the local store does not know still falls through to the tab's own
    // binding, notice and all — the mesh homing path is untouched.
    let mut notices = Vec::new();
    let (plan, task) = resolve_plan_ctx(
        &fx.store,
        "worker",
        Some("pl_not_here"),
        Some(&fx.other),
        &mut notices,
    )
    .unwrap();
    assert_eq!(plan.as_deref(), Some(fx.plan_id.as_str()));
    assert_eq!(task.as_deref(), Some(fx.held.as_str()));
    assert!(notices[0].contains("pl_not_here"));
}

// ── end state: nothing transitions ──────────────────────────────────────────

struct ReportHarness {
    fx: ServiceFixture,
    _root_recipient: RecordedAgentDeliveries,
    root: String,
    caller: String,
    plan_id: String,
    held: String,
    stale: String,
}

impl ReportHarness {
    fn task(&self, task_id: &str) -> Task {
        self.fx
            .service
            .plan_store
            .get(&self.plan_id)
            .unwrap()
            .tasks
            .into_iter()
            .find(|task| task.id == task_id)
            .unwrap()
    }

    /// Attribution as a comparable fact rather than a literal id: two fixtures
    /// mint different tab ids, but "attributed to my own caller" is the property
    /// under test. A milestone credited to any other tab, or to none, stays
    /// distinguishable — that is the wrong-tab write this must still catch.
    fn attribution(&self, from: &AgentRef) -> String {
        match from.tab_id.as_deref() {
            Some(tab) if tab == self.caller => "self".into(),
            Some(tab) => format!("other:{tab}"),
            None => "unattributed".into(),
        }
    }

    /// Everything a transition moves, minus the wall clock and the minted ids
    /// the fixture cannot hold still: status, then each milestone in order.
    fn shape(&self, task_id: &str) -> String {
        let task = self.task(task_id);
        let milestones = task
            .milestones
            .iter()
            .map(|item| {
                format!(
                    "{}|{}|{}",
                    item.kind,
                    item.text,
                    self.attribution(&item.from)
                )
            })
            .collect::<Vec<_>>()
            .join(";");
        format!("{:?}/{}", task.status, milestones)
    }

    fn reports(&self) -> String {
        serde_json::to_string(&self.fx.service.report_store.lock().get(&self.root, false)).unwrap()
    }

    fn details(&self, status: ReportStatus) -> ReportDetails {
        ReportDetails {
            status: Some(status),
            caller_tab_id: Some(self.caller.clone()),
            caller_session_id: Some("caller-session".into()),
            session_binding_nonce: Some("caller-nonce".into()),
            owner_epoch: Some(self.task(&self.held).interruption.owner_epoch),
            ..Default::default()
        }
    }
}

/// A Worker under a root Orchestrator, holding `held`, while `stale` has moved
/// on to another tab.
fn worker_harness() -> ReportHarness {
    seed_plan(worker_mux(), None, Holding::Held)
}

/// The read-only verification Worker: same placement under the Orchestrator,
/// but assigned nothing. Owning no task is the whole of what stops it
/// transitioning one, so the fixture differs from `worker_harness` only there.
fn verification_worker_harness() -> ReportHarness {
    seed_plan(worker_mux(), None, Holding::Nothing)
}

fn worker_mux() -> (MultiplexerState, String, String) {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let caller = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&caller, AgentRole::Worker, Some(&root))
        .expect("attach worker to orchestrator");
    mux.get_tab_mut(&caller).unwrap().agent_type = Some("claude".into());
    (mux, root, caller)
}

/// What the reporting tab owns in the seeded plan.
#[derive(Clone, Copy, PartialEq)]
enum Holding {
    Held,
    Nothing,
}

/// Binds the caller's live session on both sides of the seam — the tab record
/// and the attempt the plan write authorizes against — so the nonce and epoch
/// gates pass and only the task-hold predicate can refuse.
fn seed_plan(
    (mux, root, caller): (MultiplexerState, String, String),
    owner: Option<&str>,
    holding: Holding,
) -> ReportHarness {
    let fx = ServiceFixture::new(mux);
    let root_recipient = fx.bind_recorded_agent_recipient(&root, "root-session", "root-nonce");
    {
        let mut mux = fx.service.mux.write();
        let tab = mux.get_tab_mut(&caller).unwrap();
        tab.agent_session_id = Some("caller-session".into());
        tab.session_binding_nonce = Some("caller-nonce".into());
        tab.reported_session_binding_nonce = Some("caller-nonce".into());
    }
    let store = &fx.service.plan_store;
    let mut plan = Plan::new(
        "stale hint".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(owner.unwrap_or(root.as_str())),
    );
    store.save(&mut plan, 1).unwrap();
    let stale = add_task(store, &plan.id, "moved on", 2);
    let held = add_task(store, &plan.id, "actually held", 2);
    store
        .assign_task(&plan.id, &stale, "someone-else", None, 3)
        .unwrap();
    if holding == Holding::Held {
        store
            .assign_task(&plan.id, &held, &caller, None, 4)
            .unwrap();
        store
            .update_attempt_executor_identity(
                &plan.id,
                &held,
                &caller,
                None,
                Some("caller-session"),
                Some("caller-nonce"),
                5,
            )
            .unwrap();
    }
    ReportHarness {
        fx,
        _root_recipient: root_recipient,
        root,
        caller,
        plan_id: plan.id,
        held,
        stale,
    }
}

#[test]
fn stale_task_report_transitions_nothing_and_names_the_mismatch() {
    let h = worker_harness();
    let held_before = h.shape(&h.held);
    let stale_before = h.shape(&h.stale);

    let result =
        h.fx.service
            .submit_report(
                &h.caller,
                "STALE_REPORT_PAYLOAD",
                h.details(ReportStatus::InReview),
                Some(&h.plan_id),
                Some(&h.stale),
                None,
            )
            .expect("a refused transition must not swallow the report");

    let rejection = result
        .rejected_task_transition
        .expect("the transition must be refused, not silently applied");
    assert_eq!(
        rejection.report_outcome,
        Some(ReportOutcomeClass::StaleBindingConflict)
    );
    assert!(rejection.cause.contains(STALE_TASK_HINT_REFUSAL));
    assert!(
        rejection.cause.contains(&h.stale) && rejection.cause.contains(&h.caller),
        "the refusal must name both sides of the mismatch: {}",
        rejection.cause
    );
    assert!(result.persisted_task.is_none());

    // No status write and no milestone write — on either task.
    assert_eq!(h.shape(&h.stale), stale_before);
    assert_eq!(h.shape(&h.held), held_before);
    assert!(h.task(&h.stale).milestones.is_empty());
    assert!(h.task(&h.held).milestones.is_empty());

    // The report itself still reached the parent, so it must not be resent.
    assert!(h.reports().contains("STALE_REPORT_PAYLOAD"));
}

#[test]
fn stale_task_report_correct_binding_matches_the_absent_hint_shape() {
    let hinted = worker_harness();
    hinted
        .fx
        .service
        .submit_report(
            &hinted.caller,
            "CORRECT_BINDING_PAYLOAD",
            hinted.details(ReportStatus::InReview),
            Some(&hinted.plan_id),
            Some(&hinted.held),
            None,
        )
        .unwrap();

    let implicit = worker_harness();
    implicit
        .fx
        .service
        .submit_report(
            &implicit.caller,
            "CORRECT_BINDING_PAYLOAD",
            implicit.details(ReportStatus::InReview),
            None,
            None,
            None,
        )
        .unwrap();

    // Anchor the normalized attribution to a real id, so "self" cannot become a
    // label both fixtures agree on while the write went to the wrong tab.
    for h in [&hinted, &implicit] {
        assert_eq!(
            h.task(&h.held).milestones[0].from.tab_id.as_deref(),
            Some(h.caller.as_str()),
            "the transition must be attributed to the reporting tab itself"
        );
    }

    assert_eq!(
        hinted.shape(&hinted.held),
        implicit.shape(&implicit.held),
        "an explicit hint for the held task must transition exactly like no hint at all"
    );
    assert_eq!(hinted.task(&hinted.held).status, TaskStatus::InReview);
    assert_eq!(hinted.shape(&hinted.stale), implicit.shape(&implicit.stale));
}

#[test]
fn stale_task_report_absent_hint_still_transitions_the_held_task() {
    let h = worker_harness();
    let stale_before = h.shape(&h.stale);

    let result =
        h.fx.service
            .submit_report(
                &h.caller,
                "ABSENT_HINT_PAYLOAD",
                h.details(ReportStatus::InReview),
                None,
                None,
                None,
            )
            .unwrap();

    assert!(result.rejected_task_transition.is_none());
    assert_eq!(
        result.persisted_task,
        Some((h.plan_id.clone(), h.held.clone()))
    );
    assert_eq!(h.task(&h.held).status, TaskStatus::InReview);
    assert_eq!(h.shape(&h.stale), stale_before);
}

#[test]
fn blocked_worker_report_is_one_persisted_proposal_and_keeps_task_in_progress() {
    let h = worker_harness();
    let attempt_id = h.task(&h.held).execution_attempts[0].id.clone();
    h.fx.service
        .plan_store
        .acknowledge_task_attempt(&h.plan_id, &h.held, &h.caller, &attempt_id, 6)
        .expect("the Worker has started its assigned task");
    let before = h.task(&h.held);
    assert_eq!(before.status, TaskStatus::InProgress);
    assert_eq!(before.status_label.as_deref(), Some("acknowledged"));

    let mut details = h.details(ReportStatus::Blocked);
    details.test_cmd = Some("dependency-probe".into());
    details.test_result = Some("dependency unavailable".into());

    let result =
        h.fx.service
            .submit_report(
                &h.caller,
                "dependency unavailable",
                details,
                Some(&h.plan_id),
                Some(&h.held),
                None,
            )
            .expect("a blocked report is an accepted proposal");

    assert!(result.rejected_task_transition.is_none());
    assert_eq!(
        result.persisted_task,
        Some((h.plan_id.clone(), h.held.clone()))
    );
    assert_eq!(
        result.report_outcome,
        Some(ReportOutcomeClass::BlockProposed)
    );

    let task = h.task(&h.held);
    assert_eq!(task.status, TaskStatus::InProgress);
    assert_eq!(task.status_label, before.status_label);
    assert_eq!(task.milestones.len(), 1);
    assert!(task.milestones[0].text.starts_with("BLOCK PROPOSAL: "));
    let snapshot = task.execution_attempts[0].report.as_ref().unwrap();
    assert_eq!(snapshot.status.as_deref(), Some("blocked"));
    assert_eq!(snapshot.message.as_deref(), Some("dependency unavailable"));
    assert_eq!(snapshot.test_cmd.as_deref(), Some("dependency-probe"));
    assert_eq!(
        snapshot.test_result.as_deref(),
        Some("dependency unavailable")
    );

    let reports = h.fx.service.report_store.lock().get(&h.root, false);
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].report_outcome,
        Some(ReportOutcomeClass::BlockProposed)
    );
    assert!(reports[0].inject_body().contains(BLOCK_PROPOSAL_NOTICE));
}

/// The verification Worker's safety is its emptiness, not a second gate: a tab
/// that holds no task can name one and still transition nothing. Pinned here so
/// permitting the task-less read-only spawn cannot quietly grow it a hold.
#[test]
fn a_verification_worker_holds_no_task_and_can_transition_none() {
    let h = verification_worker_harness();
    let held_before = h.shape(&h.held);
    let stale_before = h.shape(&h.stale);

    // Naming a task explicitly.
    let named =
        h.fx.service
            .submit_report(
                &h.caller,
                "VERIFIER_NAMED_TASK",
                h.details(ReportStatus::InReview),
                Some(&h.plan_id),
                Some(&h.held),
                None,
            )
            .expect("a refused transition must not swallow the report");
    let rejection = named
        .rejected_task_transition
        .expect("a tab that holds nothing must not transition anything");
    assert!(rejection.cause.contains(STALE_TASK_HINT_REFUSAL));
    assert!(named.persisted_task.is_none());

    // Naming none at all: the report is accepted with no task involved.
    let unnamed =
        h.fx.service
            .submit_report(
                &h.caller,
                "VERIFIER_GATE_RESULT",
                h.details(ReportStatus::InReview),
                None,
                None,
                None,
            )
            .expect("a task-less report still reaches the parent");
    assert!(unnamed.persisted_task.is_none());

    assert_eq!(h.shape(&h.held), held_before);
    assert_eq!(h.shape(&h.stale), stale_before);
    assert!(h.task(&h.held).milestones.is_empty());
    assert!(h.task(&h.stale).milestones.is_empty());
    assert!(h.reports().contains("VERIFIER_GATE_RESULT"));
}

/// The B2 hazard. B2 widens a recovery hatch for a live Plan owner acting on a
/// task whose owner session is gone; that hatch sits behind the dispatch gate
/// for `plan.task.*` verbs, which `agent.report` never enters. Plan ownership
/// must therefore buy nothing here.
#[test]
fn stale_task_report_plan_owner_in_recovery_cannot_land_a_stale_binding() {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let owner = mux.create_tab(&workspace).unwrap();
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.role = Some(AgentRole::Manager);
        tab.role_profile = Some("orchestrator".into());
        tab.parent_tab_id = Some(root.clone());
        tab.team_id = Some("team-recovery".into());
    }
    let owner_id = owner.clone();
    let h = seed_plan((mux, root, owner), Some(owner_id.as_str()), Holding::Held);

    // Recovery-eligible on both halves of B2's predicate: the caller IS the live
    // Plan owner, and the stale task has no live owner session to fence against.
    h.fx.service
        .authorize_plan_orchestrator(
            &h.plan_id,
            &AuthenticatedPlanCaller {
                tab_id: Some(h.caller.clone()),
                session_id: Some("caller-session".into()),
                binding_nonce: Some("caller-nonce".into()),
            },
        )
        .expect("the caller must be the live plan owner for this control to mean anything");
    let stale_owner = h.task(&h.stale).interruption.owner_tab_id;
    assert_ne!(
        stale_owner.as_deref(),
        Some(h.caller.as_str()),
        "the stale task's owner session must be gone, or recovery would be moot"
    );

    let stale_before = h.shape(&h.stale);
    let result =
        h.fx.service
            .submit_report(
                &h.caller,
                "RECOVERY_HATCH_PAYLOAD",
                h.details(ReportStatus::InReview),
                Some(&h.plan_id),
                Some(&h.stale),
                None,
            )
            .expect("a refused transition must not swallow the report");

    let rejection = result
        .rejected_task_transition
        .expect("plan ownership must not launder a stale task hint");
    assert!(rejection.cause.contains(STALE_TASK_HINT_REFUSAL));
    assert!(result.persisted_task.is_none());
    assert_eq!(h.shape(&h.stale), stale_before);
    assert!(h.task(&h.stale).milestones.is_empty());
    assert!(h.reports().contains("RECOVERY_HATCH_PAYLOAD"));
}

// ── plan-only reports claim Planner authority (codeterm#371) ───────────────

struct PlannerReportHarness {
    fx: ServiceFixture,
    _root_recipient: RecordedAgentDeliveries,
    root: String,
    planner_tab: String,
    foreign_tab: String,
    plan_id: String,
    foreign_plan_id: String,
}

impl PlannerReportHarness {
    fn details(&self, tab_id: &str, identity: &str) -> ReportDetails {
        ReportDetails {
            caller_tab_id: Some(tab_id.into()),
            caller_session_id: Some(format!("{identity}-session")),
            session_binding_nonce: Some(format!("{identity}-nonce")),
            ..Default::default()
        }
    }

    fn stale_planner_session(&self) {
        let mut mux = self.fx.service.mux.write();
        mux.get_tab_mut(&self.planner_tab)
            .unwrap()
            .reported_session_binding_nonce = Some("stale-nonce".into());
    }

    fn set_registered_planner_session(&self, session_id: Option<&str>) {
        let store = &self.fx.service.plan_store;
        let mut plan = store.get(&self.plan_id).unwrap();
        plan.planner = Some(PlannerRef::Tab {
            tab_id: self.planner_tab.clone(),
            session_id: session_id.map(str::to_string),
            title: None,
        });
        store.save(&mut plan, 99).unwrap();
    }

    fn reports(&self) -> String {
        serde_json::to_string(&self.fx.service.report_store.lock().get(&self.root, false)).unwrap()
    }
}

fn planner_report_harness() -> PlannerReportHarness {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let planner_tab = mux.create_tab(&workspace).unwrap();
    let foreign_tab = mux.create_tab(&workspace).unwrap();
    let fx = ServiceFixture::new(mux);
    let root_recipient = fx.bind_recorded_agent_recipient(&root, "owner-session", "owner-nonce");
    {
        let mut mux = fx.service.mux.write();
        let owner = mux.get_tab_mut(&root).unwrap();
        owner.role = Some(AgentRole::Manager);
        owner.role_profile = Some("orchestrator".into());
        for (tab_id, session) in [(&planner_tab, "planner"), (&foreign_tab, "foreign")] {
            let tab = mux.get_tab_mut(tab_id).unwrap();
            tab.agent_session_id = Some(format!("{session}-session"));
            tab.session_binding_nonce = Some(format!("{session}-nonce"));
            tab.reported_session_binding_nonce = Some(format!("{session}-nonce"));
            tab.parent_tab_id = Some(root.clone());
        }
    }
    let store = &fx.service.plan_store;
    let mut plan = Plan::new(
        "planner authority".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&root),
    );
    plan.planner = Some(PlannerRef::Tab {
        tab_id: planner_tab.clone(),
        session_id: Some("planner-session".into()),
        title: None,
    });
    store.save(&mut plan, 1).unwrap();
    let mut foreign_plan = Plan::new(
        "foreign plan".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&root),
    );
    store.save(&mut foreign_plan, 2).unwrap();
    PlannerReportHarness {
        fx,
        _root_recipient: root_recipient,
        root,
        planner_tab,
        foreign_tab,
        plan_id: plan.id,
        foreign_plan_id: foreign_plan.id,
    }
}

#[test]
fn planner_report_with_live_matching_session_reaches_the_plan_owner() {
    let h = planner_report_harness();

    let result =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "PLANNER_STATUS_PAYLOAD",
                ReportDetails::default(),
                Some(&h.plan_id),
                None,
                None,
            )
            .expect("a live, session-verified Planner must be authorized");

    assert_eq!(result.orchestrator_tab_id, h.root);
    assert_eq!(
        result.report_outcome,
        Some(ReportOutcomeClass::StructurallyNotAttachable)
    );
    assert!(h.reports().contains("PLANNER_STATUS_PAYLOAD"));
}

#[test]
fn planner_report_refuses_a_stale_session_binding() {
    let h = planner_report_harness();
    h.stale_planner_session();

    let error =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "STALE_SESSION_PAYLOAD",
                ReportDetails::default(),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLANNER_AUTHORITY_REFUSAL), "{error}");
    assert!(!h.reports().contains("STALE_SESSION_PAYLOAD"));
}

#[test]
fn planner_report_refuses_a_foreign_tab_that_is_not_the_registered_planner() {
    let h = planner_report_harness();

    let error =
        h.fx.service
            .submit_report(
                &h.foreign_tab,
                "FOREIGN_TAB_PAYLOAD",
                h.details(&h.foreign_tab, "foreign"),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL), "{error}");
    assert!(!h.reports().contains("FOREIGN_TAB_PAYLOAD"));
}

#[test]
fn planner_report_refuses_the_registered_planner_of_a_different_plan() {
    let h = planner_report_harness();

    let error =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "FOREIGN_PLAN_PAYLOAD",
                h.details(&h.planner_tab, "planner"),
                Some(&h.foreign_plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLAN_EVIDENCE_AUTHORITY_REFUSAL), "{error}");
    assert!(!h.reports().contains("FOREIGN_PLAN_PAYLOAD"));
}

#[test]
fn planner_report_completes_spawned_planner_session_and_reaches_owner() {
    let h = planner_report_harness();
    h.set_registered_planner_session(None);
    let result =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "MISSING_SESSION_ID_PAYLOAD",
                ReportDetails::default(),
                Some(&h.plan_id),
                None,
                None,
            )
            .expect("the registered Planner's own verified tab must complete its missing session");
    assert_eq!(result.orchestrator_tab_id, h.root);
    assert!(h.reports().contains("MISSING_SESSION_ID_PAYLOAD"));
    assert!(
        matches!(h.fx.service.plan_store.get(&h.plan_id).unwrap().planner,
        Some(PlannerRef::Tab { session_id: Some(session), .. }) if session == "planner-session")
    );
}

#[test]
fn planner_report_missing_session_refuses_stale_or_foreign_claims_without_repair() {
    for cause in ["reported_nonce", "caller_nonce", "caller_session"] {
        let h = planner_report_harness();
        h.set_registered_planner_session(None);
        let before = h.fx.service.plan_store.get(&h.plan_id).unwrap();
        let mut details = h.details(&h.planner_tab, "planner");
        match cause {
            "reported_nonce" => h.stale_planner_session(),
            "caller_nonce" => details.session_binding_nonce = Some("stale-nonce".into()),
            "caller_session" => details.caller_session_id = Some("foreign-session".into()),
            _ => unreachable!(),
        }
        let error =
            h.fx.service
                .submit_report(
                    &h.planner_tab,
                    "REFUSED_MISSING_SESSION",
                    details,
                    Some(&h.plan_id),
                    None,
                    None,
                )
                .unwrap_err();
        assert!(error.contains(PLANNER_AUTHORITY_REFUSAL));
        assert_eq!(h.fx.service.plan_store.get(&h.plan_id).unwrap(), before);
        assert!(!h.reports().contains("REFUSED_MISSING_SESSION"));
    }
}

#[test]
fn planner_runtime_session_finalize_records_the_registered_session() {
    let h = planner_report_harness();
    h.set_registered_planner_session(None);
    h.fx.service
        .finalize_runtime_worker_session(&h.planner_tab, "planner-session", "planner-nonce")
        .unwrap();
    assert!(
        matches!(h.fx.service.plan_store.get(&h.plan_id).unwrap().planner,
        Some(PlannerRef::Tab { session_id: Some(session), .. }) if session == "planner-session")
    );
}

#[test]
fn planner_report_refuses_a_stale_registered_session_id() {
    let h = planner_report_harness();
    h.set_registered_planner_session(Some("planner-session-before-relaunch"));

    let error =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "STALE_SESSION_ID_PAYLOAD",
                ReportDetails::default(),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLANNER_AUTHORITY_REFUSAL), "{error}");
    assert!(!h.reports().contains("STALE_SESSION_ID_PAYLOAD"));
}

#[test]
fn planner_report_refuses_a_foreign_registered_session_id() {
    let h = planner_report_harness();
    h.set_registered_planner_session(Some("foreign-session"));

    let error =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "FOREIGN_SESSION_ID_PAYLOAD",
                ReportDetails::default(),
                Some(&h.plan_id),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains(PLANNER_AUTHORITY_REFUSAL), "{error}");
    assert!(!h.reports().contains("FOREIGN_SESSION_ID_PAYLOAD"));
}

/// A plan-only claim naming an unknown plan is refused before resolution,
/// delivery, or persistence — never a graceful notice+fallback.
#[test]
fn planner_report_refuses_an_unknown_plan_before_any_fallback() {
    let h = planner_report_harness();

    let error =
        h.fx.service
            .submit_report(
                &h.planner_tab,
                "UNKNOWN_PLAN_PAYLOAD",
                ReportDetails::default(),
                Some("pl_does_not_exist"),
                None,
                None,
            )
            .unwrap_err();

    assert!(error.contains("pl_does_not_exist"), "{error}");
    assert!(!h.reports().contains("UNKNOWN_PLAN_PAYLOAD"));
}

struct PlanEvidenceHarness {
    fx: ServiceFixture,
    _root_recipient: RecordedAgentDeliveries,
    root: String,
    reporter: String,
    ordinary: String,
    bound: String,
    plan_id: String,
    task_id: String,
}

impl PlanEvidenceHarness {
    fn details(&self, tab_id: &str, session: &str, nonce: &str) -> ReportDetails {
        ReportDetails {
            plan_id: Some(self.plan_id.clone()),
            caller_tab_id: Some(tab_id.into()),
            caller_session_id: Some(session.into()),
            session_binding_nonce: Some(nonce.into()),
            ..Default::default()
        }
    }

    fn reports(&self) -> String {
        serde_json::to_string(&self.fx.service.report_store.lock().get(&self.root, false)).unwrap()
    }

    fn plan(&self) -> Plan {
        self.fx.service.plan_store.get(&self.plan_id).unwrap()
    }
}

fn plan_evidence_harness() -> PlanEvidenceHarness {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let reporter = mux.create_tab(&workspace).unwrap();
    let ordinary = mux.create_tab(&workspace).unwrap();
    let bound = mux.create_tab(&workspace).unwrap();
    {
        let owner = mux.get_tab_mut(&root).unwrap();
        owner.role = Some(AgentRole::Manager);
        owner.role_profile = Some("orchestrator".into());
        owner.team_id = Some("team-evidence".into());
        owner.agent_session_id = Some("owner-session".into());
        owner.session_binding_nonce = Some("owner-nonce".into());
        owner.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    for (tab_id, session, nonce) in [
        (&reporter, "reporter-session", "reporter-nonce"),
        (&ordinary, "ordinary-session", "ordinary-nonce"),
        (&bound, "bound-session", "bound-nonce"),
    ] {
        let tab = mux.get_tab_mut(tab_id).unwrap();
        tab.agent_session_id = Some(session.into());
        tab.session_binding_nonce = Some(nonce.into());
        tab.reported_session_binding_nonce = Some(nonce.into());
        tab.parent_tab_id = Some(root.clone());
    }
    let fx = ServiceFixture::new(mux);
    let root_recipient = fx.bind_recorded_agent_recipient(&root, "owner-session", "owner-nonce");
    let mut plan = Plan::new(
        "plan evidence".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(&root),
    );
    plan.team_id = Some("team-evidence".into());
    fx.service.plan_store.save(&mut plan, 1).unwrap();
    let task_id = add_task(&fx.service.plan_store, &plan.id, "unchanged task", 2);
    fx.service
        .plan_store
        .grant_plan_evidence_reporter(
            &plan.id,
            &reporter,
            "reporter-session",
            "reporter-nonce",
            PlanEvidenceClass::CanonicalQa,
            AgentRef {
                tab_id: Some(root.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
            3,
        )
        .unwrap();
    fx.service
        .plan_store
        .grant_plan_evidence_reporter(
            &plan.id,
            &bound,
            "bound-session",
            "bound-nonce",
            PlanEvidenceClass::Review,
            AgentRef {
                tab_id: Some(root.clone()),
                session_id: Some("owner-session".into()),
                title: None,
            },
            4,
        )
        .unwrap();
    fx.service
        .plan_store
        .assign_task(&plan.id, &task_id, &bound, None, 5)
        .unwrap();
    PlanEvidenceHarness {
        fx,
        _root_recipient: root_recipient,
        root,
        reporter,
        ordinary,
        bound,
        plan_id: plan.id,
        task_id,
    }
}
