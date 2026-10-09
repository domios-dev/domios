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

fn command_content(
    content: &str,
    source: &crate::agent::provider::ModelSwitchTranscript,
    ignored: &[regex::Regex],
) -> bool {
    (!source.prefix.is_empty()
        && !source.suffix.is_empty()
        && content
            .strip_prefix(&source.prefix)
            .and_then(|body| body.strip_suffix(&source.suffix))
            .is_some_and(|body| !body.contains(&source.suffix)))
        || ignored.iter().any(|pattern| {
            pattern
                .find(content)
                .is_some_and(|found| found.start() == 0 && found.end() == content.len())
        })
}

pub(crate) fn jsonl_has_foreign_user_turn(
    bytes: &str,
    source: &crate::agent::provider::ModelSwitchTranscript,
) -> Result<bool, String> {
    let ignored = source
        .ignored_content_regexes
        .iter()
        .map(|pattern| regex::Regex::new(pattern))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("invalid command transcript exclusion: {error}"))?;
    for line in bytes.lines().filter(|line| !line.trim().is_empty()) {
        let record: serde_json::Value = serde_json::from_str(line)
            .map_err(|error| format!("cannot classify the command transcript window: {error}"))?;
        if record["type"].as_str() != Some(source.record_type.as_str())
            && record["type"].as_str() != Some("user")
        {
            continue;
        }
        let content = source
            .content_path
            .iter()
            .try_fold(&record, |value, key| value.get(key));
        let excluded = match content {
            Some(serde_json::Value::String(content)) => command_content(content, source, &ignored),
            Some(serde_json::Value::Array(blocks)) if !blocks.is_empty() => {
                blocks.iter().all(|block| {
                    block["type"] == "tool_result"
                        || (block["type"] == "text"
                            && block["text"]
                                .as_str()
                                .is_some_and(|text| command_content(text, source, &ignored)))
                })
            }
            _ => false,
        };
        if !excluded {
            return Ok(true);
        }
    }
    Ok(false)
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
    jsonl_has_foreign_user_turn(&bytes, source)
}
