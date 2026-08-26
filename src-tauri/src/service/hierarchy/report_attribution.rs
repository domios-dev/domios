//! Where a report lands when its task hint is stale.
//!
//! A refused task write is not a licence to deliver the words to whoever owned
//! the stale plan. The recipient is resolved live from the tab's own hierarchy —
//! the same `resolve_report_target` chain an unhinted report already uses — and
//! the response says what happened to each half: the task write was refused, the
//! message went to the live recipient, and it is not attributed to the task.
//! When live resolution names nobody, that is said too.

use super::report_plan_ctx::{NOWHERE_TO_DELIVER_REFUSAL, STALE_TASK_HINT_REFUSAL};
use crate::agent::reports::{ReportDetails, ReportStatus};
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::AgentRole;
use crate::plan::{OrchestratorRef, Plan, Task, TaskStatus};
use crate::service::plan::NewTask;
use crate::service::test_fixture::ServiceFixture;

const PAYLOAD: &str = "ATTRIBUTION_TRUTH_PAYLOAD";

/// The reporter's live parent and the stale plan's owner are DIFFERENT tabs —
/// the shape the fault needs and the shape a same-tab fixture cannot expose. The
/// worker was moved under `live_orch`; `former_orch` still owns the plan record
/// the reporting shell's stale task id points into.
struct Moved {
    fx: ServiceFixture,
    former_orch: String,
    live_orch: Option<String>,
    caller: String,
    plan_id: String,
    held: String,
    stale: String,
}

fn add_task(fx: &ServiceFixture, plan_id: &str, name: &str, now_ms: u64) -> String {
    fx.service
        .plan_store
        .add_task(
            plan_id,
            NewTask {
                name: name.into(),
                kind: "implement".into(),
                acceptance_criteria: vec!["the recipient is resolved live".into()],
                test_plan: Some("focused regression".into()),
                ..Default::default()
            },
            now_ms,
        )
        .unwrap()
}

/// `attached` decides whether the caller has a live parent at all — the second
/// fixture shape, where live resolution legitimately names nobody.
fn moved_worker(attached: bool) -> Moved {
    let (mut mux, root) = MultiplexerState::new();
    let workspace = mux.find_workspace_for_tab(&root).unwrap();
    let former_orch = mux.create_tab(&workspace).unwrap();
    let live_orch = attached.then(|| mux.create_tab(&workspace).unwrap());
    let caller = mux.create_tab(&workspace).unwrap();
    if let Some(ref live_orch) = live_orch {
        mux.set_tab_role(&caller, AgentRole::Worker, Some(live_orch.as_str()))
            .expect("attach the worker to its CURRENT orchestrator");
    }
    mux.get_tab_mut(&caller).unwrap().agent_type = Some("claude".into());

    let fx = ServiceFixture::new(mux);
    {
        let mut mux = fx.service.mux.write();
        let tab = mux.get_tab_mut(&caller).unwrap();
        tab.agent_session_id = Some("caller-session".into());
        tab.session_binding_nonce = Some("caller-nonce".into());
        tab.reported_session_binding_nonce = Some("caller-nonce".into());
    }
    let mut plan = Plan::new(
        "moved worker".into(),
        fx.temp.path().display().to_string(),
        OrchestratorRef::from_tab(former_orch.as_str()),
    );
    fx.service.plan_store.save(&mut plan, 1).unwrap();
    let stale = add_task(&fx, &plan.id, "moved on", 2);
    let held = add_task(&fx, &plan.id, "actually held", 2);
    let store = &fx.service.plan_store;
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
    Moved {
        fx,
        former_orch,
        live_orch,
        caller,
        plan_id: plan.id,
        held,
        stale,
    }
}

impl Moved {
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

    fn details(&self) -> ReportDetails {
        ReportDetails {
            status: Some(ReportStatus::InReview),
            caller_tab_id: Some(self.caller.clone()),
            caller_session_id: Some("caller-session".into()),
            session_binding_nonce: Some("caller-nonce".into()),
            owner_epoch: Some(self.task(&self.held).interruption.owner_epoch),
            ..Default::default()
        }
    }

