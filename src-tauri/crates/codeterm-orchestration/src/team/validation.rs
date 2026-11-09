use std::collections::{BTreeMap, BTreeSet};

use codeterm_service_core::plan::PlanStatus;
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::team_core::TeamPlanError;
use codeterm_service_core::service::ServiceContext;

struct PlanTeamBinding {
    plan_id: String,
    team_id: Result<String, TeamPlanError>,
}

impl PlanTeamBinding {
    fn new(plan_id: String, team_id: Option<Option<String>>) -> Self {
        let team_id = team_id
            .ok_or(TeamPlanError::Plan(PlanError::NotFound))
            .and_then(|team_id| {
                team_id
                    .filter(|team_id| !team_id.trim().is_empty())
                    .ok_or_else(|| {
                        TeamPlanError::Plan(PlanError::ActivePlanRequiresTeam {
                            plan_id: plan_id.clone(),
                        })
                    })
            });
        Self { plan_id, team_id }
    }
}

pub fn validate_active_team_plan_bindings(svc: &ServiceContext) -> Result<(), TeamPlanError> {
    let bindings = svc
        .plan_store
        .active_plan_team_bindings()
        .map_err(TeamPlanError::Plan)?
        .into_iter()
        .map(|(plan_id, team_id)| PlanTeamBinding::new(plan_id, Some(team_id)))
        .collect();
    validate_bindings_with(|| bindings, |team_ids| existing_teams(svc, team_ids))
}

pub fn validate_plan_team_binding(
    svc: &ServiceContext,
    plan_id: &str,
) -> Result<(), TeamPlanError> {
    let metadata = svc
        .plan_store
        .plan_team_binding_metadata(plan_id)
        .map_err(TeamPlanError::Plan)?;
    validate_bindings_with(
        || match metadata {
            Some((PlanStatus::Archived, _, _)) | Some((_, _, true)) => vec![],
            Some((_, team_id, _)) => vec![PlanTeamBinding::new(plan_id.into(), Some(team_id))],
            None => vec![PlanTeamBinding::new(plan_id.into(), None)],
        },
        |team_ids| existing_teams(svc, team_ids),
    )
}

fn existing_teams(svc: &ServiceContext, team_ids: BTreeSet<String>) -> BTreeSet<String> {
    let mux = svc.mux.read();
    team_ids
        .into_iter()
        .filter(|team_id| mux.get_team(team_id).is_some())
        .collect()
}

fn validate_bindings_with(
    load_bindings: impl FnOnce() -> Vec<PlanTeamBinding>,
    existing_teams: impl FnOnce(BTreeSet<String>) -> BTreeSet<String>,
) -> Result<(), TeamPlanError> {
    let bindings = load_bindings();
    if bindings.is_empty() {
        return Ok(());
    }
    let team_ids = bindings
        .iter()
        .filter_map(|binding| binding.team_id.as_ref().ok().cloned())
        .collect();
    let live_teams = existing_teams(team_ids);
    let mut claimed_teams = BTreeMap::<String, String>::new();
    for binding in bindings {
        let team_id = binding.team_id?;
        if !live_teams.contains(&team_id) {
            return Err(TeamPlanError::Team(format!(
                "Plan {} references missing Team {team_id}; adopt it into a live Team before resuming",
                binding.plan_id
            )));
        }
        if let Some(existing_plan_id) = claimed_teams.insert(team_id.clone(), binding.plan_id) {
            return Err(TeamPlanError::Plan(PlanError::TeamAlreadyHasActivePlan {
                team_id,
                existing_plan_id,
                existing_title: "conflicting active plan".into(),
            }));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
