//! The template rail under the pair primitive. Gates run before anything is created,
//! registrations land before the managers compose, payload and the summary come after.
//! An absent template is the builtin default, so every pair caller takes this rail.

use std::collections::BTreeMap;

use codeterm_service_core::service::messenger::{MessageEnvelope, Messenger};
use codeterm_service_core::service::model_decision::TargetCatalogue;
use codeterm_service_core::service::team_roles::{stamp_team_role_labels, TeamRoleStore};
use codeterm_service_core::service::team_template_start::{
    admit_bound, record_consent, vault_unbound, AdmittedTemplate, ConsentLedger,
};
use codeterm_service_core::service::{blocking, team_template_payload};
use codeterm_service_core::service::{ServiceContext, SpawnAgentParams};
use codeterm_team_package::contracts::{
    PayloadApplyResult, TemplateItemState, TemplateStartItem, TemplateStartResult,
};
use codeterm_team_package::{payload_items, Package, RoleKind, CURATOR, ORCHESTRATOR, PLANNER};

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

/// What the Orchestrator of a Team without a Planner reads instead of a counterpart.
pub(crate) const SOLE_ORCHESTRATOR_INTRODUCTION: &str = "You are the Orchestrator of this team. It has no Planner: you draft and keep the plan yourself.";

/// A start that passed every gate: the package, its inputs and the manager seats by role name.
#[derive(Debug, Clone)]
pub(crate) struct TemplateRail {
    pub(crate) admitted: AdmittedTemplate,
    pub(crate) opt_out: Vec<String>,
    pub(crate) seats: BTreeMap<String, Seat>,
}

impl TemplateRail {
    pub(crate) fn seat(&self, name: &str) -> Result<&Seat, String> {
        self.seats
            .get(name)
            .ok_or_else(|| format!("template start decided no {name} seat"))
    }

