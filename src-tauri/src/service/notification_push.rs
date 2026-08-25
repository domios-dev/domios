//! Where a levelled notification is routed, and the daemon-side policy that
//! decides it.
//!
//! The push side is a registry: the grid's channel columns, the routing, and
//! the delivery all resolve a channel by service id. Nothing here names one.

use super::notification::{PushEnvelope, PushIntent};
use super::ServiceContext;
use crate::bridge::manager::OutboundMessage;
use crate::notification::authority::SurfaceTarget;
use crate::notification::channels::{
    self, Audience, ChannelHost, ChannelReadiness, PushChannel, RegisteredChannel,
};
use codeterm_types::config::schema::AppConfig;
use codeterm_types::notification::{NotifyDestination, NotifyLevel};

/// One channel this notification is actually going to, with the audience that
/// channel resolved for it.
pub(crate) struct PushTarget {
    pub service: &'static str,
    pub channel: &'static dyn PushChannel,
    pub audience: Audience,
}

/// A channel the level was routed to that cannot carry it right now. Kept
/// rather than dropped: a refusal is reported, never absorbed into silence.
pub(crate) struct PushRefusal {
    pub service: String,
    pub reason: String,
}

/// The daemon's whole answer for one notification's push leg.
pub(crate) struct PushPlan {
    pub intent: PushIntent,
    pub per_scope_limit: u32,
    pub targets: Vec<PushTarget>,
    pub refusals: Vec<PushRefusal>,
}

impl PushPlan {
    /// Nothing was routed anywhere. Byte-identical to the path an install with
    /// no opt-in has always taken.
    fn inert() -> Self {
        Self {
            intent: PushIntent::default(),
            per_scope_limit: 0,
            targets: Vec::new(),
            refusals: Vec::new(),
        }
    }

    fn reaches_nothing(&self) -> bool {
        self.targets.is_empty() && self.refusals.is_empty()
    }
}

/// Resolve a level's routing against the registry. Pure: it reads config and
/// the roster, and asks each channel about itself.
///
/// The level's column for a channel is the whole of whether: closed here and
/// the channel is never asked. What a channel then answers is who — an
/// audience nobody chose is nobody, never that integration's admission list.
pub(crate) fn plan_push(
    cfg: &AppConfig,
    roster: &[RegisteredChannel],
    host: &dyn ChannelHost,
    intent: PushIntent,
) -> PushPlan {
    // A caller that named no level never reached a channel and still does not.
    let Some(level) = intent.level else {
        return PushPlan::inert();
    };
    // A level with no readable row is refused rather than widened into a pass.
    let Some(row) = cfg
        .notifications
        .agent_levels
        .iter()
        .find(|row| NotifyLevel::parse(&row.level) == Some(level))
    else {
        return PushPlan::inert();
    };

    let mut targets = Vec::new();
    let mut refusals = Vec::new();
    for registered in roster {
        if !row.allows(NotifyDestination::Channel(registered.id())) {
            continue;
        }
        match registered.channel.readiness(cfg, host) {
            // The user never opted in here: nothing was routed, so nothing is
            // lost and there is nothing to report.
            ChannelReadiness::OptedOut => {}
            ChannelReadiness::Unavailable(reason) => refusals.push(PushRefusal {
                service: registered.id().to_string(),
                reason,
            }),
            ChannelReadiness::Ready(audience) => targets.push(PushTarget {
                service: registered.id(),
                channel: registered.channel,
                audience,
            }),
        }
    }

    // A stored column naming a channel this build cannot resolve. The user
    // turned that column on deliberately, so it is reported rather than
    // quietly skipped into nothing.
    for (service, _) in row.channels.iter().filter(|(_, routed)| **routed) {
        if let Err(reason) = channels::resolve_in(roster, service) {
            refusals.push(PushRefusal {
                service: service.clone(),
                reason,
            });
        }
    }

    let per_scope_limit = targets
        .iter()
        .map(|target| target.channel.per_scope_limit(cfg))
        .max()
        .unwrap_or(0);

    let plan = PushPlan {
        intent,
        per_scope_limit,
        targets,
        refusals,
    };
    if plan.reaches_nothing() {
        return PushPlan::inert();
    }
    plan
}

/// Fan one rendered push to every resolved target. A refusal recorded at plan
/// time counts as a failure here, so a level routed to a channel that cannot
/// carry it degrades into a reported failure rather than into nothing.
pub(crate) fn deliver_plan(
    targets: &[PushTarget],
    refusals: &[PushRefusal],
    host: &dyn ChannelHost,
    envelope: &PushEnvelope,
) -> Result<(), String> {
    let mut failures: Vec<String> = refusals
        .iter()
        .map(|refusal| format!("{}: {}", refusal.service, refusal.reason))
        .collect();

    for target in targets {
        if let Err(reason) = target
            .channel
            .deliver(host, &target.audience, &envelope.message)
        {
            failures.push(format!("{}: {reason}", target.service));
        }
    }

    if failures.is_empty() {
        return Ok(());
    }
    Err(failures.join("; "))
}

/// One channel's standing in the settings surface.
pub(crate) struct ChannelStanding {
    pub id: &'static str,
    pub label: &'static str,
    pub control_available: bool,
}

impl ChannelHost for ServiceContext {
    /// Only a service the roster says this build can send on, and only while
    /// the bridge is actually carrying traffic. A registered service with no
    /// transport answers honestly instead of looking live.
    fn service_ready(&self, service: &str) -> bool {
        codeterm_bridge::services::find(service).is_some_and(|descriptor| descriptor.outbound)
            && self.bridge_supervisor.is_active()
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
        if !self.bridge_supervisor.is_active() {
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
        let cfg = crate::config::load_config();
        plan_push(&cfg, &channels::roster(), self, intent)
    }

    /// Resolve the surface an offer names. `desktop` is core's own; anything
    /// else must be a channel the registry answers to, so an offer can never
    /// name a destination that resolves to nothing. The bell is refused: it is
    /// presentation, not a capability an agent could be granted.
    pub(crate) fn resolve_surface_target(&self, named: &str) -> Result<SurfaceTarget, String> {
        match NotifyDestination::parse_local(named) {
            Some(NotifyDestination::Desktop) => Ok(SurfaceTarget::Desktop),
            Some(surface) => Err(format!(
                "'{}' is not a surface an agent may offer to turn on",
                surface.as_str()
            )),
            None => {
                let roster = channels::roster();
                let registered = channels::resolve_in(&roster, named)?;
                Ok(SurfaceTarget::Channel {
                    service: registered.id().to_string(),
                })
            }
        }
    }

    /// What settings may offer per channel, in roster order.
    pub(crate) fn push_channel_standings(&self) -> Vec<ChannelStanding> {
        let cfg = crate::config::load_config();
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
mod tests;

#[cfg(test)]
mod delivery_tests;

#[cfg(test)]
mod channel_audience_tests;
