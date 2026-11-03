use super::*;
use codeterm_types::agent::chat_types::{ChatMsg, RichMessage};
use parking_lot::Mutex;
use std::{collections::HashMap, sync::OnceLock};

#[derive(Default)]
struct TranscriptRetry {
    identity: (String, String),
    retry: RetryState,
    seen: Vec<String>,
    busy: bool,
    floor: u64,
}

fn states() -> &'static Mutex<HashMap<String, TranscriptRetry>> {
    static STATES: OnceLock<Mutex<HashMap<String, TranscriptRetry>>> = OnceLock::new();
    STATES.get_or_init(Mutex::default)
}

fn append_boundary(path: &std::path::Path) -> u64 {
    match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(_) => u64::MAX,
    }
}

pub(crate) fn retire(svc: &ServiceContext, tab: &str) {
    if let Err(error) = cancel(svc, tab) {
        log::warn!("[capacity-retry] {error}");
        return;
    }
    let mut states = states().lock();
    if states
        .get(tab)
        .is_some_and(|state| state.retry.cancelled.is_empty())
    {
        states.remove(tab);
    }
}

pub(crate) fn register(
    svc: &ServiceContext,
    tab: &str,
    session: &str,
    nonce: &str,
    path: &std::path::Path,
) {
    let floor = append_boundary(path);
    let mut states = states().lock();
    if states.len() >= 1024 && !states.contains_key(tab) {
        return;
    }
    let state = states.entry(tab.into()).or_default();
    if let Err(error) = remove_pending(svc, tab, &mut state.retry) {
        log::warn!("[capacity-retry] {error}");
        return;
    }
    state.identity = (session.into(), nonce.into());
    state.floor = floor;
    state.retry.attempts = 0;
    state.seen.clear();
    state.busy = false;
}

pub(super) fn cancel(svc: &ServiceContext, tab: &str) -> Result<(), String> {
    if let Some(state) = states().lock().get_mut(tab) {
        remove_pending(svc, tab, &mut state.retry)?;
        state.retry.attempts = 0;
        state.retry.error = None;
    }
    Ok(())
}

pub(super) fn skip_retry(svc: &ServiceContext, tab: &str, message: &str) -> Option<bool> {
    let identity = svc.mux.read().get_tab(tab).and_then(|tab| {
        Some((
            tab.agent_session_id.clone()?,
            tab.session_binding_nonce.clone()?,
        ))
    });
    let mut states = states().lock();
    let state = states.get_mut(tab)?;
    if state.retry.cancelled.remove(message) {
        return Some(true);
    }
    if state.retry.pending.as_deref() != Some(message) {
        return None;
    }
    state.retry.pending = None;
    Some(identity.as_ref() != Some(&state.identity) || state.busy)
}

pub(crate) fn observe(
    svc: &ServiceContext,
    tab: &str,
    session: &str,
    nonce: &str,
    records: &[RichMessage],
) {
    observe_at(
        svc,
        tab,
        session,
        nonce,
        records,
        crate::delayed_send::now_ms(),
    );
}

fn observe_at(
    svc: &ServiceContext,
    tab: &str,
    session: &str,
    nonce: &str,
    records: &[RichMessage],
    now: u64,
) {
    if engine::for_tab(tab).is_some() {
        return;
    }
    let current = svc.mux.read().get_tab(tab).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(session)
            && tab.session_binding_nonce.as_deref() == Some(nonce)
    });
    if !current {
        return;
    }
    let mut states = states().lock();
    let Some(state) = states.get_mut(tab) else {
        return;
    };
    let identity = (session.to_string(), nonce.to_string());
    if state.identity != identity {
        return;
    }
    let mut lines = Vec::new();
    for row in records {
        if row.provider_session_id.as_deref() != Some(session)
            || row.record_start.is_none_or(|offset| offset < state.floor)
        {
            continue;
        }
        let Some(id) = row.uuid.as_deref().filter(|id| !id.is_empty()) else {
            continue;
        };
        if !codeterm_provider_signals::admit_key(&mut state.seen, id) {
            continue;
        }
        let observation = row.provider_observations.as_ref();
        if row.role == "user" {
            state.busy = true;
            if let Err(error) = remove_pending(svc, tab, &mut state.retry) {
                log::warn!("[capacity-retry] {error}");
            }
            continue;
        }
        if observation.is_some_and(|observation| observation.retry_succeeded) {
            state.busy = false;
            let decision = state.retry.finish(id, None, now);
            if let Err(error) =
                apply_decision(svc, tab, Some(session.into()), &mut state.retry, decision)
            {
                log::warn!("[capacity-retry] {error}");
            }
            continue;
        }
        let Some(signal) = observation.and_then(|observation| observation.retry_error.as_ref())
        else {
            continue;
        };
        state.busy = false;
        state.retry.error = Some((id.into(), signal.clone()));
        let decision = state
            .retry
            .finish(id, Some(&RuntimeTurnDetail::Failed), now);
        match apply_decision(svc, tab, Some(session.into()), &mut state.retry, decision) {
            Ok(Some(content)) => lines.push(content),
            Ok(None) => {}
            Err(error) => log::warn!("[capacity-retry] {error}"),
        }
    }
    drop(states);
    crate::delayed_send::emit_changed(svc, tab);
    for content in lines {
        let item = ChatMsg {
            id: format!("capacity-retry:{}", uuid::Uuid::new_v4().simple()),
            kind: "system".into(),
            content,
            ..Default::default()
        };
        crate::service::tab_chat::structured_sink::emit_chat_msgs(
            svc.ui.as_ref(),
            tab,
            &[item],
            None,
            Some(session),
            Some(nonce),
        );
    }
}

#[cfg(test)]
mod tests;
