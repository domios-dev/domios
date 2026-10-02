use super::*;
use crate::api::mem_routes::decision::SupporterRegistry;
use crate::plan::Plan;
use crate::service::{supporter, ServiceContext};
use codeterm_types::config::SupporterConfig;
use std::sync::Arc;

pub(crate) fn decide(
    svc: &ServiceContext,
    plan: &Plan,
    tab: &str,
    now: u64,
) -> Result<(Snapshot, Decision), String> {
    let key = format!("{}:{tab}", plan.id);
    let previous = svc.heartbeat_gate.observation(&key);
    let (snapshot, observed) = collect(svc, plan, tab, &previous, now);
    svc.heartbeat_gate.observed(&key, observed);
    let config = crate::config::load_config();
    let cfg = config.supporter;
    let runtime = supporter::runtime_with_notifications(svc);
    let token = supporter::TokenSnapshot::read(
        &crate::api::mem_routes::decision::token_store(),
        cfg.active_id().unwrap_or(""),
    );
    let model = connected_model(&cfg, &svc.supporter_models, &token, runtime.clone(), tab);
    let deadline =
        Instant::now() + Duration::from_millis(cfg.deadline_ms.max(1)).min(Duration::from_secs(5));
    let decision = svc.heartbeat_gate.decide_with_interval(
        &key,
        &snapshot,
        now,
        model.as_deref(),
        deadline,
        config.orchestration.nudge_interval_secs,
    )?;
    let event = decision.observation_event(&snapshot, tab, &plan.id, now);
    if let Err(error) = runtime.heartbeat_event(event) {
        log::warn!("[heartbeat-gate] journal unavailable: {error}");
    }
    Ok((snapshot, decision))
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
                None,
                None,
            )
        })
}

#[cfg(test)]
mod tests;
