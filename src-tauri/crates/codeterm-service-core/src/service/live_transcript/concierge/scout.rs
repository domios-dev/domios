use std::sync::Arc;
use std::time::Instant;

use codeterm_decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionQuestion, DecisionRequest, TokenMode,
};
use codeterm_supporter::SupporterRuntime;
use serde_json::json;

use super::{ScoutStatus, TranscriptTrigger};
use crate::service::ServiceContext;

const MAX_SCOUT_SEGMENT_CHARS: usize = 512;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScoutInput {
    pub local_concierge: bool,
    pub trigger: TranscriptTrigger,
    pub active_adapter: Option<String>,
    pub adapter_registered: bool,
    pub expected_model: Option<String>,
    pub selected_model: Option<String>,
    pub supports_choice: bool,
    pub token_mode: TokenMode,
    pub token_usable: bool,
    pub token_acl_invalid: bool,
    pub adapter_backoff: bool,
    pub budget_reached: bool,
    pub credit_exhausted: bool,
    pub storage_ready: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoutUnavailableReason {
    NotConfigured,
    AdapterNotRegistered { adapter: String },
    ModelNotApplied { adapter: String, expected: String },
    ChoiceUnsupported { adapter: String },
    TokenPermissionsInvalid { adapter: String },
    TokenMissing { adapter: String },
    AdapterBackoff { adapter: String },
    CreditExhausted { adapter: String },
    BudgetReached,
    StorageUnavailable,
}

impl ScoutUnavailableReason {
    pub fn message(&self) -> String {
        match self {
            Self::NotConfigured => "No Supporter adapter is configured.".into(),
            Self::AdapterNotRegistered { adapter } => {
                format!("Supporter adapter '{adapter}' is not installed or registered.")
            }
            Self::ModelNotApplied { adapter, expected } => {
                format!("Supporter adapter '{adapter}' has not selected model '{expected}'.")
            }
            Self::ChoiceUnsupported { adapter } => {
                format!("Supporter adapter '{adapter}' does not support choice classification.")
            }
            Self::TokenPermissionsInvalid { adapter } => {
                format!("Supporter token permissions for '{adapter}' are invalid.")
            }
            Self::TokenMissing { adapter } => {
                format!("Supporter adapter '{adapter}' requires an API token.")
            }
            Self::AdapterBackoff { adapter } => {
                format!("Supporter adapter '{adapter}' is cooling down after a recent failure.")
            }
            Self::CreditExhausted { adapter } => {
                format!("Supporter adapter '{adapter}' credits are exhausted.")
            }
            Self::BudgetReached => "Supporter hourly call budget has been reached.".into(),
            Self::StorageUnavailable => "Supporter budget or spend storage is unavailable.".into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ScoutDecision {
    Ready,
    Disabled,
    Unavailable(ScoutUnavailableReason),
}

impl ScoutDecision {
    pub fn status(&self) -> ScoutStatus {
        match self {
            Self::Ready => ScoutStatus::Ready,
            Self::Disabled => ScoutStatus::Disabled,
            Self::Unavailable(_) => ScoutStatus::Unavailable,
        }
    }

    pub fn reason(&self) -> Option<String> {
        match self {
            Self::Unavailable(reason) => Some(reason.message()),
            Self::Ready | Self::Disabled => None,
        }
    }
}

pub fn decide_availability(input: &ScoutInput) -> ScoutDecision {
    if input.local_concierge || input.trigger != TranscriptTrigger::ScoutThenConcierge {
        return ScoutDecision::Disabled;
    }
    let Some(adapter) = input
        .active_adapter
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::NotConfigured);
    };
    if !input.adapter_registered {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::AdapterNotRegistered {
            adapter: adapter.to_owned(),
        });
    }
    if let Some(expected) = input
        .expected_model
        .as_deref()
        .filter(|expected| input.selected_model.as_deref() != Some(*expected))
    {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::ModelNotApplied {
            adapter: adapter.to_owned(),
            expected: expected.to_owned(),
        });
    }
    if !input.supports_choice {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::ChoiceUnsupported {
            adapter: adapter.to_owned(),
        });
    }
    if input.token_mode == TokenMode::Required && !input.token_usable {
        return ScoutDecision::Unavailable(if input.token_acl_invalid {
            ScoutUnavailableReason::TokenPermissionsInvalid {
                adapter: adapter.to_owned(),
            }
        } else {
            ScoutUnavailableReason::TokenMissing {
                adapter: adapter.to_owned(),
            }
        });
    }
    if input.adapter_backoff {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::AdapterBackoff {
            adapter: adapter.to_owned(),
        });
    }
    if input.credit_exhausted {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::CreditExhausted {
            adapter: adapter.to_owned(),
        });
    }
    if input.budget_reached {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::BudgetReached);
    }
    if !input.storage_ready {
        return ScoutDecision::Unavailable(ScoutUnavailableReason::StorageUnavailable);
    }
    ScoutDecision::Ready
}

