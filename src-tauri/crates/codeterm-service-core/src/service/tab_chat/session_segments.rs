use crate::{
    agent::chat_types::ChatMsg,
    multiplexer::types::{ContextEpochArchive, ContextEpochBoundary, ContextEpochTransport},
    service::ServiceContext,
};

pub(crate) struct TimelineSegment {
    pub session_id: String,
    pub boundary_before: Option<ContextEpochBoundary>,
    pub read_only: bool,
    pub messages: Vec<ChatMsg>,
}

pub(crate) fn project_timeline(segments: Vec<TimelineSegment>) -> Vec<ChatMsg> {
    let mut output = Vec::new();
    let mut previous_session_id: Option<String> = None;
    for segment in segments {
        if let (Some(previous), Some(boundary)) =
            (previous_session_id.as_deref(), segment.boundary_before)
        {
            output.push(boundary_message(previous, &segment.session_id, boundary));
        }
        for mut message in segment.messages {
            if segment.read_only {
                scope_historical_message(&mut message, &segment.session_id);
                message.interactive.clear();
            }
            message.seq = sequence(output.len());
            output.push(message);
        }
        previous_session_id = Some(segment.session_id);
    }
    output
}

pub fn archived_provider_sessions(
    svc: &ServiceContext,
    archives: &[ContextEpochArchive],
    current_session_id: &str,
) -> Vec<ChatMsg> {
    let mut segments = archived_segments(svc, archives);
    if let Some(previous) = archives.last() {
        segments.push(TimelineSegment {
            session_id: current_session_id.to_owned(),
            boundary_before: Some(previous.boundary),
            read_only: false,
            messages: Vec::new(),
        });
    }
    project_timeline(segments)
}

pub fn project_with_archived_provider_sessions(
    svc: &ServiceContext,
    archives: &[ContextEpochArchive],
    current_session_id: &str,
    current_messages: Vec<ChatMsg>,
) -> Vec<ChatMsg> {
    let mut segments = archived_segments(svc, archives);
    segments.push(TimelineSegment {
        session_id: current_session_id.to_owned(),
        boundary_before: archives.last().map(|archive| archive.boundary),
        read_only: false,
        messages: current_messages,
    });
    project_timeline(segments)
}

fn archived_segments(
    svc: &ServiceContext,
    archives: &[ContextEpochArchive],
) -> Vec<TimelineSegment> {
    archives
        .iter()
        .enumerate()
        .map(|(index, archive)| TimelineSegment {
            session_id: archive.session_id.clone(),
            boundary_before: index
                .checked_sub(1)
                .and_then(|previous| archives.get(previous))
                .map(|previous| previous.boundary),
            read_only: true,
            messages: archived_session_messages(svc, archive),
        })
        .collect()
}

fn archived_session_messages(svc: &ServiceContext, archive: &ContextEpochArchive) -> Vec<ChatMsg> {
    let result = match archive.transport {
        ContextEpochTransport::AgentProvider => super::session_chat_snapshot(
            svc,
            &archive.provider,
            &archive.cwd,
            &archive.session_id,
            0,
        )
        .map(|(messages, _)| messages),
        ContextEpochTransport::ChatBackend => svc
            .chatbackend_registry
            .get(&archive.provider)
            .ok_or_else(|| "chat backend provider is unavailable".to_owned())
            .and_then(|provider| {
                super::chatbackend_transcript_snapshot(provider.as_ref(), &archive.session_id, 0)
            })
            .map(|(messages, _)| messages),
    };
    match result {
        Ok(messages) => messages,
        Err(error) => {
            log::warn!(
                "[tab_chat.archive] provider={} session={} unavailable: {}",
                archive.provider,
                archive.session_id,
                error
            );
            vec![ChatMsg {
                id: format!("archived-session-unavailable-{}", archive.session_id),
                kind: "system".into(),
                content: "Archived provider conversation is unavailable.".into(),
                system_subtype: Some("archived_session_unavailable".into()),
                ..Default::default()
            }]
        }
    }
}

fn scope_historical_message(message: &mut ChatMsg, session_id: &str) {
    message.id = format!("provider-session:{session_id}:{}", message.id);
    for value in [
        &mut message.tool_id,
        &mut message.tool_flow_id,
        &mut message.tool_call_source_id,
        &mut message.tool_background_task_id,
    ] {
        if let Some(value) = value {
            *value = format!("provider-session:{session_id}:{value}");
        }
    }
}

fn boundary_message(
    previous_session_id: &str,
    next_session_id: &str,
    boundary: ContextEpochBoundary,
) -> ChatMsg {
    let (content, subtype) = match boundary {
        ContextEpochBoundary::ContextReset => (
            "Context reset. Earlier turns are archived here and are not sent to the provider.",
            "context_epoch_boundary",
        ),
        ContextEpochBoundary::ProviderHandoff => (
            "Provider handoff. Earlier conversation remains archived above.",
            "provider_handoff",
        ),
    };
    ChatMsg {
        id: format!("session-boundary:{previous_session_id}:{next_session_id}"),
        kind: "system".into(),
        content: content.into(),
        system_subtype: Some(subtype.into()),
        ..Default::default()
    }
}

fn sequence(index: usize) -> i64 {
    i64::try_from(index).unwrap_or(i64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::chat_types::{InteractiveElement, InteractiveOption, InteractiveQuestion};

    fn interactive() -> Vec<InteractiveElement> {
        vec![InteractiveElement::Question(InteractiveQuestion {
            id: "question-1".into(),
            tool_use_id: "tool-1".into(),
            prompt: "Choose one".into(),
            header: None,
            options: vec![InteractiveOption {
                value: Some("one".into()),
                label: "One".into(),
                description: None,
            }],
            multi_select: false,
        })]
    }

    #[test]
    fn provider_handoff_projects_previous_and_current_sessions_in_order() {
        let rows = project_timeline(vec![
            TimelineSegment {
                session_id: "previous-session".into(),
                boundary_before: None,
                read_only: true,
                messages: vec![ChatMsg {
                    id: "previous-user".into(),
                    kind: "user".into(),
                    content: "earlier question".into(),
                    interactive: interactive(),
                    ..Default::default()
                }],
            },
            TimelineSegment {
                session_id: "current-session".into(),
                boundary_before: Some(ContextEpochBoundary::ProviderHandoff),
                read_only: false,
                messages: vec![
                    ChatMsg {
                        id: "handoff-user".into(),
                        kind: "user".into(),
                        content: "Previous conversation handoff".into(),
                        interactive: interactive(),
                        ..Default::default()
                    },
                    ChatMsg {
                        id: "current-reply".into(),
                        kind: "assistant".into(),
                        content: "continued".into(),
                        ..Default::default()
                    },
                ],
            },
        ]);

        assert_eq!(
            rows.iter()
                .map(|row| row.content.as_str())
                .collect::<Vec<_>>(),
            [
                "earlier question",
                "Provider handoff. Earlier conversation remains archived above.",
                "Previous conversation handoff",
                "continued",
            ]
        );
        assert!(rows[0].id.starts_with("provider-session:previous-session:"));
        assert!(rows[0].interactive.is_empty());
        assert_eq!(rows[1].system_subtype.as_deref(), Some("provider_handoff"));
        assert_eq!(rows[2].interactive.len(), 1);
        assert_eq!(
            rows.iter().map(|row| row.seq).collect::<Vec<_>>(),
            [0, 1, 2, 3]
        );
    }
}
