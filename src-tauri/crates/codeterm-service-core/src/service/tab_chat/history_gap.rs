//! Explicit boundaries for bounded transcript reads: a truncated window starts
//! with a gap row, and the gap pages older rows of its own source by offset.

use crate::agent::chat_types::{ChatHistorySource, ChatMsg, ChatTimelineMarker};
use crate::multiplexer::types::{ContextEpochTransport, TabBacking};
use crate::service::ServiceContext;

pub const SUBTYPE: &str = "history_gap";

pub fn marker(source: ChatHistorySource, session_id: &str, before_offset: u64) -> ChatMsg {
    ChatMsg {
        id: format!("history-gap:{session_id}:{before_offset}"),
        kind: "system".into(),
        content: "Earlier history is not loaded.".into(),
        system_subtype: Some(SUBTYPE.into()),
        timeline_marker: Some(ChatTimelineMarker::HistoryGap {
            source,
            session_id: session_id.into(),
            before_offset,
        }),
        ..Default::default()
    }
}

/// Prefix `messages` with a gap row when the window did not reach the start.
pub fn bounded(
    source: ChatHistorySource,
    session_id: &str,
    start_offset: u64,
    messages: Vec<ChatMsg>,
) -> Vec<ChatMsg> {
    if start_offset == 0 {
        return messages;
    }
    std::iter::once(marker(source, session_id, start_offset))
        .chain(messages)
        .collect()
}

enum Owner {
    Current,
    Archived { provider: String, cwd: String },
}

fn owner(
    svc: &ServiceContext,
    tab_id: &str,
    source: ChatHistorySource,
    session_id: &str,
) -> Result<(Owner, String), String> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
    if let TabBacking::ChatBackend {
        plugin_id,
        session_id: current,
    } = &tab.backing
    {
        if source == ChatHistorySource::ChatBackend && current == session_id {
            return Ok((Owner::Current, plugin_id.clone()));
        }
    }
    let transport = match source {
        ChatHistorySource::ChatBackend => ContextEpochTransport::ChatBackend,
        ChatHistorySource::ProviderSession => ContextEpochTransport::AgentProvider,
    };
    tab.context_epoch_archives
        .iter()
        .find(|archive| archive.session_id == session_id && archive.transport == transport)
        .map(|archive| {
            (
                Owner::Archived {
                    provider: archive.provider.clone(),
                    cwd: archive.cwd.clone(),
                },
                archive.provider.clone(),
            )
        })
        .ok_or_else(|| format!("Session {session_id} is not part of this tab's history"))
}

/// Rows of `session_id` ending at `before_offset`, led by a fresh gap row when
/// older rows still remain.
pub fn page(
    svc: &ServiceContext,
    tab_id: &str,
    source: ChatHistorySource,
    session_id: &str,
    before_offset: u64,
) -> Result<Vec<ChatMsg>, String> {
    let (owner, provider_id) = owner(svc, tab_id, source, session_id)?;
    let (start_offset, messages) = match source {
        ChatHistorySource::ChatBackend => {
            let path = super::chatbackend_history::session_path(&provider_id, session_id);
            let window = super::chatbackend_history::read_window(
                &path,
                before_offset,
                super::chatbackend_history::PAGE_BYTES,
            )
            .map_err(|error| error.to_string())?
            .keep_last(super::snapshot_rows::SNAPSHOT_ROW_LIMIT);
            (window.start_offset, window.messages)
        }
        ChatHistorySource::ProviderSession => {
            let Owner::Archived { provider, cwd } = &owner else {
                return Err("Provider sessions page through chat backfill".into());
            };
            let path = svc
                .agent_registry
                .with_provider(provider, |p| {
                    (!p.uses_structured_chat())
                        .then(|| p.session_file_path(cwd, session_id))
                        .flatten()
                })
                .flatten()
                .ok_or("Archived session has no byte-addressed transcript")?;
            let page = super::session_window::parse_page_before(
                &svc.agent_registry,
                provider,
                &path,
                before_offset,
            )?;
            let (start, messages) =
                super::snapshot_rows::retain_tail(&page.messages, page.start_offset);
            (
                start,
                messages.into_iter().map(ChatMsg::with_segments).collect(),
            )
        }
    };
    if start_offset >= before_offset {
        return Err("No older history before this offset".into());
    }
    let mut rows = bounded(source, session_id, start_offset, messages);
    if matches!(owner, Owner::Archived { .. }) {
        for row in &mut rows {
            super::session_segments::scope_archived(row, session_id);
        }
    }
    Ok(rows)
}

#[cfg(test)]
mod tests;