pub fn describe_decision_error(adapter: &str, error: &DecisionError) -> String {
    match error {
        DecisionError::Deadline => format!("Scout ({adapter}) timed out before responding."),
        DecisionError::Budget => ScoutUnavailableReason::BudgetReached.message(),
        DecisionError::CreditExhausted | DecisionError::Http(402) => {
            ScoutUnavailableReason::CreditExhausted {
                adapter: adapter.to_owned(),
            }
            .message()
        }
        DecisionError::Http(status) => format!("Scout ({adapter}) returned HTTP {status}."),
        DecisionError::Unsupported => ScoutUnavailableReason::ChoiceUnsupported {
            adapter: adapter.to_owned(),
        }
        .message(),
        DecisionError::Parse(detail) => {
            format!("Scout ({adapter}) returned an invalid classification ({detail}).")
        }
        DecisionError::Transport(detail) => {
            format!("Scout ({adapter}) request failed ({detail}).")
        }
        DecisionError::Unavailable(detail) if detail == "adapter_backoff" => {
            ScoutUnavailableReason::AdapterBackoff {
                adapter: adapter.to_owned(),
            }
            .message()
        }
        DecisionError::Unavailable(detail) if detail == "provider_credentials" => {
            format!("Supporter adapter '{adapter}' credentials are unavailable.")
        }
        DecisionError::Unavailable(detail)
            if detail.contains("storage") || detail.contains("journal") =>
        {
            format!("Supporter storage is unavailable ({detail}).")
        }
        DecisionError::Unavailable(detail) => {
            format!("Scout ({adapter}) is unavailable ({detail}).")
        }
    }
}

pub(super) struct PreparedScout {
    pub adapter_id: String,
    pub model: Arc<dyn DecisionModel>,
    pub runtime: Arc<SupporterRuntime>,
    pub deadline_ms: u64,
}

pub(super) struct ResolvedScout {
    pub decision: ScoutDecision,
    pub prepared: Option<PreparedScout>,
}

