//! The one model decision for a role on one machine. Per-id resolution is the spawn
//! catalogue seam; this module adds only the role policy over an injected catalogue.

use std::collections::BTreeMap;

use codeterm_team_package::contracts::{
    EffortDecision, ModelBlockedReason, ModelDecision, ModelDecisionReason, ModelOption,
};
use codeterm_team_package::{ModelPreference, TeamDocument, TeamRoleGroup};

use super::spawn::spawn_model_catalogue::{
    close_catalogue_ids, find_catalogue_model, normalize_requested_model, preserve_context_suffix,
};
use super::ServiceContext;

mod effort;
mod target;

pub use target::{HarnessCatalogue, HarnessReadiness, TargetCatalogue};

/// Bounds the ranked outside-list choices offered for a human pick.
pub const ALTERNATIVE_LIMIT: usize = 10;

/// Everything one role's decision reads; owner picks come from the Team registration or local preferences.
#[derive(Debug, Clone, Copy)]
pub struct RoleModelRequest<'a> {
    pub group: TeamRoleGroup,
    pub preference: Option<&'a ModelPreference>,
    pub effort: Option<&'a str>,
    /// Human picks in precedence order: the Team registration choice, then the template default.
    pub owner_choices: &'a [String],
    pub requires_harnesses: &'a [String],
    pub tools: &'a [String],
    /// Models lost mid-run; never chosen again for this role.
    pub lost: &'a [String],
}

/// Per-role inputs a Team-wide decision reads; absent roles take the package defaults.
#[derive(Debug, Clone, Default)]
pub struct TeamModelInputs {
    pub owner_choices: BTreeMap<String, Vec<String>>,
    pub efforts: BTreeMap<String, String>,
    pub lost: BTreeMap<String, Vec<String>>,
}

/// Decides one role on this machine's catalogue. Blocking: call off the async runtime.
pub fn decide(
    svc: &ServiceContext,
    request: &RoleModelRequest<'_>,
    referenced: Option<&ModelDecision>,
) -> ModelDecision {
    decide_model(request, referenced, &TargetCatalogue::local(svc))
}

/// Decides every role of a package on one target, referenced roles first.
pub fn decide_team_models(
    document: &TeamDocument,
    inputs: &TeamModelInputs,
    target: &TargetCatalogue,
) -> BTreeMap<String, ModelDecision> {
    let mut decided = BTreeMap::new();
    for (_, role, _) in document.roles() {
        decide_role(
            document,
            inputs,
            target,
            role,
            &mut Vec::new(),
            &mut decided,
        );
    }
    decided
}

fn decide_role(
    document: &TeamDocument,
    inputs: &TeamModelInputs,
    target: &TargetCatalogue,
    role: &str,
    visiting: &mut Vec<String>,
    decided: &mut BTreeMap<String, ModelDecision>,
) -> Option<ModelDecision> {
    if let Some(decision) = decided.get(role) {
        return Some(decision.clone());
    }
    let (group, _, declared) = document
        .roles()
        .into_iter()
        .find(|(_, name, _)| *name == role)?;
    if visiting.iter().any(|seen| seen == role) {
        return None;
    }
    visiting.push(role.to_string());
    let referenced = match &declared.model {
        Some(ModelPreference::SameAs { same_as }) => {
            decide_role(document, inputs, target, same_as, visiting, decided)
        }
        _ => None,
    };
    visiting.pop();
    let empty = Vec::new();
    let requires = document
        .requires
        .as_ref()
        .map_or(&empty, |requires| &requires.harnesses);
    let request = RoleModelRequest {
        group,
        preference: declared.model.as_ref(),
        effort: inputs
            .efforts
            .get(role)
            .or(declared.reasoning.as_ref())
            .map(String::as_str),
        owner_choices: inputs.owner_choices.get(role).unwrap_or(&empty),
        requires_harnesses: requires,
        tools: &declared.tools,
        lost: inputs.lost.get(role).unwrap_or(&empty),
    };
    let decision = decide_model(&request, referenced.as_ref(), target);
    decided.insert(role.to_string(), decision.clone());
    Some(decision)
}

/// The pure decision: one deterministic answer from catalogue data, never an automatic outside-list pick.
pub fn decide_model(
    request: &RoleModelRequest<'_>,
    referenced: Option<&ModelDecision>,
    target: &TargetCatalogue,
) -> ModelDecision {
    if let Some(blocked) = machine_need(request, target) {
        return blocked_decision(blocked, Vec::new());
    }
    let mut skipped_owner = false;
    for choice in request.owner_choices {
        if is_lost(request, choice) {
            skipped_owner = true;
            continue;
        }
        match eligible(choice, request.effort, target) {
            Ok(eligible) => {
                return chosen(
                    eligible,
                    ModelDecisionReason::OwnerChoice,
                    false,
                    Vec::new(),
                )
            }
            Err(_) => skipped_owner = true,
        }
    }
    let mut decision = match request.preference {
        None => match request.group {
            TeamRoleGroup::Manager => needs_choice(None, alternatives(request, &[], target)),
            TeamRoleGroup::Worker => undecided(ModelDecisionReason::NormalDefault),
        },
        Some(ModelPreference::SameAs { same_as }) => {
            same_as_decision(request, same_as, referenced, target)
        }
        Some(ModelPreference::List(models)) => list_decision(request, models, target),
    };
    decision.amber |= skipped_owner && decision.choice.is_some();
    decision
}

