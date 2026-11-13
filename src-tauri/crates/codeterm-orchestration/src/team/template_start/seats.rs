//! Manager models through the shared decision service: an owner pick is honoured or refused,
//! never silently replaced; a missing pick takes the package's decision or asks for a choice.

use std::collections::BTreeMap;

use codeterm_service_core::service::model_decision::{
    decide_model, decide_team_models, HarnessReadiness, RoleModelRequest, TargetCatalogue,
    TeamModelInputs,
};
use codeterm_service_core::service::team_template_start::{with_team_managers, ResolvedTemplate};
use codeterm_team_package::contracts::{
    role_summaries, ManagerModelChoice, ManagerModelPicker, ModelBlockedReason, ModelDecision,
    ModelDecisionReason, RoleAuthority,
};
use codeterm_team_package::{ModelPreference, RoleKind};

/// One manager launch: provider, optional model id (provider default when absent) and effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seat {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// What the caller chose for one manager.
#[derive(Debug, Clone, Default)]
pub(crate) struct OwnerPick {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    /// A chat backend or remote target: the decision service has no catalogue for it.
    pub outside_catalogue: bool,
}

impl OwnerPick {
    pub(crate) fn new(provider: &str, model: Option<&str>, effort: Option<&str>) -> Option<Self> {
        let provider = provider.trim();
        if provider.is_empty() {
            return None;
        }
        let clean = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        };
        Some(Self {
            provider: provider.to_string(),
            model: clean(model),
            effort: clean(effort),
            outside_catalogue: false,
        })
    }

    /// `harness:model` for a `manager_models` entry.
    pub(crate) fn from_choice(choice: &ManagerModelChoice) -> Option<Self> {
        let (provider, model) = choice.model.split_once(':')?;
        Self::new(provider, Some(model), choice.reasoning_effort.as_deref())
    }

    /// The model id without a leading `provider:`, as spawn validation reads it.
    fn requested(&self) -> Option<&str> {
        let model = self.model.as_deref()?;
        Some(match model.split_once(':') {
            Some((prefix, rest)) if prefix.eq_ignore_ascii_case(&self.provider) => rest.trim(),
            _ => model,
        })
    }

    fn qualified(&self) -> Option<String> {
        let model = self.model.as_deref()?;
        let prefix = format!("{}:", self.provider);
        Some(if model.starts_with(&prefix) {
            model.to_string()
        } else {
            format!("{prefix}{model}")
        })
    }
}

/// A custom endpoint id (`<endpoint>/<model>`) belongs to its harness's endpoint catalogue:
/// found, refused with that catalogue's ids, or `None` when the id names no endpoint.
pub(crate) type EndpointLookup<'a> = &'a dyn Fn(&str, &str) -> Option<Result<(), Vec<String>>>;

/// The live endpoint catalogues, the same seam spawn validation reads.
pub(crate) fn local_endpoints(provider: &str, model: &str) -> Option<Result<(), Vec<String>>> {
    codeterm_service_core::agent::model_catalog::resolve_endpoint_model(provider, model)
        .map(|found| found.map(|_| ()))
}

/// Every with_team manager seat, or the pickers the owner must choose before anything starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SeatPlan {
    Seats(BTreeMap<String, Seat>),
    NeedsChoice(Vec<ManagerModelPicker>),
}

fn seat_from_decision(decision: &ModelDecision, fallback_effort: Option<&str>) -> Option<Seat> {
    let choice = decision.choice.as_ref()?;
    let (provider, model) = choice.model.split_once(':')?;
    Some(Seat {
        provider: provider.to_string(),
        model: Some(model.to_string()),
        effort: decision
            .effort
            .as_ref()
            .map(|effort| effort.applied.clone())
            .or(fallback_effort.map(str::to_string)),
    })
}

fn blocked(reason: ModelBlockedReason) -> ModelDecision {
    ModelDecision {
        choice: None,
        reason: ModelDecisionReason::Blocked,
        alternatives: Vec::new(),
        blocked: Some(reason),
        amber: false,
        effort: None,
    }
}

fn needs_choice() -> ModelDecision {
    ModelDecision {
        choice: None,
        reason: ModelDecisionReason::NeedsChoice,
        alternatives: Vec::new(),
        blocked: None,
        amber: false,
        effort: None,
    }
}

