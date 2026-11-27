use std::future::Future;

use serde_json::{json, Value};

use super::input::{self, NavAction};
use super::BrowserRuntime;
use codeterm_browser::PageChannel;

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
        format!(
            "blocked: {} (needs human: dialog); agents never answer page dialogs — ask the user to use Take control, then inspect before continuing",
            self.status_line()
        )
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

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DialogState {
    active: Option<ActiveDialog>,
}

impl DialogState {
    #[cfg(test)]
    pub(super) fn is_blocking(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn active(&self) -> Option<&ActiveDialog> {
        self.active.as_ref()
    }

    /// Any open JS dialog — including `beforeunload` — blocks agent verbs immediately.
    pub(super) fn check_page_verb(&self) -> Result<(), String> {
        if let Some(dialog) = &self.active {
            return Err(dialog.blocking_error());
        }
        Ok(())
    }

    pub(super) fn on_dialog_opening(&mut self, dialog: ActiveDialog) {
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

impl BrowserRuntime {
    /// Races agent page preparation against opening JS dialogs without leaking `launch_gate` or
    /// committing `domains_ready` on cancellation.
    pub(super) async fn ensure_page_for_agent(&self) -> Result<PageChannel, String> {
        self.race_with_dialog(self.ensure_page()).await
    }

    /// Races any page verb against an opening JS dialog so blocked verbs fail fast and visibly
    /// instead of hanging until `WAIT_DEADLINE` (15s) or `COMMAND_DEADLINE` (30s).
    /// Dialog openings take precedence on simultaneous readiness and authoritative dialog state is
    /// re-checked on the result branch before returning.
    pub(super) async fn race_with_dialog<T>(
        &self,
        fut: impl Future<Output = Result<T, String>>,
    ) -> Result<T, String> {
        let mut dialogs = self.shared.subscribe_dialogs();
        self.shared.check_page_verb()?;
        if let Some(active) = dialogs.borrow_and_update().dialog.clone() {
            return Err(active.blocking_error());
        }
        tokio::pin!(fut);
        loop {
            tokio::select! {
                biased;
                changed = dialogs.changed() => {
                    if changed.is_err() {
                        let result = fut.await;
                        self.shared.check_page_verb()?;
                        return result;
                    }
                    if let Some(active) = dialogs.borrow_and_update().dialog.clone() {
                        return Err(active.blocking_error());
                    }
                }
                result = &mut fut => {
                    self.shared.check_page_verb()?;
                    if let Some(active) = dialogs.borrow_and_update().dialog.clone() {
                        return Err(active.blocking_error());
                    }
                    return result;
                }
            }
        }
    }

    pub(super) async fn navigate_for_agent(
        &self,
        page: &PageChannel,
        action: NavAction,
        target: Option<&str>,
    ) -> Result<String, String> {
        let _guard = self.action_gate.lock().await;
        self.race_with_dialog(async {
            let (method, params) = match action {
                NavAction::Reload => ("Page.reload", json!({})),
                NavAction::Goto => {
                    let href = input::goto_href(target.unwrap_or(""))?;
                    ("Page.navigate", json!({"url":href}))
                }
                NavAction::Back | NavAction::Forward => {
                    let history = self
                        .send(page, "Page.getNavigationHistory", json!({}))
                        .await?;
                    match input::history_entry(&history, action == NavAction::Back) {
                        Some(entry_id) => {
                            ("Page.navigateToHistoryEntry", json!({"entryId":entry_id}))
                        }
                        None => return Ok("no history entry; no navigation performed".into()),
                    }
                }
            };
            self.complete_action(page, true, self.send(page, method, params))
                .await
                .map(|(_, completion)| completion)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
