use crate::traits::*;
use codeterm_plan::service_plan::ReminderCompositionRead;
use codeterm_service_core::agent::reports::ReportDetails;
use codeterm_service_core::cron::store::CronJob;
use codeterm_service_core::plan::{PlanMode, PlanStatus};
use codeterm_service_core::service::message_gateway::{DeliveryError, DeliveryReceipt};
use codeterm_service_core::service::messenger::Delivery;
use codeterm_service_core::service::orchestration_data::CuratorIdentity;
use codeterm_service_core::service::role_policy::{BlueprintAction, CronLaunch, CronSettlement};
use codeterm_service_core::service::AuthenticatedPlanCaller;
use codeterm_service_core::service::{ReportResult, ReportSubmissionError, WorkerTurnSignal};
use codeterm_types::plan::control_loop::{CuratorFailure, CuratorSuspension};
use std::sync::{Arc, Weak};

use codeterm_service_core::ports::orchestration::{OrchestrationPort, TeamCloseFuture};
use codeterm_service_core::service::{
    ExtensionsBuilder, ServiceContext, ServiceState, TeamMutationCaller,
};

use crate::team::template_start::TemplateStartLocks;
use crate::team::TeamExt;
use crate::{CartographerAttachLocks, OrchestratorAttachLocks};

/// Weak, because the port lives in the context's own extensions.
pub(crate) struct OrchestrationPortImpl(Weak<ServiceContext>);

impl OrchestrationPortImpl {
    fn svc(&self) -> ServiceState {
        self.0
            .upgrade()
            .expect("orchestration port outlived its ServiceContext")
    }
}

pub fn register_orchestration() {
    codeterm_service_core::ports::orchestration::register_orchestration(
        |svc| {
            // Fail fast at install, as required() did: both lock keys are registered extensions.
            svc.ext::<CartographerAttachLocks>();
            svc.ext::<OrchestratorAttachLocks>();
            svc.ext::<TemplateStartLocks>();
            Arc::new(OrchestrationPortImpl(Arc::downgrade(&svc)))
        },
        register_extensions,
    );
}

fn register_extensions(extensions: &mut ExtensionsBuilder) {
    extensions.insert(CartographerAttachLocks(Default::default()));
    extensions.insert(OrchestratorAttachLocks(Default::default()));
    extensions.insert(TemplateStartLocks(Default::default()));
}

