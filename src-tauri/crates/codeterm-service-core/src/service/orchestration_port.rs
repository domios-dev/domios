use std::collections::BTreeSet;
use std::sync::{Arc, Weak};

use crate::plan::{OrchestratorRef, Plan};
use crate::ports::orchestration::{OrchestrationPort, TeamCloseFuture};
use crate::service::messenger::{Delivery, DeliveryWriteObserver};
use crate::service::plan::PlanError;
use crate::service::team::TeamExt;
use crate::service::{ServiceContext, ServiceState, TeamMutationCaller, TeamPlanError};

/// Weak, because the port lives in the context's own extensions.
pub(crate) struct OrchestrationPortImpl(Weak<ServiceContext>);

impl OrchestrationPortImpl {
    fn svc(&self) -> ServiceState {
        self.0
            .upgrade()
            .expect("orchestration port outlived its ServiceContext")
    }
}

pub(crate) fn register_orchestration() {
    crate::ports::orchestration::register_orchestration(|svc| {
        Arc::new(OrchestrationPortImpl(Arc::downgrade(&svc)))
    });
}

impl OrchestrationPort for OrchestrationPortImpl {
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture {
        let svc = self.svc();
        Box::pin(async move { svc.close_team(&team_id, caller, force).await.map(|_| ()) })
    }

    fn valid_live_plan_team(&self, plan_id: &str) -> Result<String, TeamPlanError> {
        let svc = self.svc();
        let mux = svc.mux.read();
        super::team::valid_live_plan_team(&svc, &mux, plan_id)
    }

    fn bind_pending_paired_planner(
        &self,
        plan_id: &str,
        owner: &OrchestratorRef,
        now_ms: u64,
    ) -> Result<bool, String> {
        super::bind_pending_paired_planner(&self.svc(), plan_id, owner, now_ms)
    }

    fn deliver_gated_heartbeat_exact(
        &self,
        plan_id: &str,
        target: &str,
        text: &str,
        learning_ids: Option<&[String]>,
        plain: bool,
        mode_transition: bool,
        observer: Option<DeliveryWriteObserver>,
    ) -> Result<Delivery, String> {
        super::orchestrator_digests::deliver_gated_heartbeat_exact(
            &self.svc(),
            plan_id,
            target,
            text,
            learning_ids,
            plain,
            mode_transition,
            observer,
        )
    }

    fn collect_digest_lines(&self, plan: &Plan, tab_id: Option<&str>) -> Vec<String> {
        super::orchestrator_digests::collect_digest_lines(&self.svc(), plan, tab_id)
    }

    fn deliver_orchestrator_notice_exact(
        &self,
        plan_id: &str,
        requested_target: &str,
        context: &str,
        text: &str,
    ) -> Result<Delivery, String> {
        super::orchestrator_digests::deliver_orchestrator_notice_exact(
            &self.svc(),
            plan_id,
            requested_target,
            context,
            text,
        )
    }

    fn live_team_ids(&self) -> BTreeSet<String> {
        super::live_team_ids(&self.svc())
    }

    fn reconcile_dangling_team_plan_bindings(
        &self,
        live_team_ids: &BTreeSet<String>,
        now_ms: u64,
    ) -> Result<Vec<String>, PlanError> {
        super::reconcile_dangling_team_plan_bindings_and_emit(&self.svc(), live_team_ids, now_ms)
    }
}
