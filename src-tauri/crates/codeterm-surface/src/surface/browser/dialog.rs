use std::sync::Arc;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::input::{self, NavAction};
use super::page::PageShared;
use super::{BrowserRuntime, DIALOG_DEADLINE};
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

    pub(super) fn from_wire_opening(opening: &codeterm_browser::WireDialogOpening) -> Self {
        Self {
            kind: DialogKind::parse(&opening.dialog_type),
            message: bound_dialog_message(&opening.message),
        }
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
    active: Option<TrackedDialog>,
    authorized_nav_token: Option<u64>,
}

impl DialogState {
    /// Whether an open dialog must surface as `needs human: dialog` right now.
    /// An in-flight navigation's transport-owned `beforeunload` stays tracked and immediately
    /// surfaces if the navigation ends, fails, is revoked, or is cancelled before close.
    pub(super) fn is_blocking_for_human(&self) -> bool {
        self.active.as_ref().is_some_and(|tracked| {
            !(self.authorized_nav_token.is_some()
                && tracked.owned_by_nav == self.authorized_nav_token
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

    /// Registers an authorized in-flight agent navigation token; refuses if any dialog is open.
    pub(super) fn register_agent_navigation(&mut self, token: u64) -> Result<(), ActiveDialog> {
        if let Some(tracked) = &self.active {
            return Err(tracked.dialog.clone());
        }
        self.authorized_nav_token = Some(token);
        Ok(())
    }

    pub(super) fn is_agent_navigation_authorized(&self, token: u64) -> bool {
        self.authorized_nav_token == Some(token)
    }

    pub(super) fn end_agent_navigation(&mut self, token: u64) -> bool {
        if self.authorized_nav_token == Some(token) {
            self.authorized_nav_token = None;
            return true;
        }
        false
    }

    pub(super) fn revoke_agent_navigation(&mut self) -> bool {
        self.authorized_nav_token.take().is_some()
    }

    /// Records a transport-stamped dialog opening using its wire generation and causal owner.
    pub(super) fn on_wire_dialog_opening(
        &mut self,
        opening: &codeterm_browser::WireDialogOpening,
    ) -> TrackedDialog {
        let dialog = ActiveDialog::from_wire_opening(opening);
        let owned_by_nav = if dialog.kind.is_beforeunload() {
            opening.owner_token
        } else {
            None
        };
        let tracked = TrackedDialog {
            generation: opening.generation,
            dialog,
            owned_by_nav,
        };
        self.active = Some(tracked.clone());
        tracked
    }

    /// Records a dialog opening without transport ownership (never auto-accepted).
    pub(super) fn on_unowned_dialog_opening(&mut self, dialog: ActiveDialog) -> TrackedDialog {
        let next_gen = self.active.as_ref().map_or(1, |t| t.generation + 1);
        let tracked = TrackedDialog {
            generation: next_gen,
            dialog,
            owned_by_nav: None,
        };
        self.active = Some(tracked.clone());
        tracked
    }

    /// True only when `nav_token` is still authorized and `generation` is still the active `beforeunload`.
    pub(super) fn can_accept_beforeunload(&self, nav_token: u64, generation: u64) -> bool {
        self.authorized_nav_token == Some(nav_token)
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

struct AgentNavigationGuard {
    shared: Arc<PageShared>,
    page: PageChannel,
    nav_token: u64,
}

impl Drop for AgentNavigationGuard {
    fn drop(&mut self) {
        self.page.disarm_dialog_owner(Some(self.nav_token));
        self.shared
            .end_agent_navigation(self.page.session_id(), self.nav_token);
    }
}

impl BrowserRuntime {
    pub(super) async fn navigate_for_agent(
        &self,
        page: &PageChannel,
        action: NavAction,
        target: Option<&str>,
    ) -> Result<String, String> {
        let _guard = self.action_gate.lock().await;
        let mut dialog_events = self.shared.subscribe_dialogs();
        self.shared.check_page_verb()?;
        let (method, params) = match action {
            NavAction::Reload => ("Page.reload", json!({})),
            NavAction::Goto => {
                let href = input::goto_href(target.unwrap_or(""))?;
                ("Page.navigate", json!({"url":href}))
            }
            NavAction::Back | NavAction::Forward => {
                let history = {
                    let fetch = self.send(page, "Page.getNavigationHistory", json!({}));
                    tokio::pin!(fetch);
                    loop {
                        tokio::select! {
                            result = &mut fetch => break result?,
                            changed = dialog_events.changed() => {
                                if changed.is_err() {
                                    break fetch.await?;
                                }
                                if let Some(tracked) = dialog_events.borrow_and_update().tracked.clone() {
                                    return Err(tracked.dialog.blocking_error());
                                }
                            }
                        }
                    }
                };
                match input::history_entry(&history, action == NavAction::Back) {
                    Some(entry_id) => ("Page.navigateToHistoryEntry", json!({"entryId":entry_id})),
                    None => return Ok("no history entry; no navigation performed".into()),
                }
            }
        };
        self.shared.check_page_verb()?;
        let nav_token = page.allocate_dialog_token();
        let nav_guard_slot: Mutex<Option<AgentNavigationGuard>> = Mutex::new(None);
        let nav_token_slot: Mutex<Option<u64>> = Mutex::new(None);
        let mut beforeunload_accepted = false;
        let mut handled_generation: Option<u64> = None;
        let outcome = {
            let mut pending_accept: Option<(
                u64,
                std::pin::Pin<
                    Box<
                        dyn std::future::Future<Output = Result<Value, codeterm_browser::CdpError>>
                            + Send
                            + '_,
                    >,
                >,
            )> = None;
            let nav_action = async {
                // `ActionCompletion::observe` finishes before `observation.finish` polls this future.
                // Transport `write_loop` arms `nav_token` only at actual wire write of `method`.
                self.shared
                    .register_agent_navigation(page.session_id(), nav_token)?;
                *nav_guard_slot.lock() = Some(AgentNavigationGuard {
                    shared: self.shared.clone(),
                    page: page.clone(),
                    nav_token,
                });
                *nav_token_slot.lock() = Some(nav_token);
                let shared = self.shared.clone();
                let session_id = page.session_id().to_string();
                let nav_gate: codeterm_browser::DispatchGate =
                    Arc::new(move || shared.is_agent_navigation_authorized(&session_id, nav_token));
                page.send_causal_nav(method, params, nav_token, nav_gate)
                    .await
                    .map_err(|error| {
                        if matches!(error, codeterm_browser::CdpError::Cancelled(_)) {
                            if let Some(blocking) = self.shared.active_dialog() {
                                return blocking.blocking_error();
                            }
                            if let Some(wire) = page.active_wire_dialog() {
                                return ActiveDialog::from_wire_opening(&wire).blocking_error();
                            }
                        }
                        super::cdp_error(error)
                    })
            };
            let completion = self.complete_action(page, true, nav_action);
            tokio::pin!(completion);
            loop {
                tokio::select! {
                    result = &mut completion => break result,
                    accept_result = async {
                        match pending_accept.as_mut() {
                            Some((_, fut)) => fut.await,
                            None => std::future::pending().await,
                        }
                    } => {
                        let accepted_gen = pending_accept.take().map(|(gen_id, _)| gen_id);
                        match accept_result {
                            Ok(_) => {
                                beforeunload_accepted = true;
                                if let Some(gen_id) = accepted_gen {
                                    self.shared.ack_beforeunload_closed(page.session_id(), gen_id);
                                }
                            }
                            Err(error) if is_no_dialog_cdp_error(&error.to_string()) => {
                                beforeunload_accepted = true;
                                if let Some(gen_id) = accepted_gen {
                                    self.shared.ack_beforeunload_closed(page.session_id(), gen_id);
                                }
                            }
                            Err(codeterm_browser::CdpError::Cancelled(_)) => {
                                if let Some(blocking) = self.shared.active_dialog() {
                                    break Err(blocking.blocking_error());
                                }
                                if let Some(wire) = page.active_wire_dialog() {
                                    break Err(ActiveDialog::from_wire_opening(&wire).blocking_error());
                                }
                            }
                            Err(error) => {
                                break Err(format!(
                                    "failed to accept beforeunload dialog during navigation: {error}"
                                ));
                            }
                        }
                    }
                    changed = dialog_events.changed() => {
                        if changed.is_err() {
                            break completion.await;
                        }
                        let snapshot = dialog_events.borrow_and_update().clone();
                        let current_nav = *nav_token_slot.lock();
                        let Some(tracked) = snapshot.tracked else {
                            pending_accept = None;
                            continue;
                        };
                        if current_nav.is_some()
                            && tracked.owned_by_nav == current_nav
                            && tracked.dialog.kind.is_beforeunload()
                        {
                            let token = current_nav.unwrap();
                            if !self.shared.can_accept_beforeunload(
                                page.session_id(),
                                token,
                                tracked.generation,
                            ) {
                                pending_accept = None;
                                if let Some(blocking) = self.shared.active_dialog() {
                                    break Err(blocking.blocking_error());
                                }
                                continue;
                            }
                            if handled_generation == Some(tracked.generation) {
                                continue;
                            }
                            handled_generation = Some(tracked.generation);
                            let shared = self.shared.clone();
                            let session_id = page.session_id().to_string();
                            let generation = tracked.generation;
                            let gate: codeterm_browser::DispatchGate = Arc::new(move || {
                                shared.can_accept_beforeunload(&session_id, token, generation)
                            });
                            pending_accept = Some((
                                generation,
                                Box::pin(page.send_dialog_accept(
                                    generation,
                                    token,
                                    DIALOG_DEADLINE,
                                    gate,
                                )),
                            ));
                        } else {
                            pending_accept = None;
                            break Err(tracked.dialog.blocking_error());
                        }
                    }
                }
            }
        };
        drop(nav_guard_slot);
        outcome.map(|(_, completion)| navigation_outcome(completion, beforeunload_accepted))
    }
}

#[cfg(test)]
mod tests;