    /// The pair seats a Planner only when the package lists a with_team planner manager.
    pub(crate) fn starts_planner(&self) -> bool {
        seats::started_managers(&self.admitted.resolved)
            .iter()
            .any(|name| name == PLANNER)
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
fn outside_catalogue(svc: &ServiceContext, name: &str, pick: &OwnerPick) -> Result<bool, String> {
    let (role, role_profile) = match name {
        ORCHESTRATOR => ("Orchestrator", None),
        other => ("Manager", Some(other.to_string())),
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

fn owner_picks(params: &OrchestrationPairParams) -> BTreeMap<String, OwnerPick> {
    let mut picks = BTreeMap::new();
    if let Some(request) = params.template.as_ref() {
        for (name, choice) in &request.manager_models {
            if let Some(pick) = OwnerPick::from_choice(choice) {
                picks.insert(name.clone(), pick);
            }
        }
    }
    let pair_fields = [
        (
            ORCHESTRATOR,
            &params.orchestrator_agent,
            &params.orchestrator_model,
            &params.orchestrator_reasoning_effort,
        ),
        (
            PLANNER,
            &params.planner_agent,
            &params.planner_model,
            &params.planner_reasoning_effort,
        ),
    ];
    for (name, provider, model, effort) in pair_fields {
        if let Some(pick) = OwnerPick::new(provider, model.as_deref(), effort.as_deref()) {
            picks.insert(name.to_string(), pick);
        }
    }
    picks
}

/// Records the owner's admitted consent on the saved template, off the async runtime.
pub(crate) async fn remember_consent(admitted: &AdmittedTemplate) {
    let admitted = admitted.clone();
    let _ = blocking::run(move || {
        record_consent(&admitted);
        Ok(())
    })
    .await;
}

/// Resolves the package, gathers inputs, enforces consent and decides manager models,
/// all before any Tab, Team or registration exists.
pub(crate) async fn prepare(
    svc: &ServiceContext,
    params: &OrchestrationPairParams,
) -> Result<TemplateRail, String> {
    let request = params.template.as_ref();
    let unbound = vault_unbound(svc, request).await;
    let owned_request = request.cloned();
    let admitted = blocking::run(move || {
        admit_bound(owned_request.as_ref(), ConsentLedger::global(), &unbound).map_err(refused)
    })
    .await?;
    remember_consent(&admitted).await;
    let mut picks = owner_picks(params);
    for (name, pick) in picks.iter_mut() {
        pick.outside_catalogue = outside_catalogue(svc, name, pick)?;
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
    svc: &ServiceContext,
    team_id: &str,
    rail: &TemplateRail,
) -> Result<Vec<TemplateStartItem>, String> {
    let mut registrations =
        registration::registrations_for(team_id, &rail.admitted, registration::now_ms());
    registration::record_later_seats(
        &mut registrations,
        &rail.seats,
        &seats::started_managers(&rail.admitted.resolved),
    );
    let items =
        blocking::run(move || registration::register_all(TeamRoleStore::global(), registrations))
            .await?;
    let (svc, team) = (svc.clone(), team_id.to_string());
    blocking::run(move || {
        stamp_team_role_labels(&svc, &team);
        Ok(())
    })
    .await?;
    Ok(items)
}

/// A rolled-back mint never came up, so its registrations go with it.
pub(crate) fn forget_minted(team_id: &str) {
    if let Err(error) = TeamRoleStore::global().forget(team_id) {
        log::warn!("[template-start] team={team_id} registrations survived rollback: {error}");
    }
}

fn manager_item(name: &str, state: TemplateItemState) -> TemplateStartItem {
    TemplateStartItem {
        kind: RoleKind::Manager,
        role: name.to_string(),
        state,
    }
}

/// with_team managers beyond the pair, custom ones included, join through the ordinary Team
/// staffing path the Cartographer takes; the Curator runs on its schedule instead.
async fn start_extra_manager(
    svc: &ServiceContext,
    team_id: &str,
    orchestrator_tab_id: &str,
    name: &str,
    seat: &Seat,
) -> Result<(), String> {
    if name == CURATOR {
        return Err(format!(
            "the {name} runs on its schedule, never with the Team"
        ));
    }
    crate::team::add_team_member(
        svc,
        team_id,
        Some(orchestrator_tab_id),
        crate::team::AddTeamMemberParams {
            provider: seat.provider.clone(),
            role: name.to_string(),
            title: None,
            task: None,
            model: seat.model.clone(),
        },
    )
    .await
    .map(|_| ())
}

/// The package and opt-outs the payload leaf receives, or nothing when the package brings none.
pub(crate) fn payload_request(rail: &TemplateRail) -> Option<(Package, Vec<String>)> {
    let package = &rail.admitted.resolved.package;
    (!payload_items(package).is_empty()).then(|| (package.clone(), rail.opt_out.clone()))
}

pub(crate) fn payload_status(result: &PayloadApplyResult) -> PostStartStatus {
    let detail = format!(
        "{} applied, {} waiting for the Team, {} opted out",
        result.applied.len(),
        result.deferred.len(),
        result.skipped.len()
    );
    if result.failed.is_empty() {
        return PostStartStatus::ok("payload", detail);
    }
    let failures: Vec<String> = result
        .failed
        .iter()
        .map(|failure| format!("{}: {}", failure.id, failure.reason))
        .collect();
    PostStartStatus::failed(
        "payload",
        format!("{detail}; failed: {}", failures.join("; ")),
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
    let applied = blocking::run(move || {
        Ok(team_template_payload::apply(
            &svc, &team_id, &package, &opt_out,
        ))
    })
    .await;
    Some(match applied {
        Ok(result) => payload_status(&result),
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

/// After the pair managers are up: extra with_team managers, payload and the one summary.
pub(crate) async fn finish(
    svc: &ServiceContext,
    rail: &TemplateRail,
    team_id: &str,
    orchestrator_tab_id: &str,
    registrations: Vec<TemplateStartItem>,
) -> TemplateStartResult {
    let mut managers = Vec::new();
    let mut statuses = Vec::new();
    let pair = std::iter::once(ORCHESTRATOR).chain(rail.starts_planner().then_some(PLANNER));
    for name in pair {
        managers.push(manager_item(name, TemplateItemState::Created));
        statuses.push(PostStartStatus::ok(name, "started"));
    }
    let started = seats::started_managers(&rail.admitted.resolved);
    for name in &started {
        // A later manager's seat is recorded on its registration and used when it joins.
        if name == ORCHESTRATOR || name == PLANNER {
            continue;
        }
        let outcome = match rail.seat(name) {
            Ok(seat) => start_extra_manager(svc, team_id, orchestrator_tab_id, name, seat).await,
            Err(reason) => Err(reason),
        };
        match outcome {
            Ok(()) => {
                managers.push(manager_item(name, TemplateItemState::Created));
                statuses.push(PostStartStatus::ok(name.as_str(), "started"));
            }
            Err(reason) => statuses.push(PostStartStatus::failed(name.as_str(), reason)),
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
