//! The Discord push channel.
//!
//! Its audience is the Discord integration the config already carries, so
//! becoming routable took a roster row and this module — no core edit, no new
//! config section, no column added by hand. This build ships no Discord
//! transport yet, which is a readiness answer rather than an absence: an
//! opted-in level says why it could not be carried instead of going quiet.

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_types::config::schema::AppConfig;

use super::{Audience, ChannelHost, ChannelReadiness, PushChannel};

const SERVICE: &str = "discord";

pub static CHANNEL: DiscordChannel = DiscordChannel;

pub struct DiscordChannel;

/// The audience the integration config already names. An entry that is not an
/// id is dropped rather than broadcast to, exactly as every audience is read.
fn audience(cfg: &AppConfig) -> Audience {
    if !cfg.integrations.discord_enabled {
        return Audience::default();
    }
    Audience(
        cfg.integrations
            .discord_allowed_users
            .iter()
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .collect(),
    )
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
        cfg.notifications
            .channels
            .get(SERVICE)
            .map(|entry| entry.per_scope_limit)
            .unwrap_or_else(|| {
                codeterm_types::config::schema::PushChannelConfig::default().per_scope_limit
            })
    }

    fn control_available(&self, cfg: &AppConfig, host: &dyn ChannelHost) -> bool {
        host.service_ready(SERVICE) && !cfg.integrations.discord_allowed_users.is_empty()
    }

    fn deliver(
        &self,
        host: &dyn ChannelHost,
        audience: &Audience,
        text: &str,
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
                    text: text.to_string(),
                },
                append: false,
                buttons: None,
            },
        )
    }
}
