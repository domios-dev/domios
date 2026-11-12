//! The template rail under the pair primitive. Gates run before anything is created,
//! registrations land before the managers compose, payload and the summary come after.
//! An absent template is the builtin default, so every pair caller takes this rail.

use std::collections::BTreeMap;

use codeterm_service_core::service::messenger::{MessageEnvelope, Messenger};
use codeterm_service_core::service::model_decision::TargetCatalogue;
use codeterm_service_core::service::team_roles::TeamRoleStore;
use codeterm_service_core::service::team_template_start::{
    admit, resolve_placements, AdmittedTemplate, ConsentLedger,
};
use codeterm_service_core::service::{blocking, team_template_payload};
use codeterm_service_core::service::{ServiceContext, SpawnAgentParams};
use codeterm_team_package::contracts::{
    PayloadApplyResult, TemplateItemState, TemplateStartItem, TemplateStartResult,
};
use codeterm_team_package::{payload_items, ManagerSlot, Package, TeamRoleGroup};

use crate::OrchestrationPairParams;

mod registration;
mod rerun;
mod seats;
mod summary;

pub(crate) use rerun::rerun;
use seats::{plan_seats, OwnerPick, Seat, SeatPlan};
use summary::{pinned_statuses, summary_text, PostStartStatus};

/// Serializes template starts on one existing Team so concurrent re-runs settle identically.
pub(crate) struct TemplateStartLocks(pub(crate) crate::team::staffing::TeamCartographerAttachLocks);

/// A start that passed every gate: the package, its inputs and the manager seats.
#[derive(Debug, Clone)]
pub(crate) struct TemplateRail {
    pub(crate) admitted: AdmittedTemplate,
    pub(crate) opt_out: Vec<String>,
    pub(crate) seats: BTreeMap<ManagerSlot, Seat>,
}

impl TemplateRail {
    pub(crate) fn seat(&self, slot: ManagerSlot) -> Result<&Seat, String> {
        self.seats
            .get(&slot)
            .ok_or_else(|| format!("template start decided no {} seat", slot.as_str()))
    }

    /// The manager introduction plus the start's inputs, which every manager reads.
    pub(crate) fn coordination(&self, introduction: String) -> String {
        let inputs = &self.admitted.inputs;
        if inputs.is_empty() {
            return introduction;
        }
        let mut text = introduction;
        text.push_str("\n\nThis Team was started with these template inputs:");
        for (key, value) in inputs {
            text.push_str(&format!("\n- {key}: {value}"));
        }
        text
    }
}

fn refused(refusal: codeterm_team_package::contracts::TemplateStartRefusal) -> String {
    TemplateStartResult::Refused { refusal }.error_message()
}

/// A chat backend or remote target has no model catalogue; the decision service skips it.
fn outside_catalogue(
    svc: &ServiceContext,
    slot: ManagerSlot,
    pick: &OwnerPick,
) -> Result<bool, String> {
    let (role, role_profile) = match slot {
        ManagerSlot::Orchestrator => ("Orchestrator", None),
        other => ("Manager", Some(other.as_str().to_string())),
    };
    let params = SpawnAgentParams {
        agent: Some(pick.provider.clone()),
        model: pick.model.clone(),
        reasoning_effort: pick.effort.clone(),
        role: Some(role.to_string()),
        role_profile,
        ..Default::default()
    };
    svc.classify_spawn_target_for_params(&pick.provider, &params)
        .map_err(String::from)
}

fn owner_picks(params: &OrchestrationPairParams) -> BTreeMap<ManagerSlot, OwnerPick> {
    let mut picks = BTreeMap::new();
    if let Some(request) = params.template.as_ref() {
        for (slot, choice) in &request.manager_models {
            if let Some(pick) = OwnerPick::from_choice(choice) {
                picks.insert(*slot, pick);
            }
        }
    }
    let pair_fields = [
        (
            ManagerSlot::Orchestrator,
            &params.orchestrator_agent,
            &params.orchestrator_model,
            &params.orchestrator_reasoning_effort,
        ),
        (
            ManagerSlot::Planner,
            &params.planner_agent,
            &params.planner_model,
            &params.planner_reasoning_effort,
        ),
    ];
    for (slot, provider, model, effort) in pair_fields {
        if let Some(pick) = OwnerPick::new(provider, model.as_deref(), effort.as_deref()) {
            picks.insert(slot, pick);
        }
    }
    picks
}

/// Resolves the package, gathers inputs, enforces consent and decides manager models,
/// all before any Tab, Team or registration exists.
pub(crate) async fn prepare(
    svc: &ServiceContext,
    params: &OrchestrationPairParams,
) -> Result<TemplateRail, String> {
    let request = params.template.as_ref();
    let mut admitted = admit(request, ConsentLedger::global()).map_err(refused)?;
    resolve_placements(svc, &mut admitted).map_err(refused)?;
    let mut picks = owner_picks(params);
    for (slot, pick) in picks.iter_mut() {
        pick.outside_catalogue = outside_catalogue(svc, *slot, pick)?;
    }
    let target_svc = svc.clone();
    let target = blocking::run(move || Ok(TargetCatalogue::local(&target_svc))).await?;
    match plan_seats(&admitted.resolved, &picks, &target, &seats::local_endpoints) {
        SeatPlan::Seats(seats) => Ok(TemplateRail {
            opt_out: request
                .map(|request| request.opt_out.clone())
                .unwrap_or_default(),
            admitted,
            seats,
        }),
        SeatPlan::NeedsChoice(choices) => {
            Err(TemplateStartResult::NeedsChoice { choices }.error_message())
        }
    }
}

