//! Report plan/task hint resolution.
//!
//! A caller-supplied task id is a HINT, never authority. The reporting shell can
//! hold an id the daemon has since moved (promotion, rebind, reassignment), so a
//! named task is honoured only when the plan record still says the caller tab
//! holds it. The hint may disambiguate among the tasks a tab holds; it may never
//! name a task the tab does not hold.

use crate::agent::reports::ReportDetails;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::Tab;
use crate::plan::{PlanEvidenceClass, PlannerRef, Task};
use crate::service::plan::PlanStore;

/// Marks the refusal class so a wrong-task report stays distinguishable from the
/// neighbouring nonce and epoch refusals in logs and in the delivered report.
pub(crate) const STALE_TASK_HINT_REFUSAL: &str = "STALE TASK BINDING";

/// The same typed-refusal family, one step further down the path: the task write
/// was refused, and live resolution then named no recipient either. A stale plan
/// owner is not a fallback and a vanished report is not a delivery, so the only
/// honest outcome left is to say the words landed nowhere.
pub(crate) const NOWHERE_TO_DELIVER_REFUSAL: &str = "NOWHERE TO DELIVER";

/// A plan-only report claims Planner authority; this marks that the claim
/// itself was refused, distinct from the task-hint refusal family above.
pub(crate) const PLANNER_AUTHORITY_REFUSAL: &str = "PLANNER AUTHORITY REFUSED";
pub(crate) const PLAN_EVIDENCE_AUTHORITY_REFUSAL: &str = "PLAN EVIDENCE AUTHORITY REFUSED";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PlanOnlyReportAuthority {
    Planner,
    PlanOwner,
    EvidenceReporter(PlanEvidenceClass),
}

/// Names what did NOT happen, so a caller cannot read the failure as a delivery
/// that merely missed its task. When a task write was refused too, that cause is
/// carried through — otherwise the sender learns only half of why it landed nowhere.
pub(crate) fn nowhere_to_deliver_refusal(tab_id: &str, refused_task_write: Option<&str>) -> String {
    let mut message = format!(
        "{NOWHERE_TO_DELIVER_REFUSAL}: tab '{tab_id}' resolves to no live recipient - no parent, no report target, no spawning tab. NOTHING was delivered and NOTHING was recorded; these words landed nowhere and must not be treated as sent. Ask for the target tab id, then use `codeterm send [--mesh <peer>] --tab <tab-or-alias> \"...\"`, or re-dispatch with a report target."
    );
    if let Some(cause) = refused_task_write {
        message.push_str(&format!(
            "\nThe task write was refused as well, so no plan record carries them either: {cause}"
        ));
    }
    message
}

/// The plan record still names this tab as the task's assignee, or the tab runs
/// an open competition attempt on it — competition is the one shape where
/// several tabs legitimately hold one task at once. A lingering non-competitive
/// attempt does not count: it survives a rebind and would launder a stale hint.
///
/// The batch-write boundary asks the same question, so both ask it here.
pub(crate) fn tab_holds(task: &Task, tab_id: &str) -> bool {
    task.assigned_tab_id.as_deref() == Some(tab_id)
        || task.execution_attempts.iter().any(|attempt| {
            attempt.executor_tab_id == tab_id
                && attempt.closed_at.is_none()
                && attempt.retired_at.is_none()
                && attempt.competition_group_id.is_some()
        })
}

fn tab_holds_task(plan_store: &PlanStore, tab_id: &str, plan_id: &str, task_id: &str) -> bool {
    let Some(plan) = plan_store.get(plan_id) else {
        return false;
    };
    crate::plan::task_key::find_task(&plan.tasks, task_id)
        .is_some_and(|task| tab_holds(task, tab_id))
}

