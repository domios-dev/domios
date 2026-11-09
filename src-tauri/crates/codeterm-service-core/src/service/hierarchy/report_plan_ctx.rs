//! Report plan/task hint resolution.
//!
//! A caller-supplied task id is a HINT, never authority. The reporting shell can
//! hold an id the daemon has since moved (promotion, rebind, reassignment), so a
//! named task is honoured only when the plan record still says the caller tab
//! holds it. The hint may disambiguate among the tasks a tab holds; it may never
//! name a task the tab does not hold.

use crate::agent::reports::ReportDetails;
use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{AgentRole, Tab};
use crate::plan::{PlanEvidenceClass, PlannerRef};
use crate::service::plan::{PlanError, PlanStore};

/// Marks the refusal class so a wrong-task report stays distinguishable from the
/// neighbouring nonce and epoch refusals in logs and in the delivered report.
pub const STALE_TASK_HINT_REFUSAL: &str = "STALE TASK BINDING";

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
pub enum PlanOnlyReportAuthority {
    Planner,
    PlanOwner,
    EvidenceReporter(PlanEvidenceClass),
    ParentMessage,
}

/// Names what did NOT happen, so a caller cannot read the failure as a delivery
/// that merely missed its task. When a task write was refused too, that cause is
/// carried through — otherwise the sender learns only half of why it landed nowhere.
pub(crate) fn nowhere_to_deliver_refusal(tab_id: &str, refused_task_write: Option<&str>) -> String {
    let mut message = format!(
        "{NOWHERE_TO_DELIVER_REFUSAL}: tab '{tab_id}' resolves to no live recipient - no parent, no report target, no spawning tab. NOTHING was delivered and NOTHING was recorded; these words landed nowhere and must not be treated as sent. Ask for the target tab id, then use `domios send [--mesh <peer>] --tab <tab-or-alias> \"...\"`, or re-dispatch with a report target."
    );
    if let Some(cause) = refused_task_write {
        message.push_str(&format!(
            "\nThe task write was refused as well, so no plan record carries them either: {cause}"
        ));
    }
    message
}

fn tab_holds_task(
    plan_store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
) -> Result<bool, String> {
    plan_store
        .report_tab_holds_task_projection(plan_id, task_id, tab_id)
        .map_err(|error| error.to_string())
}

/// Names both sides of the mismatch — the task the caller pointed at, and what
/// it actually holds — so the refusal is diagnosable without daemon logs.
fn stale_task_hint_refusal(
    plan_store: &PlanStore,
    tab_id: &str,
    plan_id: &str,
    task_id: &str,
) -> Result<String, String> {
    let bindings = plan_store
        .task_bindings_with_name_for_tab(tab_id)
        .map_err(|error| error.to_string())?;
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
    Ok(format!(
        "{STALE_TASK_HINT_REFUSAL}: tab '{tab_id}' reported against task '{task_id}' in plan \
         '{plan_id}', but that tab {held}. Nothing was written to '{task_id}' — no status, no \
         milestone. The reporting shell is carrying a task id the daemon has moved; report \
         without --task so the daemon resolves the live binding, or name the task this tab \
         actually holds."
    ))
}