pub(super) fn resolve(
    svc: &ServiceContext,
    local_concierge: bool,
    trigger: TranscriptTrigger,
) -> ResolvedScout {
    if local_concierge || trigger != TranscriptTrigger::ScoutThenConcierge {
        return ResolvedScout {
            decision: ScoutDecision::Disabled,
            prepared: None,
        };
    }
    let config = svc.supporter_decision_config();
    let active_id = config.active_id().map(str::to_owned);
    let runtime = svc.supporter_decision_runtime();
    let adapter = active_id
        .as_deref()
        .and_then(|id| svc.supporter_models.get(id));
    let expected_model = active_id.as_deref().and_then(|id| {
        config.connection(id).model.or_else(|| {
            adapter
                .as_ref()
                .and_then(|model| model.cached_models().first().map(|info| info.id.clone()))
        })
    });
    let selected_model = adapter.as_ref().and_then(|model| model.model_id());
    let supports_choice = adapter
        .as_ref()
        .is_some_and(|model| model.primitives().choice);
    let token_mode = adapter
        .as_ref()
        .map_or(TokenMode::None, |model| model.token_mode());
    let tokens = active_id
        .as_deref()
        .filter(|_| token_mode == TokenMode::Required)
        .map(|id| {
            codeterm_supporter::read_token_snapshot(
                &runtime,
                &crate::service::supporter::registry::token_store(),
                id,
            )
        })
        .unwrap_or_default();
    let cap = config.calls_per_hour.unwrap_or(0);
    let (adapter_backoff, budget_reached, credit_exhausted, storage_ready) =
        if let Some(id) = active_id.as_deref() {
            let activity = runtime.status(cap);
            let spend = runtime.spend_status(id);
            (
                !runtime.adapter_available(id),
                activity.budget.reached,
                spend.exhausted,
                activity.budget.storage_ready && spend.storage_ready,
            )
        } else {
            (false, false, false, true)
        };
    let input = ScoutInput {
        local_concierge,
        trigger,
        active_adapter: active_id.clone(),
        adapter_registered: adapter.is_some(),
        expected_model,
        selected_model,
        supports_choice,
        token_mode,
        token_usable: tokens.usable,
        token_acl_invalid: tokens.acl_invalid,
        adapter_backoff,
        budget_reached,
        credit_exhausted,
        storage_ready,
    };
    let decision = decide_availability(&input);
    let prepared = match (&decision, active_id, adapter) {
        (ScoutDecision::Ready, Some(adapter_id), Some(model)) => {
            let budgeted = codeterm_supporter::budgeted_model_with_activity(
                model,
                runtime.clone(),
                cap,
                codeterm_supporter::VerdictKind::TabState,
                None,
                None,
                None,
            );
            Some(PreparedScout {
                adapter_id,
                model: budgeted,
                runtime,
                deadline_ms: config.deadline_ms,
            })
        }
        _ => None,
    };
    ResolvedScout { decision, prepared }
}

pub(super) fn scout_segment(seq: u64, text: &str) -> Option<serde_json::Value> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let bounded: String = trimmed.chars().take(MAX_SCOUT_SEGMENT_CHARS).collect();
    let redacted = codeterm_supporter::privacy::value(&json!(bounded));
    Some(json!({
        "seq": seq,
        "text": redacted,
    }))
}

pub(super) fn classify(
    svc: &ServiceContext,
    owner: &str,
    speech: &[(u64, &str)],
) -> Result<Option<bool>, String> {
    let segments: Vec<_> = speech
        .iter()
        .filter_map(|(seq, text)| scout_segment(*seq, text))
        .collect();
    if segments.is_empty() {
        return Ok(None);
    }
    let resolved = resolve(svc, false, TranscriptTrigger::ScoutThenConcierge);
    let Some(prepared) = resolved.prepared else {
        let reason = resolved
            .decision
            .reason()
            .unwrap_or_else(|| ScoutUnavailableReason::NotConfigured.message());
        log::debug!("[live-transcript] owner={owner} scout unavailable: {reason}");
        return Err(reason);
    };
    let request = DecisionRequest {
        state: json!({ "segments": segments }),
        instructions: "Decide whether this recent transcript contains an actionable request, instruction, or message meant for an assistant or agent in any language (Russian, English, or any other language). Ignore pure ambient chatter or filler with no request.".into(),
        question: DecisionQuestion::Choice {
            options: [
                ("wake".into(), "Actionable request".into()),
                ("ignore".into(), "No actionable request".into()),
            ]
            .into(),
        },
    };
    let deadline = prepared.runtime.decision_deadline(
        &prepared.adapter_id,
        prepared.deadline_ms.clamp(100, 5000),
        Instant::now(),
    );
    let answer = prepared.model.decide(&request, deadline).map_err(|error| {
        log::warn!(
            "[live-transcript] owner={owner} scout decision failed adapter={}: {error}",
            prepared.adapter_id
        );
        describe_decision_error(&prepared.adapter_id, &error)
    })?;
    request.question.validate_answer(&answer).map_err(|error| {
        log::warn!(
            "[live-transcript] owner={owner} scout answer invalid adapter={}: {error}",
            prepared.adapter_id
        );
        describe_decision_error(&prepared.adapter_id, &error)
    })?;
    Ok(Some(
        matches!(answer, DecisionAnswer::Choice { choice, .. } if choice == "wake"),
    ))
}

#[cfg(test)]
mod tests;
