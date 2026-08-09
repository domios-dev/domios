//! A stale task id must never transition the wrong task.
//!
//! The resolver cases below fix the predicate; the `ServiceContext` cases fix the
//! observable end state — that a refused report writes NO status and NO milestone
//! while the report itself still reaches the parent.

use super::{resolve_plan_ctx, STALE_TASK_HINT_REFUSAL};
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{AgentRef, OrchestratorRef, Plan, Task, TaskStatus};
use crate::service::plan::{NewTask, PlanStore};
use crate::service::test_fixture::ServiceFixture;
use crate::service::AuthenticatedPlanCaller;

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
        store.assign_task(&plan.id, &other, worker, None, 3).unwrap();
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
    assert!(error.contains(&fx.other), "must name the stale task: {error}");
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

/// A reassignment pushes a new attempt without closing the old one, so the tab
/// still has an open attempt on a task it no longer holds. That residue must not
/// be mistaken for a live binding — otherwise the rebind case leaks straight
/// through the predicate.
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
    assert!(
        other.execution_attempts.iter().any(|attempt| {
            attempt.executor_tab_id == "worker" && attempt.closed_at.is_none()
        }),
        "the reassigned task must still carry the tab's open attempt for this to prove anything"
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
    let (plan, task) = resolve_plan_ctx(&fx.store, "unbound-tab", None, None, &mut notices).unwrap();
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
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let caller = mux.create_tab(&workspace).unwrap();
    mux.set_tab_role(&caller, AgentRole::Worker, Some(&root))
        .expect("attach worker to orchestrator");
    mux.get_tab_mut(&caller).unwrap().agent_type = Some("claude".into());
    seed_plan(mux, root, caller, None)
}

/// Binds the caller's live session on both sides of the seam — the tab record
/// and the attempt the plan write authorizes against — so the nonce and epoch
/// gates pass and only the task-hold predicate can refuse.
fn seed_plan(
    mux: MultiplexerState,
    root: String,
    caller: String,
    owner: Option<&str>,
) -> ReportHarness {
    let fx = ServiceFixture::new(mux);
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
    store.assign_task(&plan.id, &held, &caller, None, 4).unwrap();
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
    ReportHarness {
        fx,
        root,
        caller,
        plan_id: plan.id,
        held,
        stale,
    }
}

#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn stale_task_report_transitions_nothing_and_names_the_mismatch() {
    let h = worker_harness();
    let held_before = h.shape(&h.held);
    let stale_before = h.shape(&h.stale);

    let result = h
        .fx
        .service
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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn stale_task_report_absent_hint_still_transitions_the_held_task() {
    let h = worker_harness();
    let stale_before = h.shape(&h.stale);

    let result = h
        .fx
        .service
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

/// The B2 hazard. B2 widens a recovery hatch for a live Plan owner acting on a
/// task whose owner session is gone; that hatch sits behind the dispatch gate
/// for `plan.task.*` verbs, which `agent.report` never enters. Plan ownership
/// must therefore buy nothing here.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
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
    let h = seed_plan(mux, root, owner, Some(owner_id.as_str()));

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
    let result = h
        .fx
        .service
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
