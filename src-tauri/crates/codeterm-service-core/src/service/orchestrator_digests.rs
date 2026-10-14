mod inventory;

use crate::plan::{Plan, TaskStatus};
use crate::service::plan::digest_rules::{evaluate_digest_rules, plan_digest_counters};
use crate::service::plan::usage_digest::{
    needs_refresh, usage_digest_lines, UsageDigestRow, FRESHNESS_MS,
};
use crate::service::ServiceContext;

#[derive(Default)]
struct HeartbeatSelection<'a> {
    learning_ids: Option<&'a [String]>,
    plain: bool,
    mode_transition: bool,
}

pub fn deliver_orchestrator_notice_exact(
    svc: &ServiceContext,
    plan_id: &str,
    requested_target: &str,
    context: &str,
    text: &str,
) -> Result<crate::service::messenger::Delivery, String> {
    deliver_notice_with_learning_selection(
        svc,
        plan_id,
        requested_target,
        context,
        text,
        HeartbeatSelection::default(),
        None,
    )
}

pub(crate) fn deliver_gated_heartbeat_exact(
    svc: &ServiceContext,
    plan_id: &str,
    target: &str,
    text: &str,
    learning_ids: Option<&[String]>,
    plain: bool,
    mode_transition: bool,
    observer: Option<crate::service::messenger::DeliveryWriteObserver>,
) -> Result<crate::service::messenger::Delivery, String> {
    deliver_notice_with_learning_selection(
        svc,
        plan_id,
        target,
        "system",
        text,
        HeartbeatSelection {
            learning_ids,
            plain,
            mode_transition,
        },
        observer,
    )
}

fn deliver_notice_with_learning_selection(
    svc: &ServiceContext,
    plan_id: &str,
    requested_target: &str,
    context: &str,
    text: &str,
    selection: HeartbeatSelection<'_>,
    observer: Option<crate::service::messenger::DeliveryWriteObserver>,
) -> Result<crate::service::messenger::Delivery, String> {
    let plan = svc.plan_store.get(plan_id).ok_or("plan not found")?;
    if crate::service::plan::pause_operation::plan_notices_suppressed(&plan)
        && !selection.mode_transition
    {
        return Err("plan_paused: plan notices are suppressed while the plan is paused".into());
    }
    let owner = plan
        .owner
        .tab_id
        .as_deref()
        .ok_or("plan owner has no local delivery target")?;
    deliver_exact_role_target("orchestrator", plan_id, owner, requested_target, |target| {
        let injected =
            if selection.plain || crate::service::plan::heartbeat::is_plan_complete(&plan) {
                text.to_string()
            } else {
                let digest_lines = collect_digest_lines(svc, &plan, Some(owner));
                svc.plan_store.prepend_heartbeat_with_selected_learnings(
                    plan_id,
                    text,
                    &digest_lines,
                    selection.learning_ids,
                )
            };
        let mut envelope = crate::service::messenger::MessageEnvelope::codeterm_reminder(context);
        envelope.delivery_observer = Some(observer.unwrap_or_else(|| {
            crate::service::messenger::DeliveryWriteObserver::PlanRole {
                plan_id: plan_id.to_string(),
                role: "orchestrator".into(),
            }
        }));
        let result = svc.gateway_deliver(
            target,
            envelope,
            &injected,
            crate::service::message_gateway::DeliveryClass::SystemNotice,
        );
        if result.as_ref().is_ok_and(|delivery| delivery.is_written()) {
            let heartbeat_bytes = injected.len().saturating_sub(text.len());
            if heartbeat_bytes > 0 {
                svc.record_kpi_event(
                    owner,
                    crate::agent::kpi_ledger::KpiEventKind::Heartbeat,
                    heartbeat_bytes,
                );
            }
        }
        result
    })
}

