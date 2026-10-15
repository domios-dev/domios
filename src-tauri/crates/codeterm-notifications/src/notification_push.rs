//! Where a levelled notification is routed, and the daemon-side policy that
//! decides it.
//!
//! The push side is a registry: the grid's channel columns, the routing, and
//! the delivery all resolve a channel by service id. Nothing here names one.

use crate::notification::channels::{
    self, Audience, ChannelHost, ChannelReadiness, PushChannel, RegisteredChannel,
};
use crate::notification_create::{PushEnvelope, PushIntent};
use codeterm_notification_core::contract::{NotifyDestination, NotifyLevel};
use codeterm_types::config::schema::AppConfig;

/// One channel this notification is actually going to, with the audience that
/// channel resolved for it.
pub struct PushTarget {
    pub service: &'static str,
    pub channel: &'static dyn PushChannel,
    pub audience: Audience,
}

/// A channel the level was routed to that cannot carry it right now. Kept
/// rather than dropped: a refusal is reported, never absorbed into silence.
pub struct PushRefusal {
    pub service: String,
    pub reason: String,
}

/// The daemon's whole answer for one notification's push leg.
pub struct PushPlan {
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
pub fn plan_push(
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

    // The user turned that column on deliberately, so it is reported rather than quietly skipped into nothing.
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
pub fn deliver_plan(
    targets: &[PushTarget],
    refusals: &[PushRefusal],
    host: &dyn ChannelHost,
    envelope: &PushEnvelope,
) -> Result<(), String> {
    let mut failures: Vec<String> = refusals
        .iter()
        .map(|refusal| format!("{}: {}", refusal.service, refusal.reason))
        .collect();

    // Rendered per target: the length bound belongs to the transport carrying
    // the message, so one channel's ceiling never cuts another channel's copy.
    for target in targets {
        let message = envelope.message_for(target.channel.bounds());
        if let Err(reason) = target.channel.deliver(host, &target.audience, &message) {
            failures.push(format!("{}: {reason}", target.service));
        }
    }

    if failures.is_empty() {
        return Ok(());
    }
    Err(failures.join("; "))
}

/// One channel's standing in the settings surface.
pub struct ChannelStanding {
    pub id: &'static str,
    pub label: &'static str,
    pub control_available: bool,
}

#[cfg(test)]
mod channel_audience_tests;
#[cfg(test)]
mod tests;

pub fn resolve_surface_target(
    named: &str,
) -> Result<crate::notification::authority::SurfaceTarget, String> {
    use crate::notification::authority::SurfaceTarget;
    use codeterm_notification_core::contract::NotifyDestination;
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
