use std::future::Future;

use serde_json::{json, Value};

use super::input::{self, NavAction};
use super::BrowserRuntime;
use crate::surface::model::SurfaceError;
use crate::surface::runtime::{SurfaceCallCtx, SurfaceNotice};
use codeterm_browser::PageChannel;

const MAX_DIALOG_MESSAGE_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogKind {
    Alert,
    Confirm,
    Prompt,
    BeforeUnload,
    Auth,
    Other,
}

impl DialogKind {
    pub(super) fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "alert" => Self::Alert,
            "confirm" => Self::Confirm,
            "prompt" => Self::Prompt,
            "beforeunload" => Self::BeforeUnload,
            "auth" => Self::Auth,
            _ => Self::Other,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Alert => "alert",
            Self::Confirm => "confirm",
            Self::Prompt => "prompt",
            Self::BeforeUnload => "beforeunload",
            Self::Auth => "auth",
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
    pub url: String,
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
        let url = params
            .get("url")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or_default()
            .to_string();
        Self { kind, message, url }
    }

    pub(super) fn status_line(&self) -> String {
        if self.message.is_empty() {
            format!("dialog: {}", self.kind.as_str())
        } else {
            format!("dialog: {} — {}", self.kind.as_str(), self.message)
        }
    }

    pub(super) fn blocking_error(&self) -> String {
        if self.kind.is_beforeunload() {
            format!(
                "blocked: {} (needs human: dialog); agents never answer page dialogs — run `domios plugin browser close --discard-unsaved` to abandon unsaved state in your own session, or ask the user to use Take control, then inspect before continuing",
                self.status_line()
            )
        } else {
            format!(
                "blocked: {} (needs human: dialog); agents never answer page dialogs — ask the user to use Take control, then inspect before continuing",
                self.status_line()
            )
        }
    }
}

pub(super) fn page_origin(dialog_url: &str, state_url: &str) -> String {
    for candidate in [dialog_url, state_url] {
        if let Ok(parsed) = url::Url::parse(candidate.trim()) {
            let origin = parsed.origin().ascii_serialization();
            if origin != "null" && !origin.is_empty() {
                return origin;
            }
        }
    }
    "unknown origin".to_string()
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

    fn check_discard_owner(ctx: &SurfaceCallCtx) -> Result<(), SurfaceError> {
        let owned = ctx.actor.is_none()
            && ctx.owner_tab_id.is_some()
            && ctx.caller_tab_id == ctx.owner_tab_id;
        if !owned {
            return Err(SurfaceError::Blocked(
                "blocked: close --discard-unsaved is only allowed for the session owner".into(),
            ));
        }
        Ok(())
    }

    fn check_discard_take_control(&self) -> Result<(), SurfaceError> {
        if self.owner_holds_control() {
            return Err(SurfaceError::Blocked(
                "blocked: owner holds Take control; cannot discard unsaved session while the user is in control".into(),
            ));
        }
        Ok(())
    }

    fn check_discard_dialog(&self) -> Result<Option<ActiveDialog>, SurfaceError> {
        match self.shared.active_dialog() {
            None => Ok(None),
            Some(dialog) if dialog.kind.is_beforeunload() => Ok(Some(dialog)),
            Some(dialog) => Err(SurfaceError::Blocked(dialog.blocking_error())),
        }
    }

    pub(super) async fn discard_unsaved_session(
        &self,
        ctx: &SurfaceCallCtx,
    ) -> Result<Option<String>, SurfaceError> {
        Self::check_discard_owner(ctx)?;
        self.check_discard_take_control()?;
        self.check_discard_dialog()?;
        let _launch = self.launch_gate.lock().await;
        Self::check_discard_owner(ctx)?;
        self.check_discard_take_control()?;
        let abandoned = self.check_discard_dialog()?;
        let state = self.shared.state();
        let who = if state.owner.trim().is_empty() {
            "agent"
        } else {
            state.owner.trim()
        };
        let origin = match &abandoned {
            Some(dialog) => page_origin(&dialog.url, &state.url),
            None => page_origin("", &state.url),
        };
        let detail = if abandoned.is_some() {
            "abandoned beforeunload dialog"
        } else {
            "explicit discard; no active beforeunload dialog"
        };
        let line = format!("{who} discarded unsaved browser session on {origin} ({detail})");
        let outcome = self
            .host
            .close_without_dialog_answer()
            .await
            .map_err(SurfaceError::Blocked)?;
        codeterm_browser::process::termination::close_result(outcome)
            .map_err(SurfaceError::TerminationFailed)?;
        self.shared.set_activity(&line, None);
        self.shared.notice(SurfaceNotice::info(&line));
        log::debug!("[browser] {line}");
        self.shared.unbind();
        Ok(Some(line))
    }
}

#[cfg(test)]
mod tests;