/// Registers every package role on the Team before its managers compose their prompts.
pub(crate) async fn register(
    team_id: &str,
    rail: &TemplateRail,
) -> Result<Vec<TemplateStartItem>, String> {
    let registrations =
        registration::registrations_for(team_id, &rail.admitted, registration::now_ms());
    blocking::run(move || registration::register_all(TeamRoleStore::global(), registrations)).await
}

/// A rolled-back mint never came up, so its registrations go with it.
pub(crate) fn forget_minted(team_id: &str) {
    if let Err(error) = TeamRoleStore::global().forget(team_id) {
        log::warn!("[template-start] team={team_id} registrations survived rollback: {error}");
    }
}

fn manager_item(slot: ManagerSlot, state: TemplateItemState) -> TemplateStartItem {
    TemplateStartItem {
        group: TeamRoleGroup::Manager,
        role: slot.as_str().to_string(),
        state,
    }
}

/// with_team managers beyond the pair join through the ordinary Team staffing path.
async fn start_extra_manager(
    svc: &ServiceContext,
    team_id: &str,
    orchestrator_tab_id: &str,
    slot: ManagerSlot,
    seat: &Seat,
) -> Result<(), String> {
    match slot {
        ManagerSlot::Cartographer => crate::team::add_team_member(
            svc,
            team_id,
            Some(orchestrator_tab_id),
            crate::team::AddTeamMemberParams {
                provider: seat.provider.clone(),
                role: slot.as_str().to_string(),
                title: None,
                task: None,
                model: seat.model.clone(),
            },
        )
        .await
        .map(|_| ()),
        other => Err(format!(
            "the {} cannot start with the Team on this host",
            other.as_str()
        )),
    }
}

/// The package and opt-outs the payload leaf receives, or nothing when the package brings none.
pub(crate) fn payload_request(rail: &TemplateRail) -> Option<(Package, Vec<String>)> {
    let package = &rail.admitted.resolved.package;
    (!payload_items(package).is_empty()).then(|| (package.clone(), rail.opt_out.clone()))
}

fn payload_detail(result: &PayloadApplyResult) -> String {
    format!(
        "{} applied, {} opted out",
        result.applied.len(),
        result.skipped.len()
    )
}

/// Applies the opted-in payload through the shared payload leaf; never re-implemented here.
async fn apply_payload(
    svc: &ServiceContext,
    team_id: &str,
    rail: &TemplateRail,
) -> Option<PostStartStatus> {
    let (package, opt_out) = payload_request(rail)?;
    let svc = svc.clone();
    let team_id = team_id.to_string();
    let applied =
        blocking::run(move || team_template_payload::apply(&svc, &team_id, &package, &opt_out))
            .await;
    Some(match applied {
        Ok(result) => PostStartStatus::ok("payload", payload_detail(&result)),
        Err(reason) => PostStartStatus::failed("payload", reason),
    })
}

/// One routed message to the coordinator when anything after the Orchestrator failed.
async fn deliver_summary(
    svc: &ServiceContext,
    team_id: &str,
    coordinator_tab_id: &str,
    statuses: &[PostStartStatus],
) {
    let Some(text) = summary_text(team_id, statuses) else {
        return;
    };
    let svc = svc.clone();
    let tab_id = coordinator_tab_id.to_string();
    let delivered = blocking::run(move || {
        Messenger::deliver(
            &svc,
            &tab_id,
            MessageEnvelope::codeterm_reminder("template-start"),
            &text,
        )
        .map(|_| ())
    })
    .await;
    if let Err(error) = delivered {
        log::warn!("[template-start] team={team_id} coordinator summary not delivered: {error}");
    }
}

/// After both pair managers are up: extra with_team managers, payload and the one summary.
pub(crate) async fn finish(
    svc: &ServiceContext,
    rail: &TemplateRail,
    team_id: &str,
    orchestrator_tab_id: &str,
    registrations: Vec<TemplateStartItem>,
) -> TemplateStartResult {
    let mut managers = Vec::new();
    let mut statuses = Vec::new();
    for slot in [ManagerSlot::Orchestrator, ManagerSlot::Planner] {
        managers.push(manager_item(slot, TemplateItemState::Created));
        statuses.push(PostStartStatus::ok(slot.as_str(), "started"));
    }
    for (slot, seat) in &rail.seats {
        if matches!(slot, ManagerSlot::Orchestrator | ManagerSlot::Planner) {
            continue;
        }
        match start_extra_manager(svc, team_id, orchestrator_tab_id, *slot, seat).await {
            Ok(()) => {
                managers.push(manager_item(*slot, TemplateItemState::Created));
                statuses.push(PostStartStatus::ok(slot.as_str(), "started"));
            }
            Err(reason) => statuses.push(PostStartStatus::failed(slot.as_str(), reason)),
        }
    }
    statuses.extend(pinned_statuses(&registrations));
    statuses.extend(apply_payload(svc, team_id, rail).await);
    deliver_summary(svc, team_id, orchestrator_tab_id, &statuses).await;
    TemplateStartResult::Started {
        team_id: team_id.to_string(),
        managers,
        registrations,
        warnings: rail.admitted.resolved.warnings.clone(),
    }
}

#[cfg(test)]
mod tests;
