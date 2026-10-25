use std::path::{Path, PathBuf};

use super::{session_window, ChatMsg};
use crate::service::ServiceContext;

fn inherited_source(own_missing: bool, source: Option<&str>) -> Option<&str> {
    source.filter(|id| own_missing && !id.is_empty())
}

enum HistorySource {
    Own,
    Inherited { session_id: String, path: PathBuf },
    Unavailable,
}

fn history_source(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
    own_path: &Path,
) -> HistorySource {
    let source = {
        let mux = svc.mux.read();
        mux.get_tab(tab_id)
            .filter(|tab| tab.agent_session_id.as_deref() == Some(session_id))
            .and_then(|tab| tab.fork_source_session_id.clone())
    };
    let own_missing = std::fs::metadata(own_path)
        .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
    let Some(source) = inherited_source(own_missing, source.as_deref()) else {
        return HistorySource::Own;
    };
    svc.agent_registry
        .with_provider(provider_id, |provider| {
            provider.session_jsonl_path(cwd, source)
        })
        .flatten()
        .map_or(HistorySource::Unavailable, |path| {
            HistorySource::Inherited {
                session_id: source.to_owned(),
                path,
            }
        })
}

fn mark_inherited(messages: Vec<ChatMsg>, source: &str) -> Vec<ChatMsg> {
    messages
        .into_iter()
        .map(|mut message| {
            message.inherited_from_session_id = Some(source.to_owned());
            message.interactive.clear();
            message
        })
        .collect()
}

pub fn snapshot(
    svc: &ServiceContext,
    tab_id: &str,
    provider_id: &str,
    cwd: &str,
    session_id: &str,
    own_path: &Path,
) -> session_window::SessionSnapshot {
    let (source, source_path) =
        match history_source(svc, tab_id, provider_id, cwd, session_id, own_path) {
            HistorySource::Own => {
                return session_window::snapshot(&svc.agent_registry, provider_id, own_path)
            }
            HistorySource::Unavailable => return session_window::SessionSnapshot::default(),
            HistorySource::Inherited { session_id, path } => (session_id, path),
        };
    let inherited = session_window::snapshot(&svc.agent_registry, provider_id, &source_path);
    log::debug!(
        "[tab_chat.fork_history] tab={tab_id} session={session_id} inherited_from={source}"
    );
    // Source offsets and observations must never become fork cursors or delivery evidence.
    session_window::SessionSnapshot {
        messages: mark_inherited(inherited.messages, &source),
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
    let tails = &svc.chat_registry.tail_cache;
    let source = history_source(svc, tab_id, provider_id, cwd, session_id, &path);
    if matches!(source, HistorySource::Own) {
        match std::fs::metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok((Vec::new(), 0));
            }
            Err(error) => return Err(format!("{}: {error}", path.display())),
            Ok(_) => {}
        }
    }
    let messages = match source {
        HistorySource::Own => {
            tails
                .jsonl_tail(
                    &svc.agent_registry,
                    provider_id,
                    &path,
                    &format!("tab:{tab_id}"),
                )?
                .0
        }
        HistorySource::Unavailable => Vec::new(),
        HistorySource::Inherited {
            session_id: source,
            path,
        } => mark_inherited(
            tails
                .jsonl_tail(
                    &svc.agent_registry,
                    provider_id,
                    &path,
                    &format!("tab:{tab_id}/inherited:{source}"),
                )?
                .0,
            &source,
        ),
    };
    let total = messages.len();
    Ok((
        messages
            .into_iter()
            .skip(since)
            .map(ChatMsg::with_segments)
            .collect(),
        total,
    ))
}

#[cfg(test)]
mod tests;
