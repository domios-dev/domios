use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::plan::{OrchestratorRef, Plan};
use crate::service::extensions::Extensions;
use crate::service::messenger::{Delivery, DeliveryWriteObserver};
use crate::service::plan::PlanError;
use crate::service::{ServiceState, TeamMutationCaller, TeamPlanError};

pub(crate) type TeamCloseFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Calls the core makes up into Team and orchestration logic.
pub(crate) trait OrchestrationPort: Send + Sync {
    /// Async by rule-3 exception: it runs the existing close body; the caller awaits it holding no guard.
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture;
    fn valid_live_plan_team(&self, plan_id: &str) -> Result<String, TeamPlanError>;
    fn bind_pending_paired_planner(
        &self,
        plan_id: &str,
        owner: &OrchestratorRef,
        now_ms: u64,
    ) -> Result<bool, String>;
    fn deliver_gated_heartbeat_exact(
        &self,
        plan_id: &str,
        target: &str,
        text: &str,
        learning_ids: Option<&[String]>,
        plain: bool,
        mode_transition: bool,
        observer: Option<DeliveryWriteObserver>,
    ) -> Result<Delivery, String>;
    fn collect_digest_lines(&self, plan: &Plan, tab_id: Option<&str>) -> Vec<String>;
    fn deliver_orchestrator_notice_exact(
        &self,
        plan_id: &str,
        requested_target: &str,
        context: &str,
        text: &str,
    ) -> Result<Delivery, String>;
    fn live_team_ids(&self) -> BTreeSet<String>;
    fn reconcile_dangling_team_plan_bindings(
        &self,
        live_team_ids: &BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError>;
}

type Factory = fn(ServiceState) -> Arc<dyn OrchestrationPort>;

static FACTORY: OnceLock<Factory> = OnceLock::new();

/// Per-context slot, filled once right after the context is wrapped in its `Arc`.
#[derive(Default)]
pub(crate) struct InstalledOrchestration(OnceLock<Arc<dyn OrchestrationPort>>);

pub(crate) fn register_orchestration(make: Factory) {
    let _ = FACTORY.set(make);
}

pub(crate) fn install(svc: &ServiceState) {
    let make = FACTORY
        .get()
        .expect("orchestration port is registered before the service starts");
    let _ = svc
        .extensions
        .get::<InstalledOrchestration>()
        .0
        .set(make(svc.clone()));
}

pub(crate) fn installed(extensions: &Extensions) -> &Arc<dyn OrchestrationPort> {
    extensions
        .get::<InstalledOrchestration>()
        .0
        .get()
        .expect("orchestration port is installed after ServiceContext construction")
}

#[cfg(test)]
mod tests {
    #[test]
    fn orchestration_port_is_object_safe_and_thread_safe() {
        super::super::assert_port::<dyn super::OrchestrationPort>();
    }
}
