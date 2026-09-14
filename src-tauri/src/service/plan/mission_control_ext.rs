//! Fills the two Mission Control blocks the plan crate cannot reach: the
//! plan-scoped KPI figures and the machine-scoped provider/model spend.
//!
//! The KPI block reads exactly one source, `KpiLedger::aggregate_by_plan`, and
//! renders an explicit not-recording state whenever the flag is off, the ledger
//! is unreadable, or the plan has no row. No provider session jsonl is opened.
//! The opt-out state carries the config key that turns recording back on.
//!
//! The spend block reads `token_usage` rows across the whole ledger, because plan
//! attribution cannot yet charge every coordination turn — so it is labelled a
//! machine reading. Provider percentages come from the usage store at provider and
//! window granularity and are never divided across the models under them.

use std::collections::BTreeMap;

use codeterm_plan::service_plan::state::mission_control::{
    KpiNotRecordingReason, MissionControlKpi, MissionControlSpend, ModelSpend, PlanKpiSpend,
    PlanKpiTokens, ProviderAllowance, ProviderSpend,
};

use crate::agent::kpi_ledger::ProviderModelUsage;
use crate::service::ServiceContext;

pub(crate) fn plan_kpi(svc: &ServiceContext, plan_id: &str) -> MissionControlKpi {
    let Some(ledger) = svc.kpi.ledger() else {
        // A ledger that refused to open is a failure, not an opt-out: naming the
        // setting there would tell the owner to switch on what is already on.
        return MissionControlKpi::not_recording(if svc.kpi.open_failed() {
            KpiNotRecordingReason::Unreadable
        } else {
            KpiNotRecordingReason::Disabled
        });
    };
    let aggregates = match ledger.aggregate_by_plan() {
        Ok(aggregates) => aggregates,
        Err(e) => {
            log::warn!("[mission-control] kpi aggregate_by_plan failed: {}", e);
            return MissionControlKpi::not_recording(KpiNotRecordingReason::Unreadable);
        }
    };
    let Some(row) = aggregates
        .into_iter()
        .find(|aggregate| aggregate.plan_id.as_deref() == Some(plan_id))
    else {
        return MissionControlKpi::not_recording(KpiNotRecordingReason::NoRowsForPlan);
    };
    MissionControlKpi::recording(PlanKpiSpend {
        events: row.count,
        bytes: row.total_bytes,
        turns: row.turns,
        tokens: PlanKpiTokens {
            input: row.input_tokens,
            uncached_input: row.uncached_input_tokens,
            output: row.output_tokens,
            total: row.input_tokens.saturating_add(row.output_tokens),
        },
    })
}

pub(crate) fn machine_spend(svc: &ServiceContext) -> MissionControlSpend {
    let allowances = provider_allowances(svc);
    let Some(ledger) = svc.kpi.ledger() else {
        let reason = if svc.kpi.open_failed() {
            KpiNotRecordingReason::Unreadable
        } else {
            KpiNotRecordingReason::Disabled
        };
        return MissionControlSpend::not_recording(reason).with_providers(provider_rows(
            svc,
            Vec::new(),
            allowances,
            0,
        ));
    };
    let usage = match ledger.aggregate_token_usage_by_provider_model() {
        Ok(usage) => usage,
        Err(e) => {
            log::warn!(
                "[mission-control] kpi provider/model aggregate failed: {}",
                e
            );
            return MissionControlSpend::not_recording(KpiNotRecordingReason::Unreadable)
                .with_providers(provider_rows(svc, Vec::new(), allowances, 0));
        }
    };
    let measured_tokens: u64 = usage.iter().fold(0u64, |sum, row| {
        sum.saturating_add(row.uncached_input_tokens)
    });
    let turns: u64 = usage
        .iter()
        .fold(0u64, |sum, row| sum.saturating_add(row.turns));
    let measured = !usage.is_empty();
    let providers = provider_rows(svc, usage, allowances, measured_tokens);
    if !measured {
        return MissionControlSpend::not_recording(KpiNotRecordingReason::NoRowsForPlan)
            .with_providers(providers);
    }
    MissionControlSpend::recording(measured_tokens, turns, providers)
}

