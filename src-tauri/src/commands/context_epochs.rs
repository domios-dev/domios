use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::ContextEpochArchive;
use crate::ServiceState;

pub(crate) const CONTEXT_EPOCH_BOUNDARY_TEXT: &str =
    "Context reset. Earlier turns are archived here and are not sent to the provider.";

pub(crate) fn materialize_archived_context_epochs(
    svc: &ServiceState,
    archives: &[ContextEpochArchive],
) -> Vec<ChatMsg> {
    let mut output = Vec::new();
    for (epoch, archive) in archives.iter().enumerate() {
        let messages = if svc
            .agent_registry
            .with_provider(&archive.provider, |provider| {
                provider.uses_structured_chat()
            })
            .unwrap_or(false)
        {
            svc.agent_registry
                .with_provider(&archive.provider, |provider| {
                    provider
                        .read_structured_chat(&archive.cwd, &archive.session_id, None)
                        .messages
                })
                .unwrap_or_default()
        } else {
            svc.agent_registry
                .with_provider(&archive.provider, |provider| {
                    provider.session_jsonl_path(&archive.cwd, &archive.session_id)
                })
                .flatten()
                .map(|path| {
                    crate::service::tab_chat::session_window::snapshot(
                        &svc.agent_registry,
                        &archive.provider,
                        &path,
                    )
                    .messages
                })
                .unwrap_or_default()
        };
        output.extend(messages.into_iter().map(|mut message| {
            message.id = format!("epoch-{epoch}-{}-{}", archive.session_id, message.id);
            message
        }));
        output.push(ChatMsg {
            id: format!("context-epoch-boundary-{epoch}-{}", archive.session_id),
            kind: "system".into(),
            content: CONTEXT_EPOCH_BOUNDARY_TEXT.into(),
            system_subtype: Some("context_epoch_boundary".into()),
            ..Default::default()
        });
    }
    output
}