/// Names both sides of the mismatch — the task the caller pointed at, and what
/// it actually holds — so the refusal is diagnosable without daemon logs.
fn stale_task_hint_refusal(
    plan_store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
) -> String {
    let bindings = plan_store.task_bindings_with_name_for_tab(tab_id);
    let held = if bindings.is_empty() {
        "holds no live task binding".to_string()
    } else {
        format!(
            "holds {}",
            bindings
                .iter()
                .map(|(plan, task, name)| format!("{plan}/{task} «{name}»"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    format!(
        "{STALE_TASK_HINT_REFUSAL}: tab '{tab_id}' reported against task '{task_id}' in plan \
         '{plan_id}', but that tab {held}. Nothing was written to '{task_id}' — no status, no \
         milestone. The reporting shell is carrying a task id the daemon has moved; report \
         without --task so the daemon resolves the live binding, or name the task this tab \
         actually holds."
    )
}

/// Resolve the effective (plan_id, task_id) for a report, following the priority chain:
/// 1. Provided `plan_id_hint` validated against the live store, and `task_id_hint`
///    validated against the caller tab's hold on that task.
/// 2. If stale (plan not found), falls back to daemon-side tab assignment.
/// 3. If no `plan_id_hint`, tries tab assignment directly.
/// 4. Ambiguous (>1 assignment) — errors with candidate list.
/// 5. No assignment — returns (None, None) so the report proceeds without a milestone.
pub(crate) fn resolve_plan_ctx(
    plan_store: &PlanStore,
    tab_id: &str,
    plan_id_hint: Option<&str>,
    task_id_hint: Option<&str>,
    notices: &mut Vec<String>,
) -> Result<(Option<String>, Option<String>), String> {
    let valid_plan = plan_id_hint
        .filter(|s| !s.trim().is_empty())
        .and_then(|id| {
            if plan_store.get(id).is_some() {
                Some(id.to_string())
            } else {
                notices.push(format!(
                    "plan '{}' not found in active plans — ignoring, checking tab assignment",
                    id
                ));
                None
            }
        });

    if let Some(plan_id) = valid_plan {
        let Some(task_id) = task_id_hint.map(str::trim).filter(|id| !id.is_empty()) else {
            return Ok((Some(plan_id), None));
        };
        if !tab_holds_task(plan_store, tab_id, &plan_id, task_id) {
            return Err(stale_task_hint_refusal(
                plan_store, tab_id, &plan_id, task_id,
            ));
        }
        return Ok((Some(plan_id), Some(task_id.to_string())));
    }

    let bindings = plan_store.task_bindings_with_name_for_tab(tab_id);
    match bindings.len() {
        0 => Ok((None, None)),
        1 => {
            let (p, t, _name) = bindings.into_iter().next().unwrap();
            Ok((Some(p), Some(t)))
        }
        _ => {
            let list = bindings
                .iter()
                .map(|(p, t, n)| format!("  {t} «{n}» in plan {p}"))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "ambiguous: tab {} is assigned to {} tasks — pass --plan and --task to disambiguate:\n{}",
                tab_id,
                bindings.len(),
                list
            ))
        }
    }
}

/// Proves a plan-only report's Planner claim: the caller IS `plan.planner`
/// and its session binding is live, not merely that `plan_id` exists.
pub(crate) fn verify_planner_report_authority(
    plan_store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    tab_id: &str,
) -> Result<(), String> {
    let plan = plan_store
        .get(plan_id)
        .ok_or_else(|| format!("plan '{plan_id}' not found"))?;
    let registered_session_id = match &plan.planner {
        Some(PlannerRef::Tab {
            tab_id: planner_tab,
            session_id: Some(session_id),
            ..
        }) if planner_tab == tab_id => session_id.clone(),
        _ => {
            return Err(format!(
                "{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' is not the registered live Planner \
                 of plan '{plan_id}'"
            ))
        }
    };
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' not found"))?;
    if !session_binding_is_live(tab) {
        return Err(format!(
            "{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' has a stale or unverified session binding"
        ));
    }
    if tab.agent_session_id.as_deref() != Some(registered_session_id.as_str()) {
        return Err(format!(
            "{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' session does not match the plan's \
             registered Planner session"
        ));
    }
    Ok(())
}

pub(crate) fn classify_plan_only_report_authority(
    plan_store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    tab_id: &str,
    details: &ReportDetails,
) -> Result<PlanOnlyReportAuthority, String> {
    let plan = plan_store
        .get(plan_id)
        .ok_or_else(|| format!("plan '{plan_id}' not found"))?;
    if matches!(
        &plan.planner,
        Some(PlannerRef::Tab { tab_id: planner_tab, .. }) if planner_tab == tab_id
    ) {
        verify_planner_report_authority(plan_store, mux, plan_id, tab_id)?;
        return Ok(PlanOnlyReportAuthority::Planner);
    }
    if !plan_store
        .task_bindings_with_name_for_tab(tab_id)
        .is_empty()
    {
        return Err(format!(
            "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' holds a task; use ordinary `codeterm agent report --status ...` for that task"
        ));
    }
    let caller_matches = details.caller_tab_id.as_deref() == Some(tab_id);
    let supplied_nonce = details.session_binding_nonce.as_deref();
    let tab = mux
        .get_tab(tab_id)
        .ok_or_else(|| format!("{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' is not live"))?;
    let supplied_session = details
        .caller_session_id
        .as_deref()
        .or(tab.agent_session_id.as_deref());
    if !caller_matches
        || tab.agent_session_id.as_deref() != supplied_session
        || tab.session_binding_nonce.as_deref() != supplied_nonce
        || tab.reported_session_binding_nonce.as_deref() != supplied_nonce
        || supplied_session.is_none()
        || supplied_nonce.is_none()
    {
        return Err(format!(
            "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' has a stale, rebound, or unverified caller identity"
        ));
    }
    let current_owner = plan.owner.tab_id.as_deref().ok_or_else(|| {
        format!("{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: plan '{plan_id}' has no current owner")
    })?;
    let owner = mux.get_tab(current_owner).ok_or_else(|| {
        format!("{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: plan '{plan_id}' owner is not live")
    })?;
    if !owner.has_team_orchestrator_authority()
        || owner.agent_session_id.is_none()
        || !session_binding_is_live(owner)
    {
        return Err(format!(
            "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: plan '{plan_id}' owner is not a live authenticated Orchestrator"
        ));
    }
    if current_owner == tab_id && owner.agent_session_id.as_deref() == supplied_session {
        return Ok(PlanOnlyReportAuthority::PlanOwner);
    }
    let grant = plan
        .evidence_reporters
        .iter()
        .rev()
        .find(|grant| {
            grant.tab_id == tab_id
                && grant.session_id == supplied_session.unwrap_or_default()
                && grant.session_binding_digest
                    == crate::service::plan::review_gate::plan_evidence_binding_digest(
                        supplied_nonce.unwrap_or_default(),
                    )
                && grant.granted_by.tab_id.as_deref() == Some(current_owner)
                && grant.granted_by.session_id.as_deref() == owner.agent_session_id.as_deref()
        })
        .ok_or_else(|| {
            format!(
                "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' has no current plan evidence grant; the live Plan owner must run `codeterm plan review authorize-reporter {plan_id} {tab_id} --class review|canonical_qa`"
            )
        })?;
    Ok(PlanOnlyReportAuthority::EvidenceReporter(grant.class))
}

fn session_binding_is_live(tab: &Tab) -> bool {
    match (
        tab.session_binding_nonce.as_deref(),
        tab.reported_session_binding_nonce.as_deref(),
    ) {
        (Some(configured), Some(reported)) => configured == reported,
        _ => false,
    }
}

#[cfg(test)]
mod tests;