/// The owner's own pick on `target`: a model goes through the decision service, a bare
/// provider keeps its launch default once the harness is ready.
fn decide_pick(
    resolved: &ResolvedTemplate,
    name: &str,
    pick: &OwnerPick,
    target: &TargetCatalogue,
    endpoints: EndpointLookup<'_>,
) -> Result<Seat, ModelDecision> {
    let as_given = || Seat {
        provider: pick.provider.clone(),
        model: pick.model.clone(),
        effort: pick.effort.clone(),
    };
    if pick.outside_catalogue {
        return Ok(as_given());
    }
    if let Some(found) = pick
        .requested()
        .and_then(|requested| endpoints(&pick.provider, requested))
    {
        return match found {
            Ok(()) => Ok(as_given()),
            Err(valid) => Err(blocked(ModelBlockedReason::UnknownModel {
                model: pick.qualified().unwrap_or_default(),
                suggestions: valid
                    .into_iter()
                    .map(|id| format!("{}:{id}", pick.provider))
                    .collect(),
            })),
        };
    }
    let Some(candidate) = pick.qualified() else {
        return match target.readiness(&pick.provider) {
            HarnessReadiness::Ready => Ok(as_given()),
            HarnessReadiness::Absent => Err(blocked(ModelBlockedReason::AbsentHarness {
                harness: pick.provider.clone(),
            })),
            HarnessReadiness::NotSignedIn => Err(blocked(ModelBlockedReason::NotSignedIn {
                harness: pick.provider.clone(),
            })),
            HarnessReadiness::Quota => Err(blocked(ModelBlockedReason::Quota {
                harness: pick.provider.clone(),
            })),
        };
    };
    let document = &resolved.package.document;
    let declared = document.manager(name);
    let preference = ModelPreference::List(vec![candidate.clone()]);
    let owner_choices = [candidate];
    let request = RoleModelRequest {
        kind: RoleKind::Manager,
        preference: Some(&preference),
        effort: pick
            .effort
            .as_deref()
            .or(declared.and_then(|role| role.reasoning.as_deref())),
        owner_choices: &owner_choices,
        requires_harnesses: resolved.requires_harnesses(),
        tools: declared
            .map(|role| role.tools.as_slice())
            .unwrap_or_default(),
        lost: &[],
    };
    let decision = decide_model(&request, None, target);
    if decision.choice.is_none() {
        return Err(decision);
    }
    // The owner's own id launches as given (an alias keeps tracking its family).
    Ok(Seat {
        provider: pick.provider.clone(),
        model: pick.model.clone(),
        effort: decision
            .effort
            .map(|effort| effort.applied)
            .or_else(|| pick.effort.clone()),
    })
}

impl Seat {
    /// `harness:model` as a registration's model preference reads it.
    pub(crate) fn preference(&self) -> Option<ModelPreference> {
        let model = self.model.as_deref()?;
        Some(ModelPreference::List(vec![format!(
            "{}:{model}",
            self.provider
        )]))
    }
}

/// The managers this start launches: the Orchestrator always, a Planner only when the package
/// lists one with_team, and every other with_team manager, custom ones included.
pub(crate) fn started_managers(resolved: &ResolvedTemplate) -> Vec<String> {
    with_team_managers(resolved)
}

/// Decides every started manager plus each later manager the owner picked a model for;
/// one refusal anywhere returns every picker that needs a hand.
pub(crate) fn plan_seats(
    resolved: &ResolvedTemplate,
    picks: &BTreeMap<String, OwnerPick>,
    target: &TargetCatalogue,
    endpoints: EndpointLookup<'_>,
) -> SeatPlan {
    let mut inputs = TeamModelInputs::default();
    for (name, pick) in picks {
        if let Some(candidate) = pick.qualified() {
            inputs.owner_choices.insert(name.clone(), vec![candidate]);
        }
        if let Some(effort) = &pick.effort {
            inputs.efforts.insert(name.clone(), effort.clone());
        }
    }
    let document = &resolved.package.document;
    let package_decisions = decide_team_models(document, &inputs, target);
    let labels = role_summaries(&resolved.package);
    let label = |name: &str| {
        labels
            .iter()
            .find(|role| role.kind == RoleKind::Manager && role.role == name)
            .map_or_else(|| name.to_string(), |role| role.label.clone())
    };
    let mut seats = BTreeMap::new();
    let mut pickers = Vec::new();
    let mut names = started_managers(resolved);
    // A pick for a role the package does not list as a manager is not a seat.
    let later: Vec<String> = picks
        .keys()
        .filter(|name| !names.contains(name) && document.manager(name).is_some())
        .cloned()
        .collect();
    names.extend(later);
    for name in names {
        let outcome = match picks.get(&name) {
            Some(pick) => decide_pick(resolved, &name, pick, target, endpoints),
            None => {
                let decision = package_decisions
                    .get(&name)
                    .cloned()
                    .unwrap_or_else(needs_choice);
                seat_from_decision(&decision, None).ok_or(decision)
            }
        };
        match outcome {
            Ok(seat) => {
                seats.insert(name, seat);
            }
            Err(decision) => pickers.push(ManagerModelPicker {
                label: label(&name),
                authority: RoleAuthority::of(RoleKind::Manager, &name),
                role: name,
                decision,
            }),
        }
    }
    if pickers.is_empty() {
        SeatPlan::Seats(seats)
    } else {
        SeatPlan::NeedsChoice(pickers)
    }
}
