//! Fills the two Mission Control blocks the plan crate cannot reach: the
//! ledger-backed KPI figures and the per-provider runway.
//!
//! The KPI block reads exactly one source, `KpiLedger::aggregate_by_plan`, and
//! renders an explicit not-recording state whenever the flag is off, the ledger
//! is unreadable, or the plan has no row. No provider session jsonl is opened.

use codeterm_plan::service_plan::state::mission_control::{
    KpiNotRecordingReason, MissionControlKpi, PlanKpiSpend, PlanKpiTokens, ProviderRunway,
    RunwaySection, RunwayState,
};

use crate::service::ServiceContext;

pub(crate) fn plan_kpi(svc: &ServiceContext, plan_id: &str) -> MissionControlKpi {
    let Some(ledger) = svc.kpi.ledger() else {
        return MissionControlKpi::not_recording(KpiNotRecordingReason::Disabled);
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

pub(crate) fn provider_runway(svc: &ServiceContext) -> RunwaySection {
    let mut providers: Vec<ProviderRunway> = Vec::new();
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
        let label = svc
            .agent_registry
            .with_provider(&provider, |p| p.display_name().to_string())
            .unwrap_or_else(|| provider.clone());
        providers.push(ProviderRunway {
            provider,
            label,
            captured_at: row.captured_at,
            session_pct: row.session_pct,
            session_resets_at: row.session_resets_at,
            weekly_pct: row.weekly_pct,
            weekly_resets_at: row.weekly_resets_at,
            stale_reason: row.failure_reason,
        });
    }
    RunwaySection {
        state: if providers.is_empty() {
            RunwayState::NotReporting
        } else {
            RunwayState::Reporting
        },
        providers,
    }
}