    fn inbox(&self, tab_id: &str) -> String {
        serde_json::to_string(&self.fx.service.report_store.lock().get(tab_id, false)).unwrap()
    }

    fn report(&self, task_hint: &str) -> Result<crate::service::ReportResult, String> {
        self.fx.service.submit_report(
            &self.caller,
            PAYLOAD,
            self.details(),
            Some(&self.plan_id),
            Some(task_hint),
            None,
        )
    }
}

/// DEFECT TEST 2. Pre-change the stale plan's owner wins the destination and the
/// response still reads as a success, so a former Orchestrator receives a report
/// about a task it no longer holds, from an agent it no longer manages.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn a_stale_task_hint_delivers_to_the_live_recipient_not_the_stale_plan_owner() {
    let h = moved_worker(true);
    let live_orch = h.live_orch.clone().unwrap();
    let stale_status_before = h.task(&h.stale).status;

    let result = h
        .report(&h.stale)
        .expect("a refused task write must not swallow the report");

    assert_eq!(
        result.orchestrator_tab_id, live_orch,
        "the recipient must resolve live from this tab's hierarchy, not from the stale plan's owner"
    );
    assert!(
        h.inbox(&live_orch).contains(PAYLOAD),
        "the live recipient must actually hold the words"
    );
    assert!(
        !h.inbox(&h.former_orch).contains(PAYLOAD),
        "the former orchestrator must receive nothing about a task it no longer holds"
    );

    let rejection = result
        .rejected_task_transition
        .expect("the response must state that the task write was refused");
    assert!(rejection.cause.contains(STALE_TASK_HINT_REFUSAL));
    assert!(
        result.persisted_task.is_none(),
        "the response must state the report is NOT attributed to the task"
    );
    assert!(
        result
            .notices
            .iter()
            .any(|notice| notice.contains(&live_orch)),
        "the response must name where the report actually went: {:?}",
        result.notices
    );
    assert!(h.task(&h.stale).milestones.is_empty());
    assert_eq!(h.task(&h.stale).status, stale_status_before);
}

/// Nobody to deliver to is a state, not a silent drop: nothing is stored, nothing
/// is delivered, and the sender is told both halves of why.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn no_live_recipient_surfaces_nowhere_to_deliver_rather_than_a_stale_delivery() {
    let h = moved_worker(false);

    let error = h
        .report(&h.stale)
        .expect_err("a report with no live recipient must not claim success");

    assert!(
        error.contains(NOWHERE_TO_DELIVER_REFUSAL),
        "the nowhere-to-deliver state must be typed, not generic: {error}"
    );
    assert!(
        error.contains(STALE_TASK_HINT_REFUSAL),
        "the refused task write must be carried in the same refusal: {error}"
    );
    assert!(
        !h.inbox(&h.former_orch).contains(PAYLOAD),
        "no recipient must never degrade into the stale plan's owner"
    );
    assert!(h.task(&h.stale).milestones.is_empty());
    assert!(h.task(&h.held).milestones.is_empty());
}

/// NEGATIVE CONTROL. A CURRENT hint is untouched: the plan owner still wins the
/// destination and the milestone still lands on the held task. This fails if the
/// change were broadened to skip the plan-owner override for every report.
#[test]
#[cfg_attr(
    not(windows),
    ignore = "the real Tauri AppHandle harness requires an interactive desktop runtime"
)]
fn a_current_task_hint_delivers_and_attributes_exactly_as_today() {
    let h = moved_worker(true);

    let result = h
        .report(&h.held)
        .expect("a held task hint must still report");

    assert_eq!(
        result.orchestrator_tab_id, h.former_orch,
        "an unrefused binding keeps homing the report to the plan's owner"
    );
    assert!(h.inbox(&h.former_orch).contains(PAYLOAD));
    assert!(result.rejected_task_transition.is_none());
    assert_eq!(
        result.persisted_task,
        Some((h.plan_id.clone(), h.held.clone()))
    );
    assert_eq!(h.task(&h.held).status, TaskStatus::InReview);
    assert_eq!(
        h.task(&h.held).milestones[0].from.tab_id.as_deref(),
        Some(h.caller.as_str())
    );
}
