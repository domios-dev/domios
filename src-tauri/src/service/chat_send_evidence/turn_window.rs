use super::{AgentRegistry, EvidenceWindow, SendEvidenceAnchor};

#[derive(Clone)]
pub(super) struct StructuredUserTurn {
    pub(super) id: Option<String>,
    pub(super) content: String,
}

impl StructuredUserTurn {
    pub(super) fn without_identity(content: String) -> Self {
        Self { id: None, content }
    }
}

pub(super) fn structured_user_turns(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
) -> Option<Vec<StructuredUserTurn>> {
    let EvidenceWindow::Structured { cursor } = &anchor.window else {
        return None;
    };
    registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.read_structured_chat(&anchor.cwd, &anchor.session_id, cursor.as_deref())
        })
        .map(|chat| {
            log::debug!(
                "[session-turn-evidence] structured-read provider={} session={} boundary={:?} next_cursor={:?} rows={} user_rows={}",
                anchor.provider_id,
                anchor.session_id,
                cursor,
                chat.cursor,
                chat.messages.len(),
                chat.messages.iter().filter(|message| message.kind == "user").count(),
            );
            chat.messages
                .into_iter()
                .filter(|message| message.kind == "user")
                .map(|message| StructuredUserTurn {
                    id: (!message.id.is_empty()).then_some(message.id),
                    content: message.content,
                })
                .collect()
        })
}

pub(crate) fn window_user_turns(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
) -> Vec<String> {
    match &anchor.window {
        EvidenceWindow::Jsonl { transcript_offset } => {
            let Some(path) = registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
                })
                .flatten()
            else {
                return Vec::new();
            };
            if !path.exists() {
                return Vec::new();
            }
            registry
                .with_provider(&anchor.provider_id, |provider| {
                    provider.parse_session_delta(&path, *transcript_offset)
                })
                .map(|delta| {
                    delta
                        .messages
                        .iter()
                        .enumerate()
                        .flat_map(|(index, rich)| {
                            crate::service::tab_chat::rich_to_chat_msgs(rich, index)
                        })
                        .filter(|message| message.kind == "user")
                        .map(|message| message.content)
                        .collect()
                })
                .unwrap_or_default()
        }
        EvidenceWindow::Structured { .. } => structured_user_turns(registry, anchor)
            .unwrap_or_default()
            .into_iter()
            .map(|turn| turn.content)
            .collect(),
    }
}

pub(crate) fn window_has_foreign_user_turn(
    registry: &AgentRegistry,
    anchor: &SendEvidenceAnchor,
    source: &crate::agent::provider::ModelSwitchTranscript,
) -> Result<bool, String> {
    let EvidenceWindow::Jsonl { transcript_offset } = &anchor.window else {
        return Ok(!window_user_turns(registry, anchor).is_empty());
    };
    if source.record_type.is_empty() || source.content_path.is_empty() {
        return Ok(!window_user_turns(registry, anchor).is_empty());
    }
    let path = registry
        .with_provider(&anchor.provider_id, |provider| {
            provider.session_jsonl_path(&anchor.cwd, &anchor.session_id)
        })
        .flatten()
        .ok_or("the own-session command transcript path is unavailable")?;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && *transcript_offset == 0 => {
            return Ok(false)
        }
        Err(error) => {
            return Err(format!(
                "cannot read the own-session command transcript: {error}"
            ))
        }
    };
    if file.metadata().map_err(|error| error.to_string())?.len() < *transcript_offset {
        return Err(
            "the own-session command transcript was truncated past its turn boundary".into(),
        );
    }
    use std::io::{Read, Seek, SeekFrom};
    file.seek(SeekFrom::Start(*transcript_offset))
        .map_err(|error| error.to_string())?;
    let mut bytes = String::new();
    file.take(1024 * 1024 + 1)
        .read_to_string(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("the command transcript window exceeds its classification limit".into());
    }
    source.has_foreign_user_turn(&bytes)
}
