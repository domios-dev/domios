use super::{AgentRestartError, RestartAttemptError, RestartOutcome};
use crate::agent::provider::ModelSwitchMode;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSwitchEvidence {
    LiveSessionRecord,
    SessionStartReport,
    FirstPostRelaunchTurn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelSwitchConfirmation {
    Observed {
        model: String,
        evidence: ModelSwitchEvidence,
    },
    Pending,
    AfterTurn,
}

#[derive(Debug, Clone)]
pub struct ModelSwitchOutcome {
    pub tab_id: String,
    pub provider: String,
    pub session_id: String,
    pub restart: Option<RestartOutcome>,
    pub mechanism: ModelSwitchMode,
    pub fallback_reason: Option<String>,
    pub previous_model: Option<String>,
    pub requested_model: String,
    pub launched_model: String,
    pub confirmation_detail: Option<String>,
    pub confirmation: ModelSwitchConfirmation,
    /// Non-model controls this switch confirmed, as before/after values.
    pub control_changes: Vec<ControlChange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControlChange {
    pub control: &'static str,
    pub previous: Option<String>,
    pub current: Option<String>,
}

#[derive(Debug)]
pub enum ModelSwitchError {
    BackgroundWork {
        items: Vec<String>,
    },
    NotFound(String),
    Unsupported {
        provider: String,
    },
    InvalidModel(crate::service::spawn::InvalidModelError),
    UnsupportedReasoningEffort {
        model: String,
        requested: String,
        supported: Vec<String>,
    },
    UnsupportedRuntimeMode {
        requested: String,
        supported: Vec<String>,
    },
    NotSwitchable(String),
    RateLimited {
        retry_after_secs: u64,
    },
    RelaunchFailed {
        outcome: RestartOutcome,
        reason: String,
        timed_out: bool,
    },
    ModelMismatch {
        outcome: RestartOutcome,
        launched_model: String,
        observed_model: String,
    },
    LiveSwitchFailed(String),
    RuntimeControlRefused(String),
    RuntimeBusy,
    LiveCommandFailed(String),
    DuringTurnIncident {
        stage: String,
        screen: String,
        reason: String,
    },
    InputNotReady(String),
}

impl std::fmt::Display for ModelSwitchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuringTurnIncident { stage, reason, .. } => write!(formatter, "mid-turn model switch incident at {stage}: {reason}"),
            Self::BackgroundWork {items}=>write!(formatter,"restart refused while background work is running: {}",items.join(", ")),
            Self::NotFound(message) | Self::NotSwitchable(message) => formatter.write_str(message),
            Self::Unsupported { provider } => write!(
                formatter,
                "provider {provider} cannot switch a live agent's model; respawn with `domios agent spawn {provider} --model MODEL`"
            ),
            Self::InvalidModel(error) => write!(formatter, "{error}"),
            Self::UnsupportedReasoningEffort { model, requested, supported } => write!(
                formatter,
                "model '{model}' does not support reasoning effort '{requested}'; supported: {}",
                if supported.is_empty() { "none".to_owned() } else { supported.join(", ") }
            ),
            Self::UnsupportedRuntimeMode { requested, supported } => write!(
                formatter,
                "runtime mode '{requested}' is unsupported; supported: {}",
                if supported.is_empty() { "none".to_owned() } else { supported.join(", ") }
            ),
            Self::RateLimited { retry_after_secs } => write!(
                formatter,
                "this Tab restarted moments ago; retry the model switch in {retry_after_secs} seconds"
            ),
            Self::RelaunchFailed { reason, .. } => {
                write!(formatter, "model switch relaunch failed: {reason}")
            }
            Self::ModelMismatch {
                launched_model,
                observed_model: observed,
                ..
            } => write!(
                formatter,
                "relaunched with {launched_model}, but the current session reported {observed}"
            ),
            Self::RuntimeBusy => formatter
                .write_str(crate::service::agent_runtime::declared_controls::RUNTIME_BUSY_MESSAGE),
            Self::RuntimeControlRefused(reason) | Self::LiveSwitchFailed(reason) | Self::LiveCommandFailed(reason) | Self::InputNotReady(reason) => {
                write!(formatter, "live model switch failed: {reason}")
            }
        }
    }
}

impl From<RestartAttemptError> for ModelSwitchError {
    fn from(error: RestartAttemptError) -> Self {
        match error {
            RestartAttemptError::Request(error) => error.into(),
            RestartAttemptError::Failed {
                outcome,
                reason,
                timed_out,
            } => Self::RelaunchFailed {
                outcome,
                reason,
                timed_out,
            },
        }
    }
}

impl From<AgentRestartError> for ModelSwitchError {
    fn from(error: AgentRestartError) -> Self {
        match error {
            AgentRestartError::BackgroundWork { items } => Self::BackgroundWork { items },
            AgentRestartError::NotFound(message) => Self::NotFound(message),
            AgentRestartError::BadRequest(message) | AgentRestartError::Internal(message) => {
                Self::NotSwitchable(message)
            }
            AgentRestartError::RateLimited { retry_after_secs } => {
                Self::RateLimited { retry_after_secs }
            }
            AgentRestartError::RuntimeProviderHandoffUnsupported => Self::NotSwitchable(
                "provider handoff into or out of an agent runtime is unsupported".to_string(),
            ),
        }
    }
}
