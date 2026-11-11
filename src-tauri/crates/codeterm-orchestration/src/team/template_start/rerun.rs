//! Re-running a template start on an existing Team: live managers and registrations already
//! pinned to the package are reported as present, only missing registrations are added, and
//! nothing ever spawns a duplicate manager or any Worker.

use std::collections::BTreeMap;

use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::service::team_template_start::{admit, ConsentLedger};
use codeterm_service_core::service::ServiceContext;
use codeterm_team_package::contracts::{
    ManagerStartFailure, TemplateItemState, TemplateStartResult,
};
use codeterm_team_package::ManagerSlot;

use super::seats::started_slots;
use super::summary::pinned_statuses;
use super::{
    apply_payload, deliver_summary, manager_item, refused, register, TemplateRail,
    TemplateStartLocks,
};
use crate::{OrchestrationPairParams, OrchestrationPairResult};

/// The live manager Tab for each started slot in the Team.
pub(super) fn live_managers(
    mux: &MultiplexerState,
    team_id: &str,
    slots: &[ManagerSlot],
) -> BTreeMap<ManagerSlot, String> {
    mux.all_tabs()
        .filter(|tab| tab.team_id.as_deref() == Some(team_id))
        .filter_map(|tab| {
            let slot = ManagerSlot::parse(tab.role_kind_id()?)?;
            slots.contains(&slot).then(|| (slot, tab.id.clone()))
        })
        .collect()
}

/// A re-run never replaces a manager: a missing one is returned to the caller with its reason.
pub(super) fn missing_manager(
    team_id: &str,
    slots: &[ManagerSlot],
    live: &BTreeMap<ManagerSlot, String>,
) -> Option<ManagerStartFailure> {
    let slot = slots.iter().find(|slot| !live.contains_key(slot))?;
    Some(ManagerStartFailure {
        slot: *slot,
        reason: format!(
            "Team {team_id} has no live {}; a template re-run adds registrations only and never starts a replacement manager",
            slot.as_str()
        ),
        choices: Vec::new(),
    })
}

pub(crate) async fn rerun(
    svc: &ServiceContext,
    params: &OrchestrationPairParams,
    team_id: &str,
) -> Result<OrchestrationPairResult, String> {
    let gate = svc.ext::<TemplateStartLocks>().0.for_team(team_id);
    let _serialized = gate.lock_owned().await;
    let request = params.template.as_ref();
    let admitted = admit(request, ConsentLedger::global()).map_err(refused)?;
    let rail = TemplateRail {
        opt_out: request
            .map(|request| request.opt_out.clone())
            .unwrap_or_default(),
        admitted,
        seats: BTreeMap::new(),
    };
    let slots = started_slots(&rail.admitted.resolved);
    let live = {
        let mux = svc.mux.read();
        if mux.get_team(team_id).is_none() {
            return Err(format!("Team {team_id} does not exist"));
        }
        let foreign_caller = params
            .spawned_by_tab_id
            .as_deref()
            .and_then(|caller| mux.get_tab(caller))
            .and_then(|tab| tab.team_id.as_deref())
            .is_some_and(|own| own != team_id);
        if foreign_caller {
            return Err(format!(
                "a member of another Team cannot re-run a template on Team {team_id}"
            ));
        }
        live_managers(&mux, team_id, &slots)
    };
    if let Some(failure) = missing_manager(team_id, &slots, &live) {
        return Err(TemplateStartResult::ManagerFailed { failure }.error_message());
    }
    let orchestrator_tab_id = live[&ManagerSlot::Orchestrator].clone();
    let planner_tab_id = live[&ManagerSlot::Planner].clone();
    let registrations = register(team_id, &rail).await?;
    let mut statuses = pinned_statuses(&registrations);
    statuses.extend(apply_payload(svc, team_id, &rail).await);
    deliver_summary(svc, team_id, &orchestrator_tab_id, &statuses).await;
    Ok(OrchestrationPairResult {
        plan_id: None,
        orchestrator_tab_id,
        orchestrator_model: None,
        planner_tab_id,
        planner_model: None,
        warning: None,
        budget_notice: None,
        template: Some(TemplateStartResult::Started {
            team_id: team_id.to_string(),
            managers: live
                .keys()
                .map(|slot| manager_item(*slot, TemplateItemState::AlreadyPresent))
                .collect(),
            registrations,
            warnings: rail.admitted.resolved.warnings.clone(),
        }),
    })
}
