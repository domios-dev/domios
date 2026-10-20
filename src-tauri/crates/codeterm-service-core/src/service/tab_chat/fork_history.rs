use std::path::Path;

use super::{session_window, ChatMsg};
use crate::service::ServiceContext;

fn inherited_source(own_missing: bool, source: Option<&str>) -> Option<&str> {
    source.filter(|id| own_missing && !id.is_empty())
}

pub fn snapshot(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
    own_path: &Path,
) -> session_window::SessionSnapshot {
    let source = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .filter(|tab| tab.agent_session_id.as_deref() == Some(session_id))
            .and_then(|tab| tab.fork_source_session_id.clone())
    };
    let own_missing = std::fs::metadata(own_path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    let Some(source) = inherited_source(own_missing, source.as_deref()) else {
        return session_window::snapshot(&svc.agent_registry, provider_id, own_path);
    };
    let source_path = svc
        .agent_registry
        .with_provider(provider_id, |provider| {
            provider.session_jsonl_path(cwd, source)
        })
        .flatten();
    let Some(source_path) = source_path else {
        return session_window::SessionSnapshot::default();
    };
    let inherited = session_window::snapshot(&svc.agent_registry, provider_id, &source_path);
    let messages = inherited
        .messages
        .into_iter()
        .map(|mut message| {
            message.inherited_from_session_id = Some(source.to_owned());
            message.interactive.clear();
            message
        })
        .collect();
    log::debug!(
        "[tab_chat.fork_history] tab={tab_id} session={session_id} inherited_from={source}"
    );
    // Source offsets and observations must never become fork cursors or delivery evidence.
    session_window::SessionSnapshot {
        messages,
        ..Default::default()
    }
}

pub(super) fn messages(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
    since: usize,
) -> Result<(Vec<ChatMsg>, usize), String> {
    let path = svc
        .agent_registry
        .with_provider(provider_id, |provider| {
            (!provider.uses_structured_chat())
                .then(|| provider.session_jsonl_path(cwd, session_id))
                .flatten()
        })
        .flatten();
    let Some(path) = path else {
        return super::session_chat_snapshot(svc, provider_id, cwd, session_id, since);
    };
    let snapshot = snapshot(svc, tab_id, provider_id, cwd, session_id, &path);
    let total = snapshot.messages.len();
    Ok((
        snapshot
            .messages
            .into_iter()
            .skip(since)
            .map(ChatMsg::with_segments)
            .collect(),
        total,
    ))
}

#[cfg(test)]
mod tests;
