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