pub(crate) fn deliver_exact_role_target(
    role: &str,
    plan_id: &str,
    intended_target: &str,
    requested_target: &str,
    deliver: impl FnOnce(&str) -> Result<crate::service::messenger::Delivery, String>,
) -> Result<crate::service::messenger::Delivery, String> {
    if intended_target != requested_target {
        log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target=none outcome=refused requested_target={}",
            plan_id,
            role,
            intended_target,
            requested_target
        );
        return Err(format!(
            "{role} payload target mismatch: intended {intended_target}, requested {requested_target}"
        ));
    }
    let result = deliver(intended_target);
    match &result {
        Ok(crate::service::messenger::Delivery::Pending { delivery_id }) => log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=pending delivery_id={}",
            plan_id,
            role,
            intended_target,
            intended_target,
            delivery_id
        ),
        Ok(_) => log::info!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=written",
            plan_id,
            role,
            intended_target,
            intended_target
        ),
        Err(error) => log::warn!(
            "[plan-role-delivery] plan={} role={} intended_target={} actual_target={} outcome=failed error={}",
            plan_id,
            role,
            intended_target,
            intended_target,
            error
        ),
    }
    result
}

fn collect_digest_lines(svc: &ServiceContext, plan: &Plan, tab_id: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    out.extend(crate::service::cartographer::digest_lines(
        plan,
        svc.configuration().orchestration.map.enabled,
    ));
    out.extend(queue_pressure_lines(svc, plan, tab_id));
    out.extend(usage_lines(svc));
    out.truncate(8);
    out
}

fn queue_pressure_lines(svc: &ServiceContext, plan: &Plan, tab_id: Option<&str>) -> Vec<String> {
    let cfg = svc.configuration().orchestration.digests;
    let now_ms = now_epoch_ms();
    let stalled = count_stalled_lanes(plan, now_ms);
    let mut counters = plan_digest_counters(plan, now_ms, stalled);
    inventory::populate(svc, plan, &mut counters);
    let fired = evaluate_digest_rules(&cfg, &counters);
    let Some(tab_id) = tab_id.filter(|id| !id.trim().is_empty()) else {
        return fired.into_iter().map(|(_, line)| line).collect();
    };
    fired
        .into_iter()
        .filter_map(|(rule, line)| {
            if svc.plan_turn_counter.digest_rule_due(
                tab_id,
                &rule.id,
                now_epoch_secs(),
                rule.rate_limit_secs,
            ) {
                svc.plan_turn_counter
                    .mark_digest_rule_fired(tab_id, &rule.id, now_epoch_secs());
                Some(line)
            } else {
                None
            }
        })
        .collect()
}

fn usage_lines(svc: &ServiceContext) -> Vec<String> {
    let cfg = svc.configuration().usage;
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
        if !capable
            || !crate::config::usage_tracking_enabled_from(&svc.configuration(), &provider_id)
        {
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
    if let Some(poller) = svc
        .ext::<crate::usage::poller::UsagePollerSlot>()
        .0
        .lock()
        .clone()
    {
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

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::deliver_exact_role_target;

    #[test]
    fn owner_undeliverable_heartbeat_never_reaches_planner() {
        let attempts = RefCell::new(Vec::new());
        let result = deliver_exact_role_target(
            "orchestrator",
            "plan-one",
            "owner-tab",
            "owner-tab",
            |target| {
                attempts.borrow_mut().push(target.to_string());
                Err("owner undeliverable".to_string())
            },
        );

        assert!(result.is_err());
        assert_eq!(attempts.into_inner(), vec!["owner-tab"]);
    }

    #[test]
    fn owner_heartbeat_refuses_a_planner_target_before_delivery() {
        let attempts = RefCell::new(Vec::new());
        let result = deliver_exact_role_target(
            "orchestrator",
            "plan-one",
            "owner-tab",
            "planner-tab",
            |target| {
                attempts.borrow_mut().push(target.to_string());
                Ok(crate::service::messenger::Delivery::Pty { text: "ok".into() })
            },
        );

        assert!(result.is_err());
        assert!(attempts.into_inner().is_empty());
    }
}
