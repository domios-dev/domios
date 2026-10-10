mod execute;
pub(crate) mod qualification;

pub(crate) use execute::recover;

use codeterm_types::pty::PromptKind;

pub(crate) const IGNORE_CONFIDENCE_FLOOR: f64 = 0.60;
pub(crate) const OWNER_NOTIFICATION_CONFIDENCE: f64 = 0.80;
pub(crate) const AUTOMATIC_ACTION_CONFIDENCE: f64 = 0.90;
pub(crate) const MAX_AUTOMATIC_RESTARTS: u32 = 2;
pub(crate) const HELDOUT_PRECISION_REQUIRED: f64 = 0.95;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Label {
    ReadyIdle,
    WorkingNormally,
    UpdatePrompt,
    CrashedToShell,
    StillStarting,
    ErrorLoop,
    LoginRequired,
    ModelUnavailable,
    TrustOrPermission,
    Other,
}

impl Label {
    pub(crate) fn parse(label: &str) -> Self {
        match label {
            "ready_idle" => Self::ReadyIdle,
            "working_normally" => Self::WorkingNormally,
            "provider_update_prompt" => Self::UpdatePrompt,
            "crashed_to_shell" => Self::CrashedToShell,
            "still_starting" => Self::StillStarting,
            "error_loop" => Self::ErrorLoop,
            "login_or_auth_required" => Self::LoginRequired,
            "model_unavailable_or_quota" => Self::ModelUnavailable,
            "trust_or_permission_prompt" => Self::TrustOrPermission,
            _ => Self::Other,
        }
    }
}

pub(crate) struct Evidence<'a> {
    pub(crate) connected: bool,
    pub(crate) agreeing_observations: usize,
    pub(crate) minimum_confidence: f64,
    pub(crate) automatic_actions_qualified: bool,
    pub(crate) pty_live: bool,
    pub(crate) provider_absent: bool,
    pub(crate) startup_deadline_elapsed: bool,
    pub(crate) prompt_kind: Option<PromptKind>,
    pub(crate) declared_safe_choice: Option<&'a str>,
    pub(crate) model_safe_choice: Option<&'a str>,
    pub(crate) safe_choice_confidence: f64,
    pub(crate) directory_covered: bool,
    pub(crate) automatic_restarts: u32,
    pub(crate) configured_restart_limit: u32,
    pub(crate) restarted_for_label: bool,
    pub(crate) owner_required: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Action<'a> {
    None,
    Notify,
    FailureNotification,
    AnswerSafeChoice(&'a str),
    RestartOnce,
    DirectoryTrust,
    PermissionPolicy,
}

pub(crate) fn decide<'a>(label: Label, e: &Evidence<'a>) -> Action<'a> {
    if !e.connected
        || !e.minimum_confidence.is_finite()
        || !(IGNORE_CONFIDENCE_FLOOR..=1.0).contains(&e.minimum_confidence)
        || e.minimum_confidence < OWNER_NOTIFICATION_CONFIDENCE
        || e.agreeing_observations < 2
        || matches!(
            label,
            Label::ReadyIdle | Label::WorkingNormally | Label::Other
        )
        || e.owner_required
    {
        return Action::None;
    }
    if label == Label::TrustOrPermission {
        return match e.prompt_kind {
            Some(PromptKind::DirectoryTrust) if e.directory_covered => Action::DirectoryTrust,
            Some(PromptKind::Provider | PromptKind::Structured) => Action::PermissionPolicy,
            _ => Action::Notify,
        };
    }
    if label == Label::CrashedToShell {
        return Action::None;
    }
    if matches!(label, Label::LoginRequired | Label::ModelUnavailable) {
        return Action::Notify;
    }
    if e.automatic_restarts >= e.configured_restart_limit.min(MAX_AUTOMATIC_RESTARTS) {
        return Action::FailureNotification;
    }
    if !e.automatic_actions_qualified || e.minimum_confidence < AUTOMATIC_ACTION_CONFIDENCE {
        return Action::Notify;
    }
    match label {
        Label::UpdatePrompt => match (e.declared_safe_choice, e.model_safe_choice) {
            (Some(declared), Some(chosen))
                if e.prompt_kind == Some(PromptKind::ManualUpdate)
                    && declared == chosen
                    && (AUTOMATIC_ACTION_CONFIDENCE..=1.0).contains(&e.safe_choice_confidence) =>
            {
                Action::AnswerSafeChoice(declared)
            }
            _ => Action::Notify,
        },
        Label::StillStarting
            if e.startup_deadline_elapsed
                && !e.restarted_for_label
                && (!e.pty_live || e.provider_absent) =>
        {
            Action::RestartOnce
        }
        Label::ErrorLoop if !e.restarted_for_label && (!e.pty_live || e.provider_absent) => {
            Action::RestartOnce
        }
        _ => Action::Notify,
    }
}

#[cfg(test)]
mod tests;