/// Resolve the effective (plan_id, task_id) for a report, following the priority chain:
/// 1. Provided `plan_id_hint` validated against the live store, and `task_id_hint`
///    validated against the caller tab's hold on that task.
/// 2. If stale (plan not found), falls back to daemon-side tab assignment.
/// 3. If no `plan_id_hint`, tries tab assignment directly.
/// 4. Ambiguous (>1 assignment) — errors with candidate list.
/// 5. No assignment — returns (None, None) so the report proceeds without a milestone.
pub fn resolve_plan_ctx(
    plan_store: &PlanStore,
    tab_id: &str,
    plan_id_hint: Option<&str>,
    task_id_hint: Option<&str>,
    notices: &mut Vec<String>,
) -> Result<(Option<String>, Option<String>), String> {
    let valid_plan = match plan_id_hint.filter(|s| !s.trim().is_empty()) {
        Some(id) => match plan_store.plan_identity_projection(id) {
            Ok(_) => Some(id.to_string()),
            Err(PlanError::NotFound) => {
                notices.push(format!(
                    "plan '{}' not found in active plans — ignoring, checking tab assignment",
                    id
                ));
                None
            }
            Err(error) => return Err(error.to_string()),
        },
        None => None,
    };

    if let Some(plan_id) = valid_plan {
        let Some(task_id) = task_id_hint.map(str::trim).filter(|id| !id.is_empty()) else {
            return Ok((Some(plan_id), None));
        };
        if !tab_holds_task(plan_store, tab_id, &plan_id, task_id)? {
            return Err(stale_task_hint_refusal(
                plan_store, tab_id, &plan_id, task_id,
            )?);
        }
        return Ok((Some(plan_id), Some(task_id.to_string())));
    }

    let bindings = plan_store
        .task_bindings_with_name_for_tab(tab_id)
        .map_err(|error| error.to_string())?;
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
    let planner =
        plan_store
            .hierarchy_planner_projection(plan_id)
            .map_err(|error| match error {
                PlanError::NotFound => format!("plan '{plan_id}' not found"),
                error => error.to_string(),
            })?;
    let registered_session_id = match &planner {
        Some(PlannerRef::Tab {
            tab_id: planner_tab,
            session_id,
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
    let live_session = tab
        .agent_session_id
        .as_deref()
        .filter(|session| !session.trim().is_empty())
        .ok_or_else(|| {
            format!("{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' has no live session")
        })?;
    if registered_session_id
        .as_deref()
        .is_some_and(|registered| registered != live_session)
    {
        return Err(format!(
            "{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' session does not match the plan's \
             registered Planner session"
        ));
    }
    plan_store
        .complete_planner_report_session(
            plan_id,
            tab_id,
            live_session,
            super::planner_delivery::epoch_ms(),
        )
        .map_err(|error| format!("{PLANNER_AUTHORITY_REFUSAL}: {error}"))?;
    Ok(())
}

pub fn classify_plan_only_report_authority(
    plan_store: &PlanStore,
    mux: &MultiplexerState,
    plan_id: &str,
    tab_id: &str,
    details: &ReportDetails,
) -> Result<PlanOnlyReportAuthority, String> {
    let plan = plan_store
        .hierarchy_authority_projection(plan_id, tab_id)
        .map_err(|error| match error {
            PlanError::NotFound => format!("plan '{plan_id}' not found"),
            error => error.to_string(),
        })?;
    if matches!(
        &plan.planner,
        Some(PlannerRef::Tab { tab_id: planner_tab, .. }) if planner_tab == tab_id
    ) {
        let tab = mux
            .get_tab(tab_id)
            .ok_or_else(|| format!("{PLANNER_AUTHORITY_REFUSAL}: tab '{tab_id}' not found"))?;
        if details
            .caller_tab_id
            .as_deref()
            .is_some_and(|claim| claim != tab_id)
            || details
                .caller_session_id
                .as_deref()
                .is_some_and(|claim| tab.agent_session_id.as_deref() != Some(claim))
            || details
                .session_binding_nonce
                .as_deref()
                .is_some_and(|claim| tab.session_binding_nonce.as_deref() != Some(claim))
        {
            return Err(format!(
                "{PLANNER_AUTHORITY_REFUSAL}: caller identity is stale or foreign"
            ));
        }
        verify_planner_report_authority(plan_store, mux, plan_id, tab_id)?;
        return Ok(PlanOnlyReportAuthority::Planner);
    }
    if !plan_store
        .task_bindings_with_name_for_tab(tab_id)
        .map_err(|error| error.to_string())?
        .is_empty()
    {
        return Err(format!(
            "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' holds a task; use ordinary `domios agent report --status ...` for that task"
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
    if tab.role == Some(AgentRole::Worker)
        && tab.parent_tab_id.as_deref() == Some(current_owner)
        && plan.team_id.as_deref().is_some_and(|team_id| {
            owner.team_id.as_deref() == Some(team_id) && tab.team_id.as_deref() == Some(team_id)
        })
    {
        return Ok(PlanOnlyReportAuthority::ParentMessage);
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
                "{PLAN_EVIDENCE_AUTHORITY_REFUSAL}: tab '{tab_id}' has no current plan evidence grant; the live Plan owner must run `domios plan review authorize-reporter {plan_id} {tab_id} --class review|canonical_qa`"
            )
        })?;
    Ok(PlanOnlyReportAuthority::EvidenceReporter(grant.class))
}

fn session_binding_is_live(tab: &Tab) -> bool {
    match (
        tab.session_binding_nonce.as_deref(),
        tab.reported_session_binding_nonce.as_deref(),
    ) {
        (Some(configured), Some(reported)) => {
            !configured.trim().is_empty() && configured == reported
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests;
