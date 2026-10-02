use super::*;
use crate::plan::Plan;
use crate::service::{supporter, ServiceContext};

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
        &cfg.adapter,
    );
    let model = supporter::supporter_connected(&cfg, &svc.supporter_models, &token)
        .then(|| svc.supporter_models.get(&cfg.adapter))
        .flatten()
        .filter(|model| model.primitives().choice)
        .map(|model| {
            supporter::budgeted_model(
                model,
                runtime.clone(),
                cfg.calls_per_hour.unwrap_or(0),
                supporter::VerdictKind::Heartbeat,
                Some(tab.into()),
                None,
            )
        });
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