/// The latest usage snapshot per provider that reports one, keyed by provider id —
/// the same id the ledger records in its `provider` column.
fn provider_allowances(svc: &ServiceContext) -> BTreeMap<String, ProviderAllowance> {
    let mut out: BTreeMap<String, ProviderAllowance> = BTreeMap::new();
    for provider in svc.agent_registry.provider_ids() {
        let reports_usage = svc
            .agent_registry
            .with_provider(&provider, |p| p.usage_parser().is_some())
            .unwrap_or(false);
        if !reports_usage || !crate::config::usage_tracking_enabled(&provider) {
            continue;
        }
        let Ok(Some(row)) = svc
            .usage_store
            .get_latest_with_stale_reason(&provider, None)
        else {
            continue;
        };
        out.insert(provider, allowance_from_row(&row));
    }
    out
}

/// Straight copy, deliberately: both shipped parsers already store a consumed
/// figure (Claude's `utilization`, Codex's `used_percent`, and Codex's text path
/// converts a "left" reading to used before storing it), so converting here would
/// invert a correct number. The window is the finest granularity the source has.
pub(crate) fn allowance_from_row(
    row: &codeterm_types::usage::UsageSnapshotRow,
) -> ProviderAllowance {
    ProviderAllowance {
        captured_at: row.captured_at,
        session_used_pct: row.session_pct,
        session_resets_at: row.session_resets_at,
        weekly_used_pct: row.weekly_pct,
        weekly_resets_at: row.weekly_resets_at,
        stale_reason: row.failure_reason.clone(),
    }
}

fn provider_rows(
    svc: &ServiceContext,
    usage: Vec<ProviderModelUsage>,
    mut allowances: BTreeMap<String, ProviderAllowance>,
    measured_tokens: u64,
) -> Vec<ProviderSpend> {
    let mut by_provider: BTreeMap<String, ProviderSpend> = BTreeMap::new();
    for row in usage {
        let entry = by_provider
            .entry(row.provider.clone())
            .or_insert_with(|| ProviderSpend {
                label: provider_label(svc, &row.provider),
                provider: row.provider.clone(),
                ..Default::default()
            });
        entry.turns = entry.turns.saturating_add(row.turns);
        entry.input_tokens = entry.input_tokens.saturating_add(row.input_tokens);
        entry.uncached_input_tokens = entry
            .uncached_input_tokens
            .saturating_add(row.uncached_input_tokens);
        entry.output_tokens = entry.output_tokens.saturating_add(row.output_tokens);
        entry.models.push(ModelSpend {
            token_share_pct: token_share_pct(row.uncached_input_tokens, measured_tokens),
            model: row.model,
            turns: row.turns,
            input_tokens: row.input_tokens,
            uncached_input_tokens: row.uncached_input_tokens,
            output_tokens: row.output_tokens,
        });
    }
    // A provider with an allowance reading but no measured row still belongs on the
    // card: its window percentages are a real observation about this machine.
    for provider in allowances.keys() {
        by_provider
            .entry(provider.clone())
            .or_insert_with(|| ProviderSpend {
                label: provider_label(svc, provider),
                provider: provider.clone(),
                ..Default::default()
            });
    }
    let mut rows: Vec<ProviderSpend> = by_provider.into_values().collect();
    for row in &mut rows {
        row.allowance = allowances.remove(&row.provider);
        row.models.sort_by(|a, b| {
            measured(b)
                .cmp(&measured(a))
                .then(model_key(a).cmp(&model_key(b)))
        });
    }
    rows.sort_by(|a, b| {
        provider_measured(b)
            .cmp(&provider_measured(a))
            .then(a.provider.cmp(&b.provider))
    });
    rows
}

fn measured(model: &ModelSpend) -> u64 {
    model.uncached_input_tokens
}

/// Unresolved identity sorts last within its provider, so a real model never hides
/// behind the unassigned bucket on a tie.
fn model_key(model: &ModelSpend) -> (bool, String) {
    match &model.model {
        Some(name) => (false, name.clone()),
        None => (true, String::new()),
    }
}

fn provider_measured(provider: &ProviderSpend) -> u64 {
    provider.uncached_input_tokens
}

fn token_share_pct(part: u64, total: u64) -> f32 {
    if total == 0 {
        return 0.0;
    }
    (part as f64 / total as f64 * 100.0) as f32
}

/// An empty `provider` column is the ledger's own unresolved provider identity; it
/// keeps a named bucket rather than borrowing a neighbour's label.
fn provider_label(svc: &ServiceContext, provider: &str) -> String {
    if provider.trim().is_empty() {
        return "Unassigned provider".to_string();
    }
    svc.agent_registry
        .with_provider(provider, |p| p.display_name().to_string())
        .unwrap_or_else(|| provider.to_string())
}