fn machine_need(
    request: &RoleModelRequest<'_>,
    target: &TargetCatalogue,
) -> Option<ModelBlockedReason> {
    if let Some(harness) = request
        .requires_harnesses
        .iter()
        .find(|harness| target.readiness(harness) == HarnessReadiness::Absent)
    {
        return Some(ModelBlockedReason::AbsentHarness {
            harness: harness.clone(),
        });
    }
    let present = target.tools.as_ref()?;
    request
        .tools
        .iter()
        .find(|tool| !present.contains(*tool))
        .map(|tool| ModelBlockedReason::MissingTool { tool: tool.clone() })
}

fn same_as_decision(
    request: &RoleModelRequest<'_>,
    same_as: &str,
    referenced: Option<&ModelDecision>,
    target: &TargetCatalogue,
) -> ModelDecision {
    let unavailable = || {
        blocked_decision(
            ModelBlockedReason::SameAsUnavailable {
                role: same_as.to_string(),
            },
            Vec::new(),
        )
    };
    let Some(referenced) = referenced else {
        return unavailable();
    };
    let Some(model) = referenced
        .choice
        .as_ref()
        .map(|choice| choice.model.as_str())
    else {
        return match referenced.reason {
            ModelDecisionReason::NormalDefault => undecided(ModelDecisionReason::NormalDefault),
            ModelDecisionReason::NeedsChoice => needs_choice(None, referenced.alternatives.clone()),
            _ => unavailable(),
        };
    };
    if is_lost(request, model) {
        return needs_choice(
            Some(ModelBlockedReason::PreferencesExhausted),
            alternatives(request, &[model.to_string()], target),
        );
    }
    match eligible(model, request.effort, target) {
        Ok(eligible) => chosen(eligible, ModelDecisionReason::SameAs, false, Vec::new()),
        Err(_) => unavailable(),
    }
}

fn list_decision(
    request: &RoleModelRequest<'_>,
    models: &[String],
    target: &TargetCatalogue,
) -> ModelDecision {
    let mut first_refusal = None;
    let mut any_lost = false;
    let mut picked: Option<(usize, Eligible)> = None;
    let mut later = Vec::new();
    for (index, model) in models.iter().enumerate() {
        if is_lost(request, model) {
            any_lost = true;
            continue;
        }
        match eligible(model, request.effort, target) {
            Ok(eligible) if picked.is_none() => picked = Some((index, eligible)),
            Ok(eligible) => later.push(eligible.option),
            Err(refusal) => {
                first_refusal.get_or_insert(refusal);
            }
        }
    }
    if let Some((index, eligible)) = picked {
        let fallback = index > 0 || any_lost;
        let reason = if fallback {
            ModelDecisionReason::WithinListFallback
        } else {
            ModelDecisionReason::FirstPreference
        };
        return chosen(eligible, reason, fallback, later);
    }
    let outside = alternatives(request, models, target);
    match first_refusal {
        Some(refusal) if !any_lost => blocked_decision(refusal, outside),
        _ => needs_choice(Some(ModelBlockedReason::PreferencesExhausted), outside),
    }
}

/// A candidate that passed every hard need on the target.
#[derive(Debug, Clone)]
struct Eligible {
    option: ModelOption,
    effort: Option<EffortDecision>,
}

