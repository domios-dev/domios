use super::*;
use crate::api::mem_routes::decision::SupporterRegistry;
use crate::service::{supporter, ServiceContext};
use codeterm_types::config::SupporterConfig;
use std::sync::Arc;

/// The active Supporter connection as the existing config and token rules see
/// it; it never selects another adapter or model.
pub(crate) struct Supporter<'a> {
    pub(crate) cfg: SupporterConfig,
    pub(crate) token: supporter::TokenSnapshot,
    pub(crate) registry: &'a SupporterRegistry,
    pub(crate) runtime: Arc<supporter::SupporterRuntime>,
}

impl<'a> Supporter<'a> {
    pub(crate) fn live(svc: &'a ServiceContext) -> Self {
        let cfg = crate::config::load_config().supporter;
        let token = supporter::TokenSnapshot::read(
            &crate::api::mem_routes::decision::token_store(),
            cfg.active_id().unwrap_or(""),
        );
        Self {
            cfg,
            token,
            registry: &svc.supporter_models,
            runtime: supporter::runtime_with_notifications(svc),
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

    /// Digest of the adapter, its selected model and token usability; None
    /// whenever no Choice-capable adapter is connected.
    pub(crate) fn generation(&self) -> Option<String> {
        if !supporter::supporter_connected(&self.cfg, self.registry, &self.token) {
            return None;
        }
        let model = supporter::configured_model(&self.cfg, self.registry)
            .filter(|model| model.primitives().choice)?;
        let identity = json!({ "adapter": model.id(), "model": model.model_id(),
            "token": self.token.usable });
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
