//! The Telegram push channel.
//!
//! Everything Telegram-specific about pushing a notification lives here: which
//! config entry holds its audience, what makes its control offerable, and how a
//! rendered push becomes an outbound bridge message.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::bridge::traits::{InlineButton, InlineKeyboard, OutboundContent};
use codeterm_types::config::schema::{AppConfig, PushChannelConfig};

use super::{Audience, ChannelHost, ChannelReadiness, PushChannel, PushMessage};
use crate::notification::push;

/// The bridge service this channel rides.
const SERVICE: &str = "telegram";

pub static CHANNEL: TelegramChannel = TelegramChannel;

pub struct TelegramChannel;

/// This channel's own audience entry. Absent is the pre-opt-in state.
fn audience_config(cfg: &AppConfig) -> PushChannelConfig {
    cfg.notifications
        .channels
        .get(SERVICE)
        .cloned()
        .unwrap_or_default()
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
        let audience = push::resolve_audience(&audience_config(cfg));
        if audience.is_empty() {
            return ChannelReadiness::OptedOut;
        }
        if !host.service_ready(SERVICE) {
            return ChannelReadiness::Unavailable(format!(
                "{SERVICE} is opted in with {} recipient(s) but its bridge is not carrying traffic",
                audience.len()
            ));
        }
        ChannelReadiness::Ready(Audience(
            audience.into_iter().map(|id| id.to_string()).collect(),
        ))
    }

    fn per_scope_limit(&self, cfg: &AppConfig) -> u32 {
        audience_config(cfg).per_scope_limit
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
}