/// Resolves one `harness:model` against the target through the spawn catalogue seam.
fn eligible(
    candidate: &str,
    effort: Option<&str>,
    target: &TargetCatalogue,
) -> Result<Eligible, ModelBlockedReason> {
    let candidate = candidate.trim();
    let Some((harness_id, _)) = candidate.split_once(':') else {
        return Err(ModelBlockedReason::UnknownModel {
            model: candidate.to_string(),
            suggestions: Vec::new(),
        });
    };
    let harness_id = harness_id.trim().to_ascii_lowercase();
    let model = normalize_requested_model(&harness_id, candidate);
    let Some(harness) = target.harnesses.get(&harness_id) else {
        return Err(ModelBlockedReason::AbsentHarness {
            harness: harness_id,
        });
    };
    match harness.readiness {
        HarnessReadiness::Ready => {}
        HarnessReadiness::Absent => {
            return Err(ModelBlockedReason::AbsentHarness {
                harness: harness_id,
            })
        }
        HarnessReadiness::NotSignedIn => {
            return Err(ModelBlockedReason::NotSignedIn {
                harness: harness_id,
            })
        }
        HarnessReadiness::Quota => {
            return Err(ModelBlockedReason::Quota {
                harness: harness_id,
            })
        }
    }
    let (resolved, levels, default_effort) = match find_catalogue_model(&model, &harness.models) {
        Some(entry) => (
            preserve_context_suffix(entry.id.clone(), &model),
            entry.reasoning_efforts.as_ref().map_or_else(
                || harness.efforts.clone(),
                |levels| levels.iter().map(|level| level.id.clone()).collect(),
            ),
            entry
                .default_reasoning_effort
                .clone()
                .or_else(|| harness.default_effort.clone()),
        ),
        None if harness.enforced => {
            return Err(ModelBlockedReason::UnknownModel {
                model: candidate.to_string(),
                suggestions: close_catalogue_ids(&model, &harness.models)
                    .into_iter()
                    .map(|id| format!("{harness_id}:{id}"))
                    .collect(),
            })
        }
        None => (
            model.clone(),
            harness.efforts.clone(),
            harness.default_effort.clone(),
        ),
    };
    let option = ModelOption {
        model: format!("{harness_id}:{resolved}"),
        price: None,
    };
    let effort = match effort.map(str::trim).filter(|effort| !effort.is_empty()) {
        None => None,
        Some(_) if levels.is_empty() => {
            return Err(ModelBlockedReason::NoReasoningEffort {
                model: option.model,
            })
        }
        Some(requested) => Some(effort::clamp(
            &option.model,
            requested,
            &levels,
            &harness.efforts,
            default_effort.as_deref(),
        )),
    };
    Ok(Eligible { option, effort })
}

/// Eligible models outside `listed`, ranked for a human pick; never chosen automatically.
fn alternatives(
    request: &RoleModelRequest<'_>,
    listed: &[String],
    target: &TargetCatalogue,
) -> Vec<ModelOption> {
    let listed_key = |model: &str| model.trim().to_ascii_lowercase();
    let listed_harnesses = listed
        .iter()
        .filter_map(|model| {
            model
                .split_once(':')
                .map(|(harness, _)| listed_key(harness))
        })
        .collect::<Vec<_>>();
    let mut harnesses = target.harnesses.iter().collect::<Vec<_>>();
    harnesses.sort_by_key(|(id, _)| {
        listed_harnesses
            .iter()
            .position(|listed| listed == *id)
            .unwrap_or(usize::MAX)
    });
    let mut out: Vec<ModelOption> = Vec::new();
    for (harness_id, harness) in harnesses {
        let mut ids = harness
            .models
            .iter()
            .filter(|model| model.available != Some(false))
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>();
        if let Some(default) = harness.default_model.as_deref() {
            ids.sort_by_key(|id| !id.eq_ignore_ascii_case(default));
        }
        for id in ids {
            let candidate = format!("{harness_id}:{id}");
            let already = |model: &str| listed_key(model) == listed_key(candidate.as_str());
            if listed.iter().any(|model| already(model.as_str()))
                || is_lost(request, &candidate)
                || out.iter().any(|option| already(option.model.as_str()))
            {
                continue;
            }
            if let Ok(eligible) = eligible(&candidate, request.effort, target) {
                out.push(eligible.option);
                if out.len() == ALTERNATIVE_LIMIT {
                    return out;
                }
            }
        }
    }
    out
}

fn is_lost(request: &RoleModelRequest<'_>, model: &str) -> bool {
    request
        .lost
        .iter()
        .any(|lost| lost.trim().eq_ignore_ascii_case(model.trim()))
}

fn undecided(reason: ModelDecisionReason) -> ModelDecision {
    ModelDecision {
        choice: None,
        reason,
        alternatives: Vec::new(),
        blocked: None,
        amber: false,
        effort: None,
    }
}

fn chosen(
    eligible: Eligible,
    reason: ModelDecisionReason,
    amber: bool,
    alternatives: Vec<ModelOption>,
) -> ModelDecision {
    ModelDecision {
        choice: Some(eligible.option),
        alternatives,
        amber,
        effort: eligible.effort,
        ..undecided(reason)
    }
}

fn needs_choice(
    blocked: Option<ModelBlockedReason>,
    alternatives: Vec<ModelOption>,
) -> ModelDecision {
    ModelDecision {
        blocked,
        alternatives,
        ..undecided(ModelDecisionReason::NeedsChoice)
    }
}

fn blocked_decision(blocked: ModelBlockedReason, alternatives: Vec<ModelOption>) -> ModelDecision {
    ModelDecision {
        blocked: Some(blocked),
        alternatives,
        ..undecided(ModelDecisionReason::Blocked)
    }
}

#[cfg(test)]
mod tests;
