use crate::config::load_config;
use crate::plan::{Plan, TaskStatus};
use crate::service::plan::digest_rules::{evaluate_digest_rules, plan_digest_counters};
use crate::service::plan::usage_digest::{
    needs_refresh, usage_digest_lines, UsageDigestRow, FRESHNESS_MS,
};
use crate::service::ServiceContext;

pub fn prepend_orchestrator_heartbeat(
    svc: &ServiceContext,
    plan_id: &str,
    pane_id: Option<&str>,
    text: &str,
) -> String {
    let Some(plan) = svc.plan_store.get(plan_id) else {
        return text.to_string();
    };
    if crate::service::plan::heartbeat::is_plan_complete(&plan) {
        return text.to_string();
    }
    let digest_lines = collect_digest_lines(svc, &plan, pane_id);
    let injected =
        svc.plan_store
            .prepend_heartbeat_for_plan_with_digests(plan_id, text, &digest_lines);
    if let Some(pane_id) = pane_id {
        let heartbeat_bytes = injected.len().saturating_sub(text.len());
        if heartbeat_bytes > 0 {
            svc.record_kpi_event(
                pane_id,
                crate::agent::kpi_ledger::KpiEventKind::Heartbeat,
                heartbeat_bytes,
            );
        }
    }
    injected
}

fn collect_digest_lines(svc: &ServiceContext, plan: &Plan, pane_id: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(queue_pressure_lines(svc, plan, pane_id));
    out.extend(usage_lines(svc));
    out.truncate(6);
    out
}

fn queue_pressure_lines(svc: &ServiceContext, plan: &Plan, pane_id: Option<&str>) -> Vec<String> {
    let cfg = load_config().orchestration.digests;
    let now_ms = now_epoch_ms();
    let stalled = count_stalled_lanes(plan, now_ms);
    let counters = plan_digest_counters(plan, now_ms, stalled);
    let fired = evaluate_digest_rules(&cfg, &counters);
    let Some(pane_id) = pane_id.filter(|id| !id.trim().is_empty()) else {
        return fired.into_iter().map(|(_, line)| line).collect();
    };
    fired
        .into_iter()
        .filter_map(|(rule, line)| {
            if svc.plan_turn_counter.digest_rule_due(
                pane_id,
                &rule.id,
                now_epoch_secs(),
                rule.rate_limit_secs,
            ) {
                svc.plan_turn_counter
                    .mark_digest_rule_fired(pane_id, &rule.id, now_epoch_secs());
                Some(line)
            } else {
                None
            }
        })
        .collect()
}

fn usage_lines(svc: &ServiceContext) -> Vec<String> {
    let cfg = load_config().usage;
    if !cfg.enabled || !cfg.heartbeat {
        return Vec::new();
    }
    let now_ms = now_epoch_ms() as i64;
    let freshness_ms = (cfg.freshness_secs.saturating_mul(1000) as i64).max(FRESHNESS_MS);
    let rows = usage_rows(svc, now_ms, freshness_ms);
    let (lines, guidance) = usage_digest_lines(&cfg, now_ms, &rows);
    let mut out = lines;
    if let Some(hint) = guidance {
        out.push(hint);
    }
    out
}

fn usage_rows(svc: &ServiceContext, now_ms: i64, freshness_ms: i64) -> Vec<UsageDigestRow> {
    let mut rows = Vec::new();
    for provider_id in svc.agent_registry.provider_ids() {
        let capable = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.usage_parser().is_some())
            .unwrap_or(false);
        if !capable || !crate::config::usage_tracking_enabled(&provider_id) {
            continue;
        }
        let latest = svc
            .usage_store
            .get_latest(&provider_id, None)
            .ok()
            .flatten();
        if needs_refresh(latest.as_ref(), now_ms, freshness_ms) {
            queue_usage_refresh(svc, &provider_id);
        }
        let label = svc
            .agent_registry
            .with_provider(&provider_id, |p| p.display_name().to_string())
            .unwrap_or(provider_id);
        rows.push(UsageDigestRow { label, row: latest });
    }
    rows
}

fn queue_usage_refresh(svc: &ServiceContext, provider_id: &str) {
    if let Some(poller) = svc.usage_poller.lock().clone() {
        poller.refresh_now(provider_id, None);
    }
}

fn count_stalled_lanes(plan: &Plan, now_ms: u64) -> usize {
    crate::plan::reconcile::aged_open_tasks(plan, now_ms)
        .into_iter()
        .filter(|task| task.status == TaskStatus::InProgress)
        .count()
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn now_epoch_secs() -> u64 {
    now_epoch_ms() / 1000
}
