//! A provider that refuses to load a session which never produced agent work
//! leaves nothing for the owner to choose between: the host starts a fresh
//! session in the same tab. Any agent row keeps the owner's explicit choice.

use super::{engine::Session, model::RuntimeFailure, transcript};
use crate::service::ServiceContext;
use codeterm_types::agent::chat_types::ChatMsg;
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    sync::{Arc, LazyLock},
};

const ATTEMPT_CAP: usize = 256;

static ATTEMPTED: LazyLock<Mutex<VecDeque<(String, String)>>> =
    LazyLock::new(|| Mutex::new(VecDeque::new()));

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LostSessionChoice {
    FreshLaunch,
    Owner,
}

/// Agent-produced rows prove a turn reached the provider; rows inherited from a
/// fork parent belong to another session.
fn agent_worked(rows: &[ChatMsg]) -> bool {
    rows.iter().any(|row| {
        row.inherited_from_session_id.is_none()
            && matches!(
                row.kind.as_str(),
                "assistant" | "thinking" | "tool" | "tool_call" | "tool_result"
            )
    })
}

/// `rows` is the tab's own durable transcript for the lost session; `None`
/// (unreadable or not host-kept) leaves the choice to the owner.
pub(super) fn choose(rows: Option<&[ChatMsg]>) -> LostSessionChoice {
    match rows {
        Some(rows) if !agent_worked(rows) => LostSessionChoice::FreshLaunch,
        _ => LostSessionChoice::Owner,
    }
}

fn claim(tab: &str, thread: &str) -> bool {
    let mut attempted = ATTEMPTED.lock();
    if attempted
        .iter()
        .any(|(known_tab, known_thread)| known_tab == tab && known_thread == thread)
    {
        return false;
    }
    if attempted.len() >= ATTEMPT_CAP {
        attempted.pop_front();
    }
    attempted.push_back((tab.into(), thread.into()));
    true
}

#[cfg(test)]
static LAUNCHES: LazyLock<Mutex<std::collections::HashMap<String, tokio::task::JoinHandle<()>>>> =
    LazyLock::new(Default::default);

#[cfg(test)]
pub(crate) fn join_for_test(tab: &str) -> bool {
    let handle = LAUNCHES.lock().remove(tab);
    handle.is_some_and(|handle| crate::async_runtime::block_on(handle).is_ok())
}

/// A wake or send that meets a provider-lost session names that cause, so tab
/// status, senders and restore presentation offer Start new session.
pub(crate) fn wake_failure(svc: &ServiceContext, tab: &str, failure: &RuntimeFailure) -> String {
    let RuntimeFailure::SessionLoadRefused(detail) = failure else {
        return failure.to_string();
    };
    let thread = svc
        .mux
        .read()
        .get_tab(tab)
        .and_then(|target| target.agent_session_id.clone())
        .unwrap_or_default();
    format!(
        "restored_session_unavailable: session_missing: The provider no longer has session {thread} for Tab {tab}; its saved transcript is kept, start a new session to continue ({detail})"
    )
}

/// Runs after the ensure gate is released, because the fresh restart takes it.
pub(super) fn schedule(svc: &ServiceContext, session: &Arc<Session>, reason: &RuntimeFailure) {
    if !matches!(reason, RuntimeFailure::SessionLoadRefused(_)) {
        return;
    }
    let context = session.context.lock().clone();
    let Some(thread) = context.thread_id.filter(|thread| !thread.is_empty()) else {
        return;
    };
    if !claim(&context.tab_id, &thread) {
        return;
    }
    let svc = svc.clone();
    let session = session.clone();
    let tab = context.tab_id;
    let _launch = tokio::task::spawn_blocking({
        let tab = tab.clone();
        move || launch(&svc, &session, &tab, &thread)
    });
    #[cfg(test)]
    LAUNCHES.lock().insert(tab, _launch);
}

fn launch(svc: &ServiceContext, session: &Session, tab: &str, thread: &str) {
    if !super::engine::session_load_refused(svc, session)
        || session.context.lock().thread_id.as_deref() != Some(thread)
    {
        return;
    }
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime());
    let rows = capability
        .and_then(|capability| transcript::durable_rows(&session.provider, thread, &capability));
    if choose(rows.as_deref()) != LostSessionChoice::FreshLaunch {
        return;
    }
    match svc.agent_restart_observed_with_options(
        tab,
        crate::service::agent_restart::RestartOptions {
            fresh: true,
            ..Default::default()
        },
    ) {
        Ok(_) => svc.record_lifecycle(
            tab,
            crate::session::lifecycle::LifecycleTransition::RolledBack {
                reason: format!(
                    "automatic fresh session: the provider lost unanswered session {thread}, now superseded"
                ),
            },
        ),
        Err(error) => log::warn!(
            "[agent-runtime-restore] tab={tab}: automatic fresh session after lost session {thread} failed: {error:?}"
        ),
    }
}

#[cfg(test)]
mod tests;
