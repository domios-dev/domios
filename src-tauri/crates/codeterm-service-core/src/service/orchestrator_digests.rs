mod inventory;
#[cfg(test)]
mod preparation_tests;
pub(crate) use inventory::{snapshot_notice_inventory, NoticeInventorySnapshot};

use crate::service::plan::digest_rules::evaluate_digest_rules;
use crate::service::plan::usage_digest::{
    needs_refresh, usage_digest_lines, UsageDigestRow, FRESHNESS_MS,
};
use crate::service::role_target::deliver_exact_role_target;
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
    _learning_ids: Option<&[String]>,
    _plain: bool,
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
            learning_ids: None,
            plain: true,
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
    if selection.plain {
        let target = svc
            .plan_store
            .notice_delivery_target_projection(plan_id)
            .map_err(|error| error.to_string())?;
        if target.mode == crate::plan::PlanMode::Paused && !selection.mode_transition {
            return Err("plan_paused: plan notices are suppressed while the plan is paused".into());
        }
        let owner = target
            .owner_tab_id
            .as_deref()
            .ok_or("plan owner has no local delivery target")?;
        return deliver_exact_role_target(
            "orchestrator",
            plan_id,
            owner,
            requested_target,
            |target| {
                deliver_notice_payload(svc, plan_id, target, owner, context, text, text, observer)
            },
        );
    }
    let read = svc
        .plan_store
        .notice_projection(plan_id)
        .map_err(|error| error.to_string())?;
    if read.mode == crate::plan::PlanMode::Paused && !selection.mode_transition {
        return Err("plan_paused: plan notices are suppressed while the plan is paused".into());
    }
    let owner = read
        .owner_tab_id
        .clone()
        .ok_or("plan owner has no local delivery target")?;
    deliver_exact_role_target(
        "orchestrator",
        plan_id,
        &owner,
        requested_target,
        |target| {
            let (injected, marks) = if read.complete {
                (text.to_string(), Vec::new())
            } else {
                let now = now_epoch_ms();
                let inventory =
                    snapshot_notice_inventory(svc, plan_id, read.team_id.as_deref(), Some(&owner));
                let input = read
                    .digest_input(now, &inventory.targets())
                    .map_err(|error| error.to_string())?;
                let prepared =
                    prepare_digest_lines_for_input(svc, &input, &inventory, Some(&owner));
                let injected = read
                    .prepend_heartbeat(now, text, &prepared.lines, selection.learning_ids)
                    .map_err(|error| error.to_string())?;
                (injected, prepared.rate_marks)
            };
            drop(read);
            apply_digest_rate_marks(svc, &marks);
            deliver_notice_payload(
                svc, plan_id, target, &owner, context, text, &injected, observer,
            )
        },
    )
}

fn deliver_notice_payload(
    svc: &ServiceContext,
    plan_id: &str,
    target: &str,
    owner: &str,
    context: &str,
    text: &str,
    injected: &str,
    observer: Option<crate::service::messenger::DeliveryWriteObserver>,
) -> Result<crate::service::messenger::Delivery, String> {
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
        injected,
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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DigestRateMark {
    tab_id: String,
    rule_id: String,
    at: u64,
}

pub(crate) struct PreparedNoticeDigests {
    pub(crate) lines: Vec<String>,
    pub(crate) rate_marks: Vec<DigestRateMark>,
}

pub(crate) fn apply_digest_rate_marks(svc: &ServiceContext, marks: &[DigestRateMark]) {
    for mark in marks {
        svc.plan_turn_counter
            .mark_digest_rule_fired(&mark.tab_id, &mark.rule_id, mark.at);
    }
}

fn prepare_queue_pressure_lines(
    svc: &ServiceContext,
    cfg: &crate::config::schema::OrchestrationDigestsConfig,
    counters: &crate::service::plan::digest_rules::PlanDigestCounters,
    tab_id: Option<&str>,
) -> (Vec<String>, Vec<DigestRateMark>) {
    let fired = evaluate_digest_rules(cfg, counters);
    let Some(tab_id) = tab_id.filter(|id| !id.trim().is_empty()) else {
        return (
            fired.into_iter().map(|(_, line)| line).collect(),
            Vec::new(),
        );
    };
    let mut lines = Vec::new();
    let mut marks = Vec::new();
    let mut pending = std::collections::HashMap::<String, u64>::new();
    for (rule, line) in fired {
        let now = now_epoch_secs();
        let due = pending
            .get(&rule.id)
            .map(|at| now.saturating_sub(*at) >= rule.rate_limit_secs)
            .unwrap_or_else(|| {
                svc.plan_turn_counter
                    .digest_rule_due(tab_id, &rule.id, now, rule.rate_limit_secs)
            });
        if due {
            let at = now_epoch_secs();
            pending.insert(rule.id.clone(), at);
            marks.push(DigestRateMark {
                tab_id: tab_id.into(),
                rule_id: rule.id.clone(),
                at,
            });
            lines.push(line);
        }
    }
    (lines, marks)
}

pub(crate) fn prepare_digest_lines_for_input(
    svc: &ServiceContext,
    input: &codeterm_plan::service_plan::NoticeDigestInput,
    inventory: &NoticeInventorySnapshot,
    tab_id: Option<&str>,
) -> PreparedNoticeDigests {
    let mut counters = input.queue_counters();
    inventory.populate(&input.finished_bindings, &mut counters);
    let cfg = svc.configuration().orchestration;
    let mut lines = crate::service::cartographer::digest_lines_from_input(input, cfg.map.enabled);
    let (queue, rate_marks) = prepare_queue_pressure_lines(svc, &cfg.digests, &counters, tab_id);
    lines.extend(queue);
    lines.extend(usage_lines(svc));
    lines.truncate(8);
    PreparedNoticeDigests { lines, rate_marks }
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

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default()
}

fn now_epoch_secs() -> u64 {
    now_epoch_ms() / 1000
}
