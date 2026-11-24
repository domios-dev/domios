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
                "blocked: page has an open {detail} (needs human: dialog); agents cannot dismiss pre-existing dialogs — ask the user to use Take control, then inspect before continuing"
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

/// One observed JavaScript dialog with its monotonic generation and causal navigation owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TrackedDialog {
    pub generation: u64,
    pub dialog: ActiveDialog,
    /// Present only when this `beforeunload` opened while an authorized agent navigation was in flight.
    pub owned_by_nav: Option<u64>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DialogState {
    next_generation: u64,
    next_nav_token: u64,
    active: Option<TrackedDialog>,
    active_nav_token: Option<u64>,
}

impl DialogState {
    /// Whether an open dialog must surface as `needs human: dialog` right now.
    /// An in-flight navigation's own `beforeunload` stays tracked and immediately surfaces if the
    /// navigation ends, fails, or is cancelled before the dialog closes.
    pub(super) fn is_blocking_for_human(&self) -> bool {
        self.active.as_ref().is_some_and(|tracked| {
            !(self.active_nav_token.is_some()
                && tracked.owned_by_nav == self.active_nav_token
                && tracked.dialog.kind.is_beforeunload())
        })
    }

    pub(super) fn active_blocking(&self) -> Option<&ActiveDialog> {
        self.is_blocking_for_human()
            .then(|| self.active.as_ref().map(|tracked| &tracked.dialog))
            .flatten()
    }

    pub(super) fn current_tracked(&self) -> Option<TrackedDialog> {
        self.active.clone()
    }

    /// Any open dialog — including a pre-existing `beforeunload` — blocks page verbs immediately.
    pub(super) fn check_page_verb(&self) -> Result<(), String> {
        if let Some(tracked) = &self.active {
            return Err(tracked.dialog.blocking_error());
        }
        Ok(())
    }

    /// Begins an authorized agent navigation; refuses immediately if any dialog is already open.
    pub(super) fn begin_agent_navigation(&mut self) -> Result<u64, ActiveDialog> {
        if let Some(tracked) = &self.active {
            return Err(tracked.dialog.clone());
        }
        self.next_nav_token += 1;
        let token = self.next_nav_token;
        self.active_nav_token = Some(token);
        Ok(token)
    }

    pub(super) fn end_agent_navigation(&mut self, token: u64) -> bool {
        if self.active_nav_token == Some(token) {
            self.active_nav_token = None;
            return true;
        }
        false
    }

    pub(super) fn revoke_agent_navigation(&mut self) -> bool {
        self.active_nav_token.take().is_some()
    }

    /// Always records the dialog as active with a fresh generation until closed or navigated away.
    pub(super) fn on_dialog_opening(&mut self, dialog: ActiveDialog) -> TrackedDialog {
        self.next_generation += 1;
        let owned_by_nav = if dialog.kind.is_beforeunload() {
            self.active_nav_token
        } else {
            None
        };
        let tracked = TrackedDialog {
            generation: self.next_generation,
            dialog,
            owned_by_nav,
        };
        self.active = Some(tracked.clone());
        tracked
    }

    /// True only when `nav_token` is still authorized and `generation` is still the active `beforeunload`.
    pub(super) fn can_accept_beforeunload(&self, nav_token: u64, generation: u64) -> bool {
        self.active_nav_token == Some(nav_token)
            && self.active.as_ref().is_some_and(|tracked| {
                tracked.generation == generation
                    && tracked.owned_by_nav == Some(nav_token)
                    && tracked.dialog.kind.is_beforeunload()
            })
    }

    pub(super) fn on_dialog_closed(&mut self) {
        self.active = None;
    }

    /// Clears the active dialog only if it is still the exact generation that was accepted.
    pub(super) fn ack_beforeunload_closed(&mut self, generation: u64) {
        if self
            .active
            .as_ref()
            .is_some_and(|tracked| tracked.generation == generation)
        {
            self.active = None;
        }
    }

    /// A committed top-level navigation proves any prior `beforeunload` dialog has resolved.
    pub(super) fn on_navigated(&mut self) {
        if self
            .active
            .as_ref()
            .is_some_and(|tracked| tracked.dialog.kind.is_beforeunload())
        {
            self.active = None;
        }
    }
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
