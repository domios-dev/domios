use serde::{Deserialize, Serialize};

use super::machine::Verdict;
use super::store::Sentinel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Adapter,
    #[serde(rename = "pattern")]
    Regex,
    Disconnected,
    NotAttempted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Budget,
    Credit,
    Deadline,
    Model,
    InvalidAnswer,
    Disconnected,
    Timeout,
}

impl Failure {
    pub fn reason(self) -> &'static str {
        match self {
            Self::Budget => "budget_exhausted",
            Self::Credit => "credit_exhausted",
            Self::Deadline => "model_deadline",
            Self::Model => "model_failed",
            Self::InvalidAnswer => "invalid_answer",
            Self::Disconnected => "adapter_disconnected",
            Self::Timeout => "timeout",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub decided_by: DecidedBy,
    pub adapter_id: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
}

impl Decision {
    pub fn valid(&self) -> bool {
        self.confidence
            .is_none_or(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            && self
                .adapter_id
                .as_ref()
                .is_none_or(|id| id.chars().count() <= 256)
            && self
                .model
                .as_ref()
                .is_none_or(|model| model.chars().count() <= 256)
            && (self.decided_by != DecidedBy::Pattern
                || (self.adapter_id.is_none() && self.model.is_none()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecidedBy {
    Pattern,
    Adapter,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Classification {
    pub source: Source,
    pub adapter_id: Option<String>,
    pub model: Option<String>,
    pub confidence: Option<f64>,
    pub result: Result<Verdict, Failure>,
}

impl Classification {
    pub fn adapter(result: Result<Verdict, Failure>) -> Self {
        let confidence = result.as_ref().ok().map(|verdict| verdict.confidence);
        Self {
            source: Source::Adapter,
            confidence,
            adapter_id: None,
            model: None,
            result,
        }
    }

    pub fn regex(verdict: Verdict) -> Self {
        Self {
            source: Source::Regex,
            confidence: Some(verdict.confidence),
            adapter_id: None,
            model: None,
            result: Ok(verdict),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub sentinel_id: String,
    pub creator_tab_id: String,
    pub team_id: Option<String>,
    pub workspace_id: String,
}

impl From<&Sentinel> for Scope {
    fn from(job: &Sentinel) -> Self {
        Self {
            sentinel_id: job.id.clone(),
            creator_tab_id: job.creator_tab_id.clone(),
            team_id: job.team_id.clone(),
            workspace_id: job.workspace_id.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Evidence {
    pub evaluation_id: String,
    pub observation_id: Option<String>,
    pub scope: Scope,
    pub state_hash: Option<String>,
    pub source: Source,
    pub decision: Option<Decision>,
}

impl Evidence {
    pub fn new(scope: Scope, state_hash: Option<String>) -> Self {
        Self {
            evaluation_id: uuid::Uuid::new_v4().to_string(),
            observation_id: None,
            scope,
            state_hash,
            source: Source::NotAttempted,
            decision: None,
        }
    }

    pub fn classified(&mut self, source: Source) {
        self.source = source;
        if matches!(source, Source::Adapter | Source::Regex) {
            self.observation_id = Some(self.evaluation_id.clone());
        }
    }

    pub fn record_decision(&mut self, classification: &Classification) {
        let decided_by = match classification.source {
            Source::Regex => DecidedBy::Pattern,
            Source::Adapter => DecidedBy::Adapter,
            _ => return,
        };
        self.decision = Some(Decision {
            decided_by,
            adapter_id: classification.adapter_id.as_deref().map(metadata),
            model: classification.model.as_deref().map(metadata),
            confidence: classification
                .confidence
                .filter(|confidence| confidence.is_finite() && (0.0..=1.0).contains(confidence)),
        });
    }

    pub fn matches(&self, job: &Sentinel) -> bool {
        self.scope == Scope::from(job)
            && self
                .state_hash
                .as_ref()
                .is_none_or(|hash| job.machine.last_hash.as_ref() == Some(hash))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Decision,
    RouteClaim,
    RouteCompleted,
    RouteFailed,
    EscalationExpired,
    Skip,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::RouteClaim => "route_claim",
            Self::RouteCompleted => "route_completed",
            Self::RouteFailed => "route_failed",
            Self::EscalationExpired => "escalation_expired",
            Self::Skip => "skip",
        }
    }

    pub fn route_status(self) -> &'static str {
        match self {
            Self::Decision | Self::Skip => "not_attempted",
            Self::RouteClaim => "claimed",
            Self::RouteCompleted => "api_completed",
            Self::RouteFailed => "failed",
            Self::EscalationExpired => "not_attempted",
        }
    }
}

fn metadata(value: &str) -> String {
    codeterm_util::redact::text(value)
        .chars()
        .take(256)
        .collect()
}
