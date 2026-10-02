use serde::Serialize;

use super::machine::Verdict;
use super::store::Sentinel;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Adapter,
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

#[derive(Clone, Debug, PartialEq)]
pub struct Classification {
    pub source: Source,
    pub result: Result<Verdict, Failure>,
}

impl Classification {
    pub fn adapter(result: Result<Verdict, Failure>) -> Self {
        Self {
            source: Source::Adapter,
            result,
        }
    }

    pub fn regex(verdict: Verdict) -> Self {
        Self {
            source: Source::Regex,
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
}

impl Evidence {
    pub fn new(scope: Scope, state_hash: Option<String>) -> Self {
        Self {
            evaluation_id: uuid::Uuid::new_v4().to_string(),
            observation_id: None,
            scope,
            state_hash,
            source: Source::NotAttempted,
        }
    }

    pub fn classified(&mut self, source: Source) {
        self.source = source;
        if matches!(source, Source::Adapter | Source::Regex) {
            self.observation_id = Some(self.evaluation_id.clone());
        }
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
    Skip,
}

impl Stage {
    pub fn name(self) -> &'static str {
        match self {
            Self::Decision => "decision",
            Self::RouteClaim => "route_claim",
            Self::RouteCompleted => "route_completed",
            Self::RouteFailed => "route_failed",
            Self::Skip => "skip",
        }
    }

    pub fn route_status(self) -> &'static str {
        match self {
            Self::Decision | Self::Skip => "not_attempted",
            Self::RouteClaim => "claimed",
            Self::RouteCompleted => "api_completed",
            Self::RouteFailed => "failed",
        }
    }
}
