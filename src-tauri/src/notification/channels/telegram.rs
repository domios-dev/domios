//! The Telegram push channel.
//!
//! Everything Telegram-specific about pushing a notification lives here: which
//! config entry holds its audience, what makes its control offerable, and how a
//! rendered push becomes an outbound bridge message.

use codeterm_bridge::bridge::manager::{OutboundEdit, OutboundMessage, PlacementReceipt};
use codeterm_bridge::bridge::telegram as transport;
use codeterm_bridge::bridge::traits::{InlineButton, InlineKeyboard, OutboundContent};
use codeterm_types::config::schema::AppConfig;

use super::{
    stored_audience, Audience, ChannelHost, ChannelReadiness, PushBounds, PushChannel, PushMessage,
};
use crate::notification::push;

/// The bridge service this channel rides.
const SERVICE: &str = "telegram";

/// Telegram's own ceiling for one text message, in characters. It is the
/// transport's number, so it lives with the transport.
const MESSAGE_CEILING: usize = 4096;

pub static CHANNEL: TelegramChannel = TelegramChannel;

pub struct TelegramChannel;

/// The chosen audience, narrowed to what Telegram can address. An entry that is
/// not a chat id is dropped rather than broadcast to, and nothing here reads
/// `integrations.telegram_allowed_users`: that list admits inbound senders.
fn audience(cfg: &AppConfig) -> Audience {
    Audience(
        push::resolve_audience(&stored_audience(cfg, SERVICE))
            .into_iter()
            .filter(|entry| entry.parse::<i64>().is_ok())
            .collect(),
    )
}

/// Chat ids, dropping anything that is not one. An entry that is not a chat id
/// is not broadcast to.
fn chat_ids(audience: &Audience) -> Vec<i64> {
    audience
        .0
        .iter()
        .filter_map(|candidate| candidate.trim().parse::<i64>().ok())
        .collect()
}

/// One offered option per row, carrying its opaque token. The label is drawn
/// and the token is what comes back, so nothing readable decides the answer.
fn keyboard(message: &PushMessage) -> Option<InlineKeyboard> {
    if !message.offers_choices() {
        return None;
    }
    Some(
        message
            .choices
            .iter()
            .map(|choice| {
                vec![InlineButton {
                    text: choice.label.clone(),
                    data: Some(choice.token.clone()),
                    url: None,
                }]
            })
            .collect(),
    )
}

impl PushChannel for TelegramChannel {
    fn service(&self) -> &'static str {
        SERVICE
    }

    fn readiness(&self, cfg: &AppConfig, host: &dyn ChannelHost) -> ChannelReadiness {
        let audience = audience(cfg);
        if audience.is_empty() {
            return ChannelReadiness::OptedOut;
        }
        if !host.service_ready(SERVICE) {
            return ChannelReadiness::Unavailable(format!(
                "{SERVICE} is opted in with {} recipient(s) but its bridge is not carrying traffic",
                audience.len()
            ));
        }
        ChannelReadiness::Ready(audience)
    }

    fn per_scope_limit(&self, cfg: &AppConfig) -> u32 {
        stored_audience(cfg, SERVICE).per_scope_limit
    }

    fn bounds(&self) -> PushBounds {
        PushBounds::for_ceiling(MESSAGE_CEILING)
    }

    fn control_available(&self, cfg: &AppConfig, host: &dyn ChannelHost) -> bool {
        push::control_available(
            host.service_ready(SERVICE),
            cfg.integrations.telegram_allowed_users.len(),
        )
    }

    /// Best-effort fan to the explicit audience. Every send names its chat id:
    /// `user_id: None` would fall back to the bridge's last speaker.
    fn deliver(
        &self,
        host: &dyn ChannelHost,
        audience: &Audience,
        message: &PushMessage,
    ) -> Result<(), String> {
        let chat_ids = chat_ids(audience);
        if chat_ids.is_empty() {
            return Err(format!("{SERVICE}: no explicit recipients configured"));
        }
        let buttons = keyboard(message);
        // Only a message that draws options is worth finding again: a push with
        // nothing to tap has nothing that could go stale.
        let receipt = message.offers_choices().then(|| PlacementReceipt {
            service: SERVICE.to_string(),
            tag: message.record.clone(),
        });
        let rejected = chat_ids
            .iter()
            .filter(|chat_id| {
                host.enqueue(
                    SERVICE,
                    OutboundMessage {
                        user_id: Some(**chat_id),
                        content: OutboundContent::Text {
                            text: message.text.clone(),
                        },
                        append: false,
                        buttons: buttons.clone(),
                        edit: None,
                        receipt: receipt.clone(),
                    },
                )
                .is_err()
            })
            .count();

        if rejected > 0 {
            return Err(format!(
                "{rejected}/{} {SERVICE} sends rejected",
                chat_ids.len()
            ));
        }
        Ok(())
    }

    /// Rewrite the placed message in place. The keyboard is always restated:
    /// a spent question draws no options, and an omitted markup would leave
    /// the ones it drew before.
    fn spend(
        &self,
        host: &dyn ChannelHost,
        locator: &str,
        message: &PushMessage,
    ) -> Result<(), String> {
        let Some((chat_id, message_id)) = transport::parse_locator(locator) else {
            return Err(format!("{SERVICE}: '{locator}' is not a {SERVICE} message"));
        };
        host.enqueue(
            SERVICE,
            OutboundMessage {
                user_id: Some(chat_id),
                content: OutboundContent::Text {
                    text: message.text.clone(),
                },
                append: false,
                buttons: Some(keyboard(message).unwrap_or_default()),
                edit: Some(OutboundEdit { message_id }),
                receipt: None,
            },
        )
    }
}
