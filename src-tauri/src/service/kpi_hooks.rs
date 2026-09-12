//! Choke-point hooks for the KPI ledger. Every entry point returns immediately when
//! `kpi.enabled` is off, so a disabled ledger costs one boolean test and resolves no
//! attribution at all.
//!
//! Attribution target per kind: deliver/nudge/heartbeat charge the *recipient* tab
//! (where the injected bytes land), report charges the *reporting* tab, spawn charges
//! the *spawned* tab.

use super::messenger::MessageKind;
use super::ServiceContext;
use crate::agent::kpi::{deliver_event_kind, KpiAttribution, UsageSource};
use crate::agent::kpi_ledger::KpiEventKind;

impl ServiceContext {
    pub(crate) fn kpi_attribution(&self, tab_id: &str) -> KpiAttribution {
        let (resolved, role, role_profile, provider) = {
            let mux = self.mux.read();
            let resolved = mux
                .resolve_tab_id(tab_id)
                .unwrap_or_else(|| tab_id.to_string());
            let tab = mux.get_tab(&resolved);
            (
                resolved,
                tab.and_then(|p| p.role),
                tab.and_then(|p| p.role_profile.clone()),
                tab.and_then(|p| p.agent_type.clone()),
            )
        };
        // Session-scoped provider evidence, or nothing: a tab the model poller has
        // not reached must not fall back to a provider or launch-time guess.
        let model = self.verified_session_model(&resolved);
        let (plan_id, task_id) = self.kpi_plan_context(&resolved);
        KpiAttribution::for_tab(resolved)
            .plan(plan_id, task_id)
            .role(role, role_profile.as_deref())
            .provider(provider)
            .model(model)
    }

    /// A task assignment carries both ids. Without one, a plan's owner with open work
    /// and its registered Planner still take the `plan_id` with a null `task_id`. An
    /// unbound tab and a tab two active plans claim both stay unassigned.
    fn kpi_plan_context(&self, tab_id: &str) -> (Option<String>, Option<String>) {
        if let Some((plan_id, task_id)) = self.plan_store.task_binding_for_tab(tab_id) {
            return (Some(plan_id), Some(task_id));
        }
        let mut candidates = self
            .plan_store
            .taskless_plan_context_candidates_for_tab(tab_id);
        match candidates.len() {
            1 => (candidates.pop(), None),
            _ => (None, None),
        }
    }

    pub(crate) fn record_kpi_delivery(&self, to_tab: &str, kind: MessageKind, bytes: usize) {
        if !self.kpi.is_enabled() {
            return;
        }
        let Some(event_kind) = deliver_event_kind(kind) else {
            return;
        };
        let attribution = self.kpi_attribution(to_tab);
        self.kpi
            .record(attribution.into_event(event_kind, bytes as u64));
    }

    /// Reports carry a plan/task already resolved through the report's own priority
    /// chain, which outranks the tab's raw binding. `report_id` is the ledger's own
    /// stable identity for this report (already used to guard its delivery state), so
    /// reusing it as the KPI dedup key makes a retried `submit_report` a no-op here too.
    pub(crate) fn record_kpi_report(
        &self,
        tab_id: &str,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        model: Option<&str>,
        bytes: usize,
        report_id: &str,
    ) {
        if !self.kpi.is_enabled() {
            return;
        }
        let mut attribution = self.kpi_attribution(tab_id);
        if plan_id.is_some() {
            attribution.plan_id = plan_id.map(str::to_string);
            attribution.task_id = task_id.map(str::to_string);
        }
        if let Some(model) = model {
            attribution.model = Some(model.to_string());
        }
        self.kpi.record(attribution.into_event_with_dedup(
            KpiEventKind::Report,
            bytes as u64,
            report_id.to_string(),
        ));
    }

    /// At spawn time the tab has no runtime model evidence yet, so the launch model is
    /// the only identity available.
    pub(crate) fn record_kpi_spawn(&self, tab_id: &str, model: Option<&str>, brief_bytes: usize) {
        if !self.kpi.is_enabled() {
            return;
        }
        let mut attribution = self.kpi_attribution(tab_id);
        if attribution.model.is_none() {
            attribution.model = model.map(str::to_string);
        }
        self.kpi
            .record(attribution.into_event(KpiEventKind::Spawn, brief_bytes as u64));
    }

    /// Single writer for the bounce/accept review verdicts (`socket/server/plan_review.rs`).
    /// `review_round` is fixed for the duration of a round (bounce is what bumps it), so
    /// `task_id:round:verdict` is already the system's own stable identity for "this
    /// round's verdict" — reusing it as the dedup key makes a retried RPC a no-op.
    pub(crate) fn record_kpi_review_verdict(
        &self,
        tab_id: &str,
        plan_id: &str,
        task_id: &str,
        verdict: KpiEventKind,
        review_round: u32,
        bytes: usize,
    ) {
        if !self.kpi.is_enabled() {
            return;
        }
        let mut attribution = self.kpi_attribution(tab_id);
        attribution.plan_id = Some(plan_id.to_string());
        attribution.task_id = Some(task_id.to_string());
        let dedup_key = format!("{task_id}:{review_round}:{}", verdict.label());
        self.kpi
            .record(attribution.into_event_with_dedup(verdict, bytes as u64, dedup_key));
    }

    pub(crate) fn record_kpi_event(&self, tab_id: &str, kind: KpiEventKind, bytes: usize) {
        if !self.kpi.is_enabled() {
            return;
        }
        let attribution = self.kpi_attribution(tab_id);
        self.kpi.record(attribution.into_event(kind, bytes as u64));
    }

    /// Queues a forward-only pass over the tab's provider session log. The path comes
    /// from the provider capability, so core never encodes a per-provider layout.
    pub(crate) fn request_kpi_usage_ingest(&self, tab_id: &str) {
        if !self.kpi.is_enabled() {
            return;
        }
        let Some(source) = self.tab_usage_source(tab_id) else {
            return;
        };
        let attribution = self.kpi_attribution(tab_id);
        self.kpi.request_ingest(source, attribution);
    }

    fn tab_usage_source(&self, tab_id: &str) -> Option<UsageSource> {
        let (agent_type, cwd, session_id) = {
            let mux = self.mux.read();
            let resolved = mux.resolve_tab_id(tab_id)?;
            let tab = mux.get_tab(&resolved)?;
            (
                tab.agent_type.clone()?,
                tab.cwd.clone()?,
                tab.agent_session_id.clone()?,
            )
        };
        self.agent_registry
            .with_provider(&agent_type, |provider| {
                provider.session_file_path(&cwd, &session_id)
            })
            .flatten()
            .map(UsageSource::new)
    }
}
