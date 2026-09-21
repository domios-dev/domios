//! Inbound grouping by conversation, and a tab's outbound traffic through its thread.

use std::sync::Arc;

use super::{send_media, OutboundMessage};
use crate::bridge::threads::TabText;
use crate::bridge::traits::{InboundMessage, OutboundContent};
use crate::ports::BridgeTabPort;

/// Debounced messages split by where they were posted, in first-seen order.
pub(super) fn group_by_context(messages: Vec<InboundMessage>) -> Vec<Vec<InboundMessage>> {
    let mut groups: Vec<Vec<InboundMessage>> = Vec::new();
    for message in messages {
        match groups
            .iter_mut()
            .find(|group| group[0].reply_ctx == message.reply_ctx)
        {
            Some(group) => group.push(message),
            None => groups.push(vec![message]),
        }
    }
    groups
}

pub(super) fn merge(group: Vec<InboundMessage>) -> InboundMessage {
    let text = group
        .iter()
        .map(|m| m.text.as_str())
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let attachments = group
        .iter()
        .flat_map(|m| m.attachments.iter().cloned())
        .collect();
    let first = &group[0];
    InboundMessage {
        service: first.service.clone(),
        user_id: first.user_id,
        user_name: first.user_name.clone(),
        text,
        attachments,
        message_id: first.message_id,
        callback_data: first.callback_data.clone(),
        reply_ctx: first.reply_ctx.clone(),
    }
}

pub(super) async fn send_for_tab(
    tab: &Arc<dyn BridgeTabPort>,
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    chat_id: i64,
    tab_id: &str,
    message: &OutboundMessage,
) {
    let threads = tab.threads();
    if let OutboundContent::Text { text } = &message.content {
        let placed = threads
            .route_text(
                tab.thread_tabs(),
                TabText {
                    tab_id: tab_id.to_string(),
                    chat_id,
                    markdown: text.clone(),
                    append: message.append,
                    buttons: message.buttons.clone(),
                },
            )
            .await;
        if let (Some(receipt), Some(message_id)) = (message.receipt.as_ref(), placed) {
            tab.record_channel_placement(
                receipt,
                &crate::bridge::telegram::locator(chat_id, message_id),
            );
        }
        return;
    }
    let destination = threads
        .media_destination(tab.thread_tabs(), tab_id, chat_id)
        .await;
    let attributed = attribute(&message.content, destination.header.as_deref());
    if let Err(error) = send_media(
        client,
        api_base,
        token,
        chat_id,
        destination.thread_id,
        &attributed,
    )
    .await
    {
        log::error!("[bridge-outbound] tab media send failed: {}", error);
        if let Some(thread_id) = destination.thread_id {
            threads.note_thread_send_failure(chat_id, thread_id, &error.to_string());
        }
        return;
    }
    threads
        .after_media(tab_id, chat_id, &destination, caption(&message.content))
        .await;
}

pub(super) async fn send_notification(
    tab: &Arc<dyn BridgeTabPort>,
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    chat_id: i64,
    message: &OutboundMessage,
    chat_states: &mut HashMap<i64, super::ChatState>,
) -> bool {
    let (tab_id, text) = match (&message.route, &message.edit, &message.content) {
        (
            super::OutboundRoute::TabNotification { tab_id },
            None,
            OutboundContent::Text { text },
        ) => (tab_id, text),
        _ => return false,
    };
    let threads = tab.threads();
    if !threads.is_attached() {
        return false;
    }
    let thread_id = threads
        .notification_thread_id(tab.thread_tabs(), tab_id, chat_id)
        .await;
    let placed = if let Some(thread_id) = thread_id {
        let placed = threads
            .send_notification_to_thread(
                tab.thread_tabs(),
                TabText {
                    tab_id: tab_id.clone(),
                    chat_id,
                    markdown: text.clone(),
                    append: message.append,
                    buttons: message.buttons.clone(),
                },
                thread_id,
            )
            .await;
        if let (Some(receipt), Some(message_id)) = (message.receipt.as_ref(), placed) {
            tab.record_channel_placement(
                receipt,
                &crate::bridge::telegram::locator(chat_id, message_id),
            );
        }
        return true;
    } else {
        super::route_text_outbound(
            client,
            api_base,
            token,
            chat_id,
            text,
            message.append,
            message.buttons.as_ref(),
            chat_states,
        )
        .await
    };
    if let (Some(receipt), Some(message_id)) = (message.receipt.as_ref(), placed) {
        tab.record_channel_placement(
            receipt,
            &crate::bridge::telegram::locator(chat_id, message_id),
        );
    }
    true
}

fn caption(content: &OutboundContent) -> &str {
    match content {
        OutboundContent::Photo { caption, .. }
        | OutboundContent::Document { caption, .. }
        | OutboundContent::Voice { caption, .. }
        | OutboundContent::Animation { caption, .. }
        | OutboundContent::Video { caption, .. } => caption.as_deref().unwrap_or_default(),
        OutboundContent::Sticker { .. } | OutboundContent::Text { .. } => "",
    }
}

fn attribute(content: &OutboundContent, header: Option<&str>) -> OutboundContent {
    let Some(header) = header else {
        return content.clone();
    };
    let headed = |caption: &Option<String>| {
        Some(
            match caption.as_deref().map(str::trim).filter(|c| !c.is_empty()) {
                Some(caption) => format!("{header}\n\n{caption}"),
                None => header.to_string(),
            },
        )
    };
    match content {
        OutboundContent::Photo { file, caption } => OutboundContent::Photo {
            file: file.clone(),
            caption: headed(caption),
        },
        OutboundContent::Document {
            file,
            caption,
            filename,
        } => OutboundContent::Document {
            file: file.clone(),
            caption: headed(caption),
            filename: filename.clone(),
        },
        OutboundContent::Voice { file, caption } => OutboundContent::Voice {
            file: file.clone(),
            caption: headed(caption),
        },
        OutboundContent::Animation { file, caption } => OutboundContent::Animation {
            file: file.clone(),
            caption: headed(caption),
        },
        OutboundContent::Video { file, caption } => OutboundContent::Video {
            file: file.clone(),
            caption: headed(caption),
        },
        other => other.clone(),
    }
}
