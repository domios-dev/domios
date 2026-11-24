use serde_json::Value;

const MAX_DIALOG_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
    Other,
}

impl DialogKind {
    pub(super) fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "alert" => Self::Alert,
            "confirm" => Self::Confirm,
            "prompt" => Self::Prompt,
            "beforeunload" => Self::BeforeUnload,
            _ => Self::Other,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Confirm => "confirm",
            Self::Prompt => "prompt",
            Self::BeforeUnload => "beforeunload",
            Self::Other => "dialog",
        }
    }

    pub(super) fn is_beforeunload(self) -> bool {
        matches!(self, Self::BeforeUnload)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ActiveDialog {
    pub kind: DialogKind,
    pub message: String,
}

impl ActiveDialog {
    pub(super) fn from_cdp_params(params: &Value) -> Self {
        let kind = params
            .get("type")
            .and_then(Value::as_str)
            .map(DialogKind::parse)
            .unwrap_or(DialogKind::Other);
        let message = params
            .get("message")
            .and_then(Value::as_str)
            .map(bound_dialog_message)
            .unwrap_or_default();
        Self { kind, message }
    }

    pub(super) fn status_line(&self) -> String {
        if self.message.is_empty() {
            format!("dialog: {}", self.kind.as_str())
        } else {
            format!("dialog: {} — {}", self.kind.as_str(), self.message)
        }
    }

    pub(super) fn blocking_error(&self) -> String {
        let detail = if self.message.is_empty() {
            format!("{} dialog", self.kind.as_str())
        } else {
            format!("{} dialog ({:?})", self.kind.as_str(), self.message)
        };
        if self.kind.is_beforeunload() {
            format!(
                "blocked: page has an open {detail}; run `goto`, `reload`, `back`, `forward`, or `close` to leave the page, or ask the user to use Take control"
            )
        } else {
            format!(
                "blocked: page has an open {detail} (needs human: dialog); agents cannot dismiss page-raised dialogs — ask the user to use Take control, then inspect before continuing"
            )
        }
    }
}

fn bound_dialog_message(raw: &str) -> String {
    let single_line = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = single_line.chars();
    let bounded: String = chars.by_ref().take(MAX_DIALOG_MESSAGE_CHARS).collect();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogOpeningDecision {
    /// Agent-initiated navigation caused this `beforeunload`; the navigation verb resolves it.
    AcceptForAgentNavigation,
    /// Must remain open and surface as `needs human: dialog`.
    BlockForHuman,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum NavigationStartPlan {
    /// No blocking dialog; send the requested navigation directly.
    Proceed,
    /// A stale `beforeunload` dialog is already open; dismiss (`accept: false`) to cancel
    /// the prior navigation and unblock the renderer before issuing the new navigation.
    DismissStaleBeforeUnloadThenProceed,
    /// A page-raised `alert`/`confirm`/`prompt` dialog is open; refuse without dismissing it.
    Blocked(ActiveDialog),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DialogState {
    active: Option<ActiveDialog>,
    agent_nav_in_flight: bool,
}

impl DialogState {
    pub(super) fn active(&self) -> Option<&ActiveDialog> {
        self.active.as_ref()
    }

    pub(super) fn is_blocking(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn check_page_verb(&self, verb: &str) -> Result<(), String> {
        let Some(dialog) = &self.active else {
            return Ok(());
        };
        if dialog.kind.is_beforeunload() && is_agent_navigation_verb(verb) {
            return Ok(());
        }
        Err(dialog.blocking_error())
    }

    pub(super) fn begin_agent_navigation(&mut self) -> NavigationStartPlan {
        match &self.active {
            Some(dialog) if !dialog.kind.is_beforeunload() => {
                NavigationStartPlan::Blocked(dialog.clone())
            }
            Some(_) => {
                self.agent_nav_in_flight = true;
                NavigationStartPlan::DismissStaleBeforeUnloadThenProceed
            }
            None => {
                self.agent_nav_in_flight = true;
                NavigationStartPlan::Proceed
            }
        }
    }

    pub(super) fn end_agent_navigation(&mut self) {
        self.agent_nav_in_flight = false;
    }

    pub(super) fn on_dialog_opening(&mut self, dialog: ActiveDialog) -> DialogOpeningDecision {
        if self.agent_nav_in_flight && dialog.kind.is_beforeunload() {
            DialogOpeningDecision::AcceptForAgentNavigation
        } else {
            self.active = Some(dialog);
            DialogOpeningDecision::BlockForHuman
        }
    }

    pub(super) fn record_blocking(&mut self, dialog: ActiveDialog) {
        self.active = Some(dialog);
    }

    pub(super) fn on_dialog_closed(&mut self) {
        self.active = None;
    }

    /// A committed top-level navigation proves any prior `beforeunload` dialog has resolved.
    pub(super) fn on_navigated(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|dialog| dialog.kind.is_beforeunload())
        {
            self.active = None;
        }
    }
}

pub(super) fn is_agent_navigation_verb(verb: &str) -> bool {
    matches!(verb, "goto" | "reload" | "back" | "forward")
}

pub(super) fn is_no_dialog_cdp_error(error: &str) -> bool {
    error.contains("No dialog is showing")
}

pub(super) fn navigation_outcome(completion: String, beforeunload_accepted: bool) -> String {
    if beforeunload_accepted {
        format!("{completion} (beforeunload dialog accepted)")
    } else {
        completion
    }
}

#[cfg(test)]
mod tests;
