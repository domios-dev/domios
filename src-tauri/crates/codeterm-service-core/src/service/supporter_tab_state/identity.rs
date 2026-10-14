use crate::multiplexer::types::Tab;
use codeterm_decision::DecisionError;
use serde_json::{json, Value};

pub(crate) mod correlation;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalIdentity {
    pub(crate) launch_key: String,
    pty: Option<String>,
    session: Option<String>,
    nonce: Option<String>,
    provider: Option<String>,
    lifecycle: Option<String>,
    prompt: Option<u64>,
    policy: String,
}

impl LocalIdentity {
    pub(crate) fn of_tab(tab: &Tab) -> Self {
        let launch_key = match tab.session_binding_nonce.as_deref() {
            Some(nonce) => format!("{}:pty:{:?}:nonce:{nonce}", tab.id, tab.pty_id),
            None => super::launch_episode_key(&tab.id, tab.pty_id.as_deref(), tab.agent_set_at),
        };
        Self {
            launch_key,
            pty: tab.pty_id.clone(),
            session: tab.agent_session_id.clone(),
            nonce: tab.session_binding_nonce.clone(),
            provider: tab.agent_type.clone(),
            lifecycle: tab.lifecycle_operation_id.clone(),
            prompt: prompt_hash(tab),
            policy: super::super::supporter::criteria_hash(&serde_json::to_vec(&json!({
                "permission": tab.permission_auto_confirm, "auto_approve": tab.auto_approve,
                "role": tab.role, "cwd": tab.cwd, "provisioned_cwd": tab.provisioned_cwd,
                "authenticated": tab.launch_session_authenticated(), "epoch": tab.lifecycle_epoch,
                "requested_model": tab.agent_requested_model, "session_model": tab.agent_session_model,
                "role_profile": tab.role_profile,
                "task_binding": tab.task_binding.as_ref().map(|binding| (&binding.plan_id, &binding.task_id, &binding.attempt_id)),
            })).unwrap_or_default()),
        }
    }
}

pub(crate) fn supporter_policy(config: &codeterm_types::config::SupporterConfig) -> String {
    super::super::supporter::criteria_hash(
        &serde_json::to_vec(&json!({
            "adapter": config.active_id(),
            "connection": config.active_id().map(|id| config.connection(id)),
            "deadline": config.deadline_ms, "cap": config.calls_per_hour,
        }))
        .unwrap_or_default(),
    )
}

pub(crate) fn prompt_hash(tab: &Tab) -> Option<u64> {
    let prompt = tab.pending_prompt.as_ref()?;
    Some(super::frame_hash(&serde_json::to_string(&json!({
        "kind": tab.pending_prompt_kind, "question": prompt.question,
        "options": prompt.options, "sections": prompt.sections, "multi_select": prompt.multi_select,
    })).unwrap_or_default()))
}

pub(crate) fn select<T>(
    adapter: Option<Result<(T, f64, bool), DecisionError>>,
    heuristic: impl FnOnce() -> T,
) -> (T, DecisionSource) {
    let source = match adapter {
        Some(Ok((value, confidence, other)))
            if confidence.is_finite() && (0.60..=1.0).contains(&confidence) && !other =>
        {
            return (value, DecisionSource::Adapter);
        }
        Some(Ok((_, _, true))) => DecisionSource::Other,
        Some(Ok(_)) => DecisionSource::LowConfidence,
        Some(Err(error)) => DecisionSource::error(&error),
        None => DecisionSource::NoAdapter,
    };
    (heuristic(), source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionSource {
    Adapter,
    NoAdapter,
    Timeout,
    Error(DecisionErrorCategory),
    Budget,
    Other,
    LowConfidence,
}

impl DecisionSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Adapter => "adapter",
            Self::NoAdapter => "fallback(no_adapter)",
            Self::Timeout => "fallback(timeout)",
            Self::Error(_) => "fallback(error)",
            Self::Budget => "fallback(budget)",
            Self::Other => "fallback(other)",
            Self::LowConfidence => "fallback(low_confidence)",
        }
    }

    pub(crate) fn error(error: &DecisionError) -> Self {
        match error {
            DecisionError::Deadline => Self::Timeout,
            DecisionError::Budget | DecisionError::CreditExhausted => Self::Budget,
            error => Self::Error(DecisionErrorCategory::from_error(error)),
        }
    }

    pub(crate) fn unavailable() -> Self {
        Self::Error(DecisionErrorCategory::Unavailable)
    }

    fn error_category(self) -> Option<&'static str> {
        match self {
            Self::Error(category) => Some(category.as_str()),
            _ => None,
        }
    }

    pub(crate) fn verdict(verdict: &super::ShadowVerdict) -> Self {
        select(
            Some(Ok(((), verdict.confidence, verdict.label == super::OTHER))),
            || (),
        )
        .1
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecisionErrorCategory {
    Transport,
    Http,
    Unavailable,
    Unsupported,
    Parse,
    InvalidObservation,
}

impl DecisionErrorCategory {
    fn from_error(error: &DecisionError) -> Self {
        match error {
            DecisionError::Transport(_) => Self::Transport,
            DecisionError::Http(_) => Self::Http,
            DecisionError::Unavailable(_) => Self::Unavailable,
            DecisionError::Unsupported => Self::Unsupported,
            DecisionError::Parse(reason) if reason == "invalid_observation" => {
                Self::InvalidObservation
            }
            DecisionError::Parse(_) => Self::Parse,
            DecisionError::Deadline | DecisionError::Budget | DecisionError::CreditExhausted => {
                Self::Unavailable
            }
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Http => "http",
            Self::Unavailable => "unavailable",
            Self::Unsupported => "unsupported",
            Self::Parse => "parse",
            Self::InvalidObservation => "invalid_observation",
        }
    }
}

pub(crate) fn journal<'a>(
    tab: &str,
    observation: impl Into<correlation::RecordIdentity<'a>>,
    source: DecisionSource,
    label: &str,
    confidence: Option<f64>,
    title: &str,
) -> Value {
    let observation = observation.into();
    let title = super::privacy::text(title);
    let decision_point = observation
        .decision_point()
        .unwrap_or("tab_state")
        .to_owned();
    let asked = match source {
        DecisionSource::Adapter | DecisionSource::Other | DecisionSource::LowConfidence => {
            super::super::supporter::tab_questions(&title, label)
        }
        _ => super::super::supporter::ActivityContext::TabState {
            title: title.into(),
            declared_safe_choice: None,
            decision_point: decision_point.clone(),
        }
        .failure(match source {
            DecisionSource::Timeout => "deadline",
            DecisionSource::Budget => "budget",
            _ => "unavailable",
        }),
    };
    let asked = super::privacy::value(&serde_json::to_value(asked).unwrap_or_default());
    let mut event = json!({ "time": chrono::Utc::now().timestamp(), "kind": "tab_state",
        "tab": tab, "action": "classification", "decided_by": source.as_str(),
        "observation_id": observation.observation_id, "label": label, "confidence": confidence,
        "reason": "model_answer", "error_category": source.error_category(), "asked": asked });
    observation.decorate(&mut event, source, label, confidence);
    event
}

impl super::super::ServiceContext {
    pub(crate) fn observation_identity_is_current(
        &self,
        tab: &str,
        expected: &LocalIdentity,
    ) -> bool {
        self.mux
            .read()
            .get_tab(tab)
            .is_some_and(|tab| LocalIdentity::of_tab(tab) == *expected)
    }
}

#[cfg(test)]
mod tests;
