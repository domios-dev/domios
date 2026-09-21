//! Inbound grouping by conversation, and a tab's outbound traffic through its thread.

use std::collections::HashMap;
use std::sync::Arc;

use super::{
    rewrite_message, route_text_outbound, send_media, ChatState, OutboundMessage, OutboundRoute,
};
use crate::bridge::threads::TabText;
use crate::bridge::traits::{InboundMessage, OutboundContent};
use crate::ports::BridgeTabPort;

pub(super) async fn send_outbound(
    tab: &Arc<dyn BridgeTabPort>,
    client: &reqwest::Client,
    api_base: &str,
    token: &str,
    chat_id: i64,
    message: &OutboundMessage,
    chat_states: &mut HashMap<i64, ChatState>,
) {
    if notification_text(message).is_some() && tab.threads().is_attached() {
        if !send_notification(tab, chat_id, message).await {
            log::error!(
                "[bridge-outbound] notification placement failed; suppressing raw fallback"
            );
        }
        return;
    }

    if let (OutboundRoute::Tab { tab_id }, None) = (&message.route, message.edit.as_ref()) {
        if tab.threads().is_attached() {
            send_for_tab(tab, client, api_base, token, chat_id, tab_id, message).await;
            return;
        }
    }

    match &message.content {
        OutboundContent::Text { text } => {
            if let Some(edit) = message.edit.as_ref() {
                // A rewrite addresses one already-sent message, so it never touches the last-message state the append path keeps.
                log::info!(
                    "[bridge-outbound] rewriting message {} in {}",
                    edit.message_id,
                    chat_id
                );
                rewrite_message(
                    client,
                    api_base,
                    token,
                    chat_id,
                    edit.message_id,
                    text,
                    message.buttons.as_ref(),
                )
                .await;
                return;
            }
            log::info!(
                "[bridge-outbound] {} {} chars to {}",
                if message.append {
                    "appending"
                } else {
                    "sending"
                },
                text.len(),
                chat_id
            );
            let placed = route_text_outbound(
                client,
                api_base,
                token,
                chat_id,
                text,
                message.append,
                message.buttons.as_ref(),
                chat_states,
            )
            .await;
            if let (Some(receipt), Some(message_id)) = (message.receipt.as_ref(), placed) {
                tab.record_channel_placement(
                    receipt,
                    &crate::bridge::telegram::locator(chat_id, message_id),
                );
            }
        }
        media_content => {
            log::info!(
                "[bridge-outbound] sending media {:?} to {}",
                std::mem::discriminant(media_content),
                chat_id
            );
            if let Err(error) =
                send_media(client, api_base, token, chat_id, None, media_content).await
            {
                log::error!("[bridge-outbound] media send failed: {}", error);
            }
        }
    }
}

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

pub async fn send_notification(
    tab: &Arc<dyn BridgeTabPort>,
    chat_id: i64,
    message: &OutboundMessage,
) -> bool {
    let Some((tab_id, text)) = notification_text(message) else {
        return false;
    };
    let threads = tab.threads();
    if !threads.is_attached() {
        return false;
    }
    let thread_id = threads
        .notification_thread_id(tab.thread_tabs(), tab_id, chat_id)
        .await;
    let placed = threads
        .send_notification_text(
            tab.thread_tabs(),
            TabText {
                tab_id: tab_id.to_string(),
                chat_id,
                markdown: text.to_string(),
                append: message.append,
                buttons: message.buttons.clone(),
            },
            thread_id,
        )
        .await;
    let Some(message_id) = placed else {
        return false;
    };
    if let Some(receipt) = message.receipt.as_ref() {
        tab.record_channel_placement(
            receipt,
            &crate::bridge::telegram::locator(chat_id, message_id),
        );
    }
    true
}

fn notification_text(message: &OutboundMessage) -> Option<(&str, &str)> {
    match (&message.route, message.edit.as_ref(), &message.content) {
        (
            super::OutboundRoute::TabNotification { tab_id },
            None,
            OutboundContent::Text { text },
        ) => Some((tab_id.as_str(), text.as_str())),
        _ => None,
    }
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
