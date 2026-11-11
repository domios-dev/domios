//! Manager models through the shared decision service: an owner pick is honoured or refused,
//! never silently replaced; a missing pick takes the package's decision or asks for a choice.

use std::collections::BTreeMap;

use codeterm_service_core::service::model_decision::{
    decide_model, decide_team_models, HarnessReadiness, RoleModelRequest, TargetCatalogue,
    TeamModelInputs,
};
use codeterm_service_core::service::team_template_start::{with_team_slots, ResolvedTemplate};
use codeterm_team_package::contracts::{
    role_summaries, ManagerModelChoice, ManagerModelPicker, ModelBlockedReason, ModelDecision,
    ModelDecisionReason,
};
use codeterm_team_package::{ManagerSlot, ModelPreference, TeamRoleGroup};

/// One manager launch: provider, optional model id (provider default when absent) and effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seat {
    pub provider: String,
    pub model: Option<String>,
    pub effort: Option<String>,
}

/// What the caller chose for one slot.
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

/// Every with_team manager seat, or the pickers the owner must choose before anything starts.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum SeatPlan {
    Seats(BTreeMap<ManagerSlot, Seat>),
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
    slot: ManagerSlot,
    pick: &OwnerPick,
    target: &TargetCatalogue,
) -> Result<Seat, ModelDecision> {
    let as_given = || Seat {
        provider: pick.provider.clone(),
        model: pick.model.clone(),
        effort: pick.effort.clone(),
    };
    if pick.outside_catalogue {
        return Ok(as_given());
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
    let declared = document.managers.slot(slot);
    let preference = ModelPreference::List(vec![candidate.clone()]);
    let owner_choices = [candidate];
    let request = RoleModelRequest {
        group: TeamRoleGroup::Manager,
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

/// The pair primitive always seats an Orchestrator and a Planner; other with_team managers join them.
pub(crate) fn started_slots(resolved: &ResolvedTemplate) -> Vec<ManagerSlot> {
    let mut slots = with_team_slots(resolved);
    for slot in [ManagerSlot::Orchestrator, ManagerSlot::Planner] {
        if !slots.contains(&slot) {
            slots.push(slot);
        }
    }
    slots.sort();
    slots
}

/// Decides every started manager; one refusal anywhere returns every picker that needs a hand.
pub(crate) fn plan_seats(
    resolved: &ResolvedTemplate,
    picks: &BTreeMap<ManagerSlot, OwnerPick>,
    target: &TargetCatalogue,
) -> SeatPlan {
    let mut inputs = TeamModelInputs::default();
    for (slot, pick) in picks {
        if let Some(candidate) = pick.qualified() {
            inputs
                .owner_choices
                .insert(slot.as_str().to_string(), vec![candidate]);
        }
        if let Some(effort) = &pick.effort {
            inputs
                .efforts
                .insert(slot.as_str().to_string(), effort.clone());
        }
    }
    let package_decisions = decide_team_models(&resolved.package.document, &inputs, target);
    let labels = role_summaries(&resolved.package);
    let label = |slot: ManagerSlot| {
        labels
            .iter()
            .find(|role| role.group == TeamRoleGroup::Manager && role.role == slot.as_str())
            .map_or_else(|| slot.as_str().to_string(), |role| role.label.clone())
    };
    let mut seats = BTreeMap::new();
    let mut pickers = Vec::new();
    for slot in started_slots(resolved) {
        let outcome = match picks.get(&slot) {
            Some(pick) => decide_pick(resolved, slot, pick, target),
            None => {
                let decision = package_decisions
                    .get(slot.as_str())
                    .cloned()
                    .unwrap_or_else(needs_choice);
                seat_from_decision(&decision, None).ok_or(decision)
            }
        };
        match outcome {
            Ok(seat) => {
                seats.insert(slot, seat);
            }
            Err(decision) => pickers.push(ManagerModelPicker {
                slot,
                label: label(slot),
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
