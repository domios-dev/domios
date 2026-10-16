use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::Arc;

use super::registry::{JsonlSource, SourceIncarnation, SourceObservation};
use super::resume_cursor::{
    plan_resume, ChatEpoch, ResumeCursor, ResumePlan, ResumeResetReason, ResumeSourceKind,
};
use crate::agent::chat_types::ChatMsg;
use crate::agent::registry::AgentRegistry;
use crate::service::ServiceState;

pub const RESUME_PAGE_BYTES: u64 = 1024 * 1024;

#[derive(serde::Serialize)]
pub struct ResumeDelta {
    pub messages: Vec<ChatMsg>,
    pub start_offset: u64,
    pub offset: u64,
    pub cursor: String,
    pub source_session_id: String,
    pub session_binding_nonce: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatResumeResult {
    Resumed {
        deltas: Vec<ResumeDelta>,
        caught_up: bool,
    },
    ResetRequired {
        reason: ResumeResetReason,
    },
    Unsupported {
        source_kind: ResumeSourceKind,
    },
}

pub fn encode_cursor(
    epoch: ChatEpoch,
    incarnation: &SourceIncarnation,
    offset: u64,
) -> Option<String> {
    ResumeCursor::Bytes {
        epoch,
        incarnation: incarnation.id.to_string(),
        offset,
    }
    .encode()
    .ok()
}

pub fn capture_incarnation(
    source: &JsonlSource,
    epoch: &ChatEpoch,
    path: &Path,
    offset: u64,
    held: Option<&SourceIncarnation>,
) -> Option<SourceIncarnation> {
    match source.observe(&epoch.source_session_id, path, held, offset) {
        SourceObservation::Present {
            incarnation,
            restart: false,
        } => incarnation,
        _ => None,
    }
}

fn boundary(path: &Path, offset: u64) -> Result<bool, String> {
    if offset == 0 {
        return Ok(true);
    }
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    file.seek(SeekFrom::Start(offset - 1))
        .map_err(|error| error.to_string())?;
    let mut last = [0];
    file.read_exact(&mut last)
        .map_err(|error| error.to_string())?;
    Ok(last == [b'\n'])
}

fn reset(reason: ResumeResetReason) -> ChatResumeResult {
    ChatResumeResult::ResetRequired { reason }
}

pub fn read_page(
    registry: &Arc<AgentRegistry>,
    provider_id: &str,
    source: &JsonlSource,
    path: &Path,
    epoch: &ChatEpoch,
    cursor: &ResumeCursor,
) -> Result<ChatResumeResult, String> {
    read_page_with(source, path, epoch, cursor, |from, budget| {
        registry
            .with_provider(provider_id, |provider| {
                provider
                    .parse_session_delta_bounded(path, from, budget)
                    .map(|mut delta| {
                        for message in &mut delta.messages {
                            super::enrich_interactive(message, provider);
                        }
                        delta
                    })
            })
            .flatten()
    })
}

fn read_page_with(
    source: &JsonlSource,
    path: &Path,
    epoch: &ChatEpoch,
    cursor: &ResumeCursor,
    parse: impl FnOnce(u64, u64) -> Option<crate::agent::chat_types::SessionDelta>,
) -> Result<ChatResumeResult, String> {
    let ResumeCursor::Bytes { offset, .. } = cursor else {
        return Ok(ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Bytes,
        });
    };
    let SourceObservation::Present {
        incarnation: Some(incarnation),
        ..
    } = source.observe(&epoch.source_session_id, path, None, *offset)
    else {
        return Ok(reset(ResumeResetReason::Compacted));
    };
    let length = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let aligned = *offset <= length && boundary(path, *offset)?;
    let from = match plan_resume(
        cursor,
        epoch,
        &incarnation.id,
        length,
        aligned,
        RESUME_PAGE_BYTES,
    ) {
        ResumePlan::Reset(reason) => return Ok(reset(reason)),
        ResumePlan::Unsupported(source_kind) => {
            return Ok(ChatResumeResult::Unsupported { source_kind })
        }
        ResumePlan::Empty => {
            return Ok(ChatResumeResult::Resumed {
                deltas: vec![],
                caught_up: true,
            })
        }
        ResumePlan::Read { from, .. } => from,
    };
    let parsed = parse(from, RESUME_PAGE_BYTES);
    let Some(parsed) = parsed else {
        return Ok(ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Bytes,
        });
    };
    match source.observe(
        &epoch.source_session_id,
        path,
        Some(&incarnation),
        parsed.new_offset,
    ) {
        SourceObservation::Present {
            incarnation: Some(current),
            restart: false,
        } if current == incarnation => {}
        _ => return Ok(reset(ResumeResetReason::Compacted)),
    }
    if parsed.new_offset < from {
        return Ok(reset(ResumeResetReason::InvalidCursor));
    }
    let caught_up = parsed.new_offset >= length || parsed.new_offset == from;
    if parsed.new_offset == from {
        return Ok(ChatResumeResult::Resumed {
            deltas: vec![],
            caught_up,
        });
    }
    let next_cursor = encode_cursor(epoch.clone(), &incarnation, parsed.new_offset)
        .ok_or_else(|| "Could not encode resume cursor".to_string())?;
    Ok(ChatResumeResult::Resumed {
        deltas: vec![ResumeDelta {
            messages: super::rich_rows_to_chat_msgs(&parsed.messages, from),
            start_offset: from,
            offset: parsed.new_offset,
            cursor: next_cursor,
            source_session_id: epoch.source_session_id.clone(),
            session_binding_nonce: epoch.session_binding_nonce.clone(),
        }],
        caught_up,
    })
}

#[cfg(test)]
mod tests;

pub fn resume(svc: &ServiceState, tab_id: &str, token: &str) -> Result<ChatResumeResult, String> {
    let (provider_id, cwd, epoch) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("Tab not found")?;
        (
            tab.agent_type.clone().unwrap_or_default(),
            tab.cwd.clone().unwrap_or_default(),
            ChatEpoch {
                source_session_id: tab.agent_session_id.clone().unwrap_or_default(),
                session_binding_nonce: tab.session_binding_nonce.clone(),
            },
        )
    };
    if crate::service::agent_runtime::requested_for_tab(svc, tab_id)
        || crate::service::codex_rpc::lifecycle::for_tab(tab_id).is_some()
    {
        return Ok(ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Items,
        });
    }
    let (structured, path) = svc
        .agent_registry
        .with_provider(&provider_id, |provider| {
            (
                provider.uses_structured_chat(),
                provider.session_jsonl_path(&cwd, &epoch.source_session_id),
            )
        })
        .ok_or("Provider not available")?;
    if structured {
        return Ok(ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Revision,
        });
    }
    let cursor = match ResumeCursor::decode(token) {
        Ok(cursor) => cursor,
        Err(reason) => return Ok(reset(reason)),
    };
    let Some(path) = path else {
        return Ok(ChatResumeResult::Unsupported {
            source_kind: ResumeSourceKind::Bytes,
        });
    };
    let source = svc.chat_registry.jsonl_source(tab_id);
    let result = read_page(
        &svc.agent_registry,
        &provider_id,
        &source,
        &path,
        &epoch,
        &cursor,
    )?;
    let authority_matches = svc.mux.read().get_tab(tab_id).is_some_and(|tab| {
        tab.agent_session_id.as_deref() == Some(epoch.source_session_id.as_str())
            && tab.session_binding_nonce == epoch.session_binding_nonce
    });
    Ok(if authority_matches {
        result
    } else {
        reset(ResumeResetReason::EpochChanged)
    })
}
