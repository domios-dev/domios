use super::*;
use crate::service::supporter::registry::SupporterRegistry;
use crate::service::{supporter, ServiceContext};
use codeterm_types::config::SupporterConfig;
use std::sync::Arc;

/// The active Supporter connection as the existing config and token rules see
/// it; it never selects another adapter or model.
pub(crate) struct Supporter<'a> {
    /// Connection epoch read before `cfg` and `token`, so a later mutation is never missed.
    pub(crate) epoch: u64,
    pub(crate) cfg: SupporterConfig,
    pub(crate) token: supporter::TokenSnapshot,
    pub(crate) registry: &'a SupporterRegistry,
    pub(crate) runtime: Arc<supporter::SupporterRuntime>,
}

impl<'a> Supporter<'a> {
    pub(crate) fn live(svc: &'a ServiceContext) -> Self {
        let runtime = supporter::runtime_with_notifications(svc);
        let epoch = runtime.connection_epoch();
        let cfg = svc.configuration().supporter;
        let token = supporter::read_token_snapshot(
            &runtime,
            &crate::service::supporter::registry::token_store(),
            cfg.active_id().unwrap_or(""),
        );
        Self {
            epoch,
            cfg,
            token,
            registry: &svc.supporter_models,
            runtime,
        }
    }

    pub(crate) fn connected(&self, tab: &str) -> Option<Arc<dyn DecisionModel>> {
        connected_model(
            &self.cfg,
            self.registry,
            &self.token,
            self.runtime.clone(),
            tab,
        )
    }

    /// Connection fingerprint, not a monotonic generation: adapter id, selected
    /// model, token usability and the registered instance; None when disconnected.
    pub(crate) fn generation(&self) -> Option<String> {
        if !supporter::supporter_connected(&self.cfg, self.registry, &self.token) {
            return None;
        }
        let model = supporter::configured_model(&self.cfg, self.registry)
            .filter(|model| model.primitives().choice)?;
        // An in-flight decision holds this instance, so its address cannot be reused meanwhile.
        let instance = Arc::as_ptr(&model) as *const () as usize;
        let identity = json!({ "adapter": model.id(), "model": model.model_id(),
            "token": self.token.usable, "instance": instance });
        Some(supporter::criteria_hash(identity.to_string().as_bytes()))
    }

    pub(crate) fn deadline(&self) -> Instant {
        Instant::now()
            + Duration::from_millis(self.cfg.deadline_ms.max(1)).min(Duration::from_secs(5))
    }
}

pub(crate) fn journal(
    runtime: &supporter::SupporterRuntime,
    decision: &Decision,
    plan: &str,
    now: u64,
) {
    let event = decision.journal_event(&decision.scope.tab_id, plan, now);
    if let Err(error) = runtime.heartbeat_event(event) {
        log::warn!("[heartbeat-gate] journal unavailable: {error}");
    }
}

fn connected_model(
    cfg: &SupporterConfig,
    registry: &SupporterRegistry,
    token: &supporter::TokenSnapshot,
    runtime: Arc<supporter::SupporterRuntime>,
    tab: &str,
) -> Option<Arc<dyn DecisionModel>> {
    if !supporter::supporter_connected(cfg, registry, token) {
        return None;
    }
    supporter::configured_model(cfg, registry)
        .filter(|model| model.primitives().choice)
        .map(|model| {
            supporter::budgeted_model_with_activity(
                model,
                runtime,
                cfg.calls_per_hour.unwrap_or(0),
                supporter::VerdictKind::Heartbeat,
                Some(tab.into()),
                Some(criteria_version()),
                None,
            )
        })
}

#[cfg(test)]
mod tests;
