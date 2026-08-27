//! The Discord push channel.
//!
//! Becoming routable took a roster row and this module — no core edit, no new
//! config section, no column added by hand. This build ships no Discord
//! transport yet, which is a readiness answer rather than an absence: an
//! opted-in level says why it could not be carried instead of going quiet.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_types::config::schema::AppConfig;

use super::{
    stored_audience, Audience, ChannelHost, ChannelReadiness, PushBounds, PushChannel, PushMessage,
};
use crate::notification::push;

const SERVICE: &str = "discord";

/// Discord's own ceiling for one message, in characters — a different number
/// from Telegram's, and the reason the bound is not one constant in core.
const MESSAGE_CEILING: usize = 2000;

pub static CHANNEL: DiscordChannel = DiscordChannel;

pub struct DiscordChannel;

/// The audience this channel's own surface stored, read exactly as every
/// channel's is. `integrations.discord_allowed_users` admits inbound senders
/// and is deliberately not read here, so an unchosen audience is nobody.
fn audience(cfg: &AppConfig) -> Audience {
    Audience(push::resolve_audience(&stored_audience(cfg, SERVICE)))
}

impl PushChannel for DiscordChannel {
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
                "{SERVICE} is opted in with {} recipient(s) but this build carries no {SERVICE} transport",
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
        host.service_ready(SERVICE) && !cfg.integrations.discord_allowed_users.is_empty()
    }

    fn deliver(
        &self,
        host: &dyn ChannelHost,
        audience: &Audience,
        message: &PushMessage,
    ) -> Result<(), String> {
        if audience.is_empty() {
            return Err(format!("{SERVICE}: no explicit recipients configured"));
        }
        // The one hop this build does not have. Handing the message to the
        // service roster's queue is what a transport would make succeed; until
        // then the refusal is the honest answer, not a swallowed send.
        host.enqueue(
            SERVICE,
            OutboundMessage {
                user_id: None,
                content: codeterm_bridge::bridge::traits::OutboundContent::Text {
                    text: message.text.clone(),
                },
                append: false,
                buttons: None,
                edit: None,
                receipt: None,
            },
        )
    }
}
