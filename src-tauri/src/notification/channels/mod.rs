//! The push-channel registry.
//!
//! A push channel is a bridge service that can carry a notification, so a
//! channel is addressed by the very same service id `codeterm bridge send
//! --service <id>` takes and `codeterm_bridge::services` publishes. Nothing
//! above this module names a channel: routing asks the registry to resolve an
//! id, and the id itself came from config or from the roster.
//!
//! Adding a channel is a row in `codeterm_bridge::services::SERVICES` plus one
//! module here, registered in `channels()`. No enum, no grid column, and no
//! match arm anywhere else has to learn about it.

pub mod discord;
pub mod telegram;

use codeterm_bridge::bridge::manager::OutboundMessage;
use codeterm_bridge::services::ServiceDescriptor;
use codeterm_types::config::schema::{AppConfig, PushChannelConfig};

/// One channel's stored audience entry, keyed by its service id. Absent is an
/// audience nobody has chosen yet, which resolves to nobody rather than to that
/// service's inbound admission list.
pub fn stored_audience(cfg: &AppConfig, service: &str) -> PushChannelConfig {
    cfg.notifications
        .channels
        .get(service)
        .cloned()
        .unwrap_or_default()
}

pub use super::channel_ask::PushMessage;

/// The recipients a channel resolved from config, in that channel's own terms.
/// Opaque to everything above: only the channel knows what a token means.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Audience(pub Vec<String>);

impl Audience {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many recipients this channel resolved. Channels report it in the
    /// reason they hand back when they cannot carry a message.
    pub fn len(&self) -> usize {
        self.0.len()
    }
}

/// What a channel says about itself for one notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelReadiness {
    /// Opted in, with an audience, and the transport can carry it.
    Ready(Audience),
    /// The user never opted this channel in. Not a failure and not a drop:
    /// nothing was ever routed here to lose.
    OptedOut,
    /// Opted in, but this message cannot be carried. Always carries the
    /// reason, so a channel that reaches nobody says so instead of going quiet.
    Unavailable(String),
}

/// What a channel needs from the daemon to know it can send, and to send.
/// Deliberately narrow: a channel may not reach into service state at will.
pub trait ChannelHost: Send + Sync {
    /// Whether the named bridge service can carry a message right now.
    fn service_ready(&self, service: &str) -> bool;
    /// Hand one message to the named service's outbound queue.
    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String>;
}

pub trait PushChannel: Send + Sync {
    /// The bridge service this channel rides.
    fn service(&self) -> &'static str;
    fn readiness(&self, cfg: &AppConfig, host: &dyn ChannelHost) -> ChannelReadiness;
    /// Lifetime push ceiling per originating tab, as this channel configures it.
    fn per_scope_limit(&self, cfg: &AppConfig) -> u32;
    /// Whether settings may offer this channel's own controls at all.
    fn control_available(&self, cfg: &AppConfig, host: &dyn ChannelHost) -> bool;
    /// Carry one rendered notification. The message holds the text every
    /// channel has always been handed plus the offered options; a channel that
    /// cannot draw options carries the text and leaves them alone.
    fn deliver(
        &self,
        host: &dyn ChannelHost,
        audience: &Audience,
        message: &PushMessage,
    ) -> Result<(), String>;

    /// Rewrite a message this channel placed, so a question answered once
    /// stops presenting live controls here. The locator is this channel's own,
    /// handed back exactly as it was reported.
    ///
    /// Presentation only. The answer is already recorded and already owed to
    /// the tab that asked, so a refusal here is reported and never propagated
    /// into the answer. Default: this channel cannot rewrite what it sent, said
    /// plainly rather than reported as a rewrite that happened.
    fn spend(
        &self,
        _host: &dyn ChannelHost,
        _locator: &str,
        _message: &PushMessage,
    ) -> Result<(), String> {
        Err(format!(
            "{} cannot rewrite a message it has already sent",
            self.service()
        ))
    }
}

/// A channel paired with the bridge service it rides. Ids and user-facing
/// labels have exactly one home — the service roster — so a column cannot be
/// captioned one way here and another way in the send path.
pub struct RegisteredChannel {
    pub channel: &'static dyn PushChannel,
    pub service: &'static ServiceDescriptor,
}

impl RegisteredChannel {
    pub fn id(&self) -> &'static str {
        self.service.id
    }

    pub fn label(&self) -> &'static str {
        self.service.label
    }
}

/// Every channel this build ships. The one list to edit when a channel lands.
fn channels() -> Vec<&'static dyn PushChannel> {
    vec![
        &telegram::CHANNEL as &dyn PushChannel,
        &discord::CHANNEL as &dyn PushChannel,
    ]
}

/// The registry, resolved against the service roster. A channel riding a
/// service the bridge does not know is a misconfiguration, not a column: it is
/// logged loudly and left out rather than rendered as a destination that
/// resolves to nothing.
pub fn roster() -> Vec<RegisteredChannel> {
    channels()
        .into_iter()
        .filter_map(|channel| match codeterm_bridge::services::find(channel.service()) {
            Some(service) => Some(RegisteredChannel { channel, service }),
            None => {
                log::error!(
                    "[notify-channels] channel rides unknown bridge service '{}'; not routable",
                    channel.service()
                );
                None
            }
        })
        .collect()
}

/// Resolve one channel by service id against a roster. An id nobody registered
/// is an error with a reason, never a silent no-op that would route a level
/// into nothing.
pub fn resolve_in<'r>(
    roster: &'r [RegisteredChannel],
    service: &str,
) -> Result<&'r RegisteredChannel, String> {
    let wanted = service.trim();
    roster
        .iter()
        .find(|registered| registered.id() == wanted)
        .ok_or_else(|| format!("no registered push channel answers to '{service}'"))
}
