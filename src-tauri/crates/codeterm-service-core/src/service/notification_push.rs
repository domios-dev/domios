//! Where a levelled notification is routed, and the daemon-side policy that
//! decides it.
//!
//! The push side is a registry: the grid's channel columns, the routing, and
//! the delivery all resolve a channel by service id. Nothing here names one.

use super::notification::PushIntent;
use super::ServiceContext;
use crate::bridge::manager::OutboundMessage;
use crate::notification::authority::SurfaceTarget;
use crate::notification::channels::{self, ChannelHost};

pub use codeterm_notifications::notification_push::{
    deliver_plan, plan_push, ChannelStanding, PushPlan,
};
impl ChannelHost for ServiceContext {
    /// Only a service the roster says this build can send on, and only while
    /// the bridge is actually carrying traffic. A registered service with no
    /// transport answers honestly instead of looking live.
    fn service_ready(&self, service: &str) -> bool {
        codeterm_bridge::services::find(service).is_some_and(|descriptor| descriptor.outbound)
            && self
                .ext::<std::sync::Arc<crate::bridge::supervisor::BridgeSupervisor>>()
                .is_active()
    }

    fn enqueue(&self, service: &str, message: OutboundMessage) -> Result<(), String> {
        let Some(descriptor) = codeterm_bridge::services::find(service) else {
            return Err(format!("unknown service '{service}'"));
        };
        if !descriptor.outbound {
            return Err(format!(
                "this build carries no outbound transport for {service}"
            ));
        }
        if !self
            .ext::<std::sync::Arc<crate::bridge::supervisor::BridgeSupervisor>>()
            .is_active()
        {
            return Err(format!("the {service} bridge is not active"));
        }
        let outbound = self.bridge_outbound.lock();
        let Some(tx) = outbound.as_ref() else {
            return Err(format!("the {service} outbound channel is closed"));
        };
        tx.send(message)
            .map_err(|_| format!("the {service} outbound channel rejected the message"))
    }
}

impl ServiceContext {
    /// Resolve the opt-in for one notification. Without a routed, reachable
    /// channel the category is dropped, so an install that never opted in takes
    /// exactly the pre-existing code path.
    pub(crate) fn resolve_push_plan(&self, intent: PushIntent) -> PushPlan {
        let cfg = self.configuration();
        plan_push(&cfg, &channels::roster(), self, intent)
    }

    /// Resolve the surface an offer names. `desktop` is core's own; anything
    /// else must be a channel the registry answers to, so an offer can never
    /// name a destination that resolves to nothing. The bell is refused: it is
    /// presentation, not a capability an agent could be granted.
    pub(crate) fn resolve_surface_target(&self, named: &str) -> Result<SurfaceTarget, String> {
        codeterm_notifications::notification_push::resolve_surface_target(named)
    }

    /// What settings may offer per channel, in roster order.
    pub fn push_channel_standings(&self) -> Vec<ChannelStanding> {
        let cfg = self.configuration();
        channels::roster()
            .into_iter()
            .map(|registered| ChannelStanding {
                id: registered.id(),
                label: registered.label(),
                control_available: registered.channel.control_available(&cfg, self),
            })
            .collect()
    }
}

#[cfg(test)]
mod delivery_tests;

#[cfg(test)]
use super::notification::PushEnvelope;
#[cfg(test)]
use codeterm_notification_core::contract::NotifyLevel;