impl OrchestrationPort for OrchestrationPortImpl {
    fn curator_identity(
        &self,
        caller: &AuthenticatedPlanCaller,
    ) -> Result<CuratorIdentity, String> {
        self.svc().curator_identity(caller)
    }
    fn schedule_planner_replay(&self, tab_id: &str) {
        self.svc().schedule_planner_replay(tab_id)
    }
    fn bind_planner_session(&self, tab_id: &str) -> Result<(), String> {
        self.svc().bind_planner_session(tab_id)
    }
    fn submit_report(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, ReportSubmissionError> {
        self.svc()
            .submit_report(tab_id, message, details, plan_id, task_id, milestone_kind)
    }
    fn reconcile_time_reminder_eligibility(&self) {
        self.svc().reconcile_time_reminder_eligibility()
    }
    fn expect_spawn_parent_link(&self, tab_id: &str, expected_parent: &str) -> Result<(), String> {
        self.svc().expect_spawn_parent_link(tab_id, expected_parent)
    }
    fn clear_dead_planner_binding(&self, tab_id: &str) {
        self.svc().clear_dead_planner_binding(tab_id)
    }
    fn curator_suspended_reason_for_fields(
        &self,
        mode: &PlanMode,
        status: &PlanStatus,
        team_id: &Option<String>,
        complete: bool,
    ) -> Option<CuratorSuspension> {
        self.svc()
            .curator_suspended_reason_for_fields(mode, status, team_id, complete)
    }
    fn heartbeat_body_for_popup(
        &self,
        plan: &ReminderCompositionRead,
        tab_id: &str,
        worker: bool,
    ) -> Result<String, String> {
        self.svc().heartbeat_body_for_popup(plan, tab_id, worker)
    }
    fn curator_turn_settled(&self, tab_id: &str) {
        self.svc().curator_turn_settled(tab_id)
    }
    fn reconcile_curator_id(
        &self,
        id: &str,
        now: u64,
    ) -> Result<(), codeterm_service_core::service::plan::PlanError> {
        self.svc().reconcile_curator_id(id, now)
    }
    fn complete_worker_turn_without_report(&self, tab_id: &str, signal: WorkerTurnSignal) {
        self.svc()
            .complete_worker_turn_without_report(tab_id, signal)
    }
    fn send_refused_report_diagnostic(
        &self,
        reporter_tab_id: &str,
        message: &str,
    ) -> Result<String, String> {
        self.svc()
            .send_refused_report_diagnostic(reporter_tab_id, message)
    }
    fn start_plan_reminder_scheduler(&self) {
        crate::hierarchy::start_plan_reminder_scheduler(self.svc())
    }
    fn deliver_tab_todo_recitation(&self, tab_id: &str, text: &str) -> Result<bool, String> {
        crate::hierarchy::deliver_tab_todo_recitation(&self.svc(), tab_id, text)
    }
    fn deliver_orchestrator_notice_exact(
        &self,
        plan_id: &str,
        requested_target: &str,
        context: &str,
        text: &str,
    ) -> Result<Delivery, String> {
        crate::orchestrator_digests::deliver_orchestrator_notice_exact(
            &self.svc(),
            plan_id,
            requested_target,
            context,
            text,
        )
    }
    fn curator_policy_authorize_notification(&self, tab: &str) -> Result<(), String> {
        let service = self.svc();
        Err(crate::curator::ask::notification_refusal(&service, tab))
    }
    fn curator_policy_session_claimed(&self, tab: &str) -> Result<(), String> {
        let service = self.svc();
        service.bind_curator_session(tab)
    }
    fn curator_policy_tab_closed(&self, tab: &str) -> Result<(), String> {
        let service = self.svc();
        service.settle_closed_curator_tab(tab)
    }
    fn curator_policy_settle_cron(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<CronSettlement, String> {
        let service = self.svc();
        let round = service.settle_curator_run(&job.task, tab, run, now)?;
        Ok(
            codeterm_service_core::service::role_policy::CronSettlement {
                success: matches!(
                    round.outcome,
                    codeterm_types::plan::CuratorOutcome::Done
                        | codeterm_types::plan::CuratorOutcome::ModeChanged
                ),
                summary: round.outcome.to_string(),
            },
        )
    }
    fn curator_policy_prepare_cron(
        &self,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String> {
        let service = self.svc();
        match service.prepare_curator_fire(job, now) {
            Ok(Some((mirror, task))) => Ok(Some(CronLaunch {
                provider: mirror.provider,
                launch_profile: mirror.launch_profile,
                model: mirror.model,
                effort: mirror.effort,
                task,
            })),
            result => {
                if let Err(error) = result {
                    if error.read_failure {
                        return Err(error.to_string());
                    }
                    log::debug!("[curator] prepare {}: {error}", job.task);
                    service.curator_fire_refused(
                        &job.task,
                        error.reason,
                        error.cause.as_deref(),
                        now,
                    )?;
                }
                service
                    .reconcile_curator_id(&job.task, now)
                    .map_err(|error| error.to_string())?;
                Ok(None)
            }
        }
    }
    fn curator_policy_launch_failed(&self, job: &CronJob, now: u64) -> Result<(), String> {
        let service = self.svc();
        service.curator_fire_failed(&job.task, CuratorFailure::LaunchFailed, now)
    }
    fn curator_policy_spawned(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String> {
        let service = self.svc();
        service.curator_spawned(&job.task, tab, run, now)
    }
    fn curator_policy_prepare_message(
        &self,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String> {
        let service = self.svc();
        service.curator_message(sender, target, body)
    }
    fn curator_policy_message_receipt(
        &self,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String> {
        let service = self.svc();
        service.curator_message_receipt(sender, receipt)
    }
    fn curator_policy_authorize_blueprint(
        &self,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String> {
        let service = self.svc();
        service.authorize_curator_operation(tab, id, action)
    }
    fn curator_policy_learning_owner(
        &self,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String> {
        let service = self.svc();
        let identity = service.authenticate_curator(caller)?;
        if identity.plan_id != plan_id || !matches!(tag, Some("curator" | "curator:next")) {
            return Err("Curator learnings require this round's plan and curator tag".into());
        }
        let plan = service
            .plan_store
            .plan_identity_projection(plan_id)
            .map_err(|error| error.to_string())?;
        Ok((plan.owner.tab_id, plan.owner.session_id))
    }
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture {
        let svc = self.svc();
        Box::pin(async move {
            let result = svc.close_team(&team_id, caller, force).await?;
            let preserved: Vec<_> = result
                .inventory
                .kept
                .iter()
                .filter_map(|lane| lane.path.as_deref().or(lane.branch.as_deref()))
                .collect();
            if !preserved.is_empty() {
                svc.notify_event(
                    "close-lanes-preserved",
                    serde_json::json!({
                        "paths": preserved,
                        "workspace_id": result.team.workspace_id,
                    }),
                );
            }
            Ok(())
        })
    }
}
