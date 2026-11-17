use crate::agent::reports::ReportDetails;
use crate::cron::store::CronJob;
use crate::plan::{PlanMode, PlanStatus};
use crate::service::message_gateway::{DeliveryError, DeliveryReceipt};
use crate::service::messenger::Delivery;
use crate::service::orchestration_data::CuratorIdentity;
use crate::service::role_policy::{BlueprintAction, CronLaunch, CronSettlement};
use crate::service::AuthenticatedPlanCaller;
use crate::service::{ReportResult, ReportSubmissionError, WorkerTurnSignal};
use codeterm_plan::service_plan::ReminderCompositionRead;
use codeterm_types::plan::control_loop::CuratorSuspension;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};

use crate::service::extensions::{Extensions, ExtensionsBuilder};
use crate::service::{ServiceState, TeamMutationCaller};

pub type TeamCloseFuture = Pin<Box<dyn Future<Output = Result<(), String>> + Send>>;

/// Calls the core makes up into Team and orchestration logic.
pub trait OrchestrationPort: Send + Sync {
    fn curator_identity(&self, caller: &AuthenticatedPlanCaller)
        -> Result<CuratorIdentity, String>;
    fn schedule_planner_replay(&self, tab_id: &str);
    fn bind_planner_session(&self, tab_id: &str) -> Result<(), String>;
    fn submit_report(
        &self,
        tab_id: &str,
        message: &str,
        details: ReportDetails,
        plan_id: Option<&str>,
        task_id: Option<&str>,
        milestone_kind: Option<&str>,
    ) -> Result<ReportResult, ReportSubmissionError>;
    fn reconcile_time_reminder_eligibility(&self);
    fn expect_spawn_parent_link(&self, tab_id: &str, expected_parent: &str) -> Result<(), String>;
    fn clear_dead_planner_binding(&self, tab_id: &str);
    fn curator_suspended_reason_for_fields(
        &self,
        mode: &PlanMode,
        status: &PlanStatus,
        team_id: &Option<String>,
        complete: bool,
    ) -> Option<CuratorSuspension>;
    fn heartbeat_body_for_popup(
        &self,
        plan: &ReminderCompositionRead,
        tab_id: &str,
        worker: bool,
    ) -> Result<String, String>;
    fn curator_turn_settled(&self, tab_id: &str);
    fn reconcile_curator_id(
        &self,
        id: &str,
        now: u64,
    ) -> Result<(), crate::service::plan::PlanError>;
    fn complete_worker_turn_without_report(&self, tab_id: &str, signal: WorkerTurnSignal);
    fn send_refused_report_diagnostic(
        &self,
        reporter_tab_id: &str,
        message: &str,
    ) -> Result<String, String>;
    fn start_plan_reminder_scheduler(&self);
    fn deliver_tab_todo_recitation(&self, tab_id: &str, text: &str) -> Result<bool, String>;
    fn deliver_orchestrator_notice_exact(
        &self,
        plan_id: &str,
        requested_target: &str,
        context: &str,
        text: &str,
    ) -> Result<Delivery, String>;
    fn curator_policy_authorize_notification(&self, tab: &str) -> Result<(), String>;
    fn curator_policy_session_claimed(&self, tab: &str) -> Result<(), String>;
    fn curator_policy_tab_closed(&self, tab: &str) -> Result<(), String>;
    fn curator_policy_settle_cron(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<CronSettlement, String>;
    fn curator_policy_prepare_cron(
        &self,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String>;
    fn curator_policy_launch_failed(&self, job: &CronJob, now: u64) -> Result<(), String>;
    fn curator_policy_spawned(
        &self,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String>;
    fn curator_policy_prepare_message(
        &self,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String>;
    fn curator_policy_message_receipt(
        &self,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String>;
    fn curator_policy_authorize_blueprint(
        &self,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String>;
    fn curator_policy_learning_owner(
        &self,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String>;
    /// Async by rule-3 exception: it runs the existing close body; the caller awaits it holding no guard.
    fn close_team(
        &self,
        team_id: String,
        caller: TeamMutationCaller,
        force: bool,
    ) -> TeamCloseFuture;
}

pub type Factory = fn(ServiceState) -> Arc<dyn OrchestrationPort>;
pub type ExtensionRegistrar = fn(&mut ExtensionsBuilder);

static FACTORY: OnceLock<(Factory, ExtensionRegistrar)> = OnceLock::new();

/// Per-context slot, filled once right after the context is wrapped in its `Arc`.
#[derive(Default)]
pub(crate) struct InstalledOrchestration(OnceLock<Arc<dyn OrchestrationPort>>);

pub fn register_orchestration(make: Factory, extensions: ExtensionRegistrar) {
    let _ = FACTORY.set((make, extensions));
}

pub(crate) fn register_extensions(builder: &mut ExtensionsBuilder) {
    let (_, extensions) = FACTORY
        .get()
        .expect("orchestration port is registered before the service starts");
    extensions(builder);
}

pub fn install(svc: &ServiceState) {
    let (make, _) = FACTORY
        .get()
        .expect("orchestration port is registered before the service starts");
    fill(svc, make(svc.clone()));
}

fn fill(svc: &ServiceState, port: Arc<dyn OrchestrationPort>) {
    let _ = svc.extensions.get::<InstalledOrchestration>().0.set(port);
}

pub(crate) fn installed(extensions: &Extensions) -> &Arc<dyn OrchestrationPort> {
    extensions
        .get::<InstalledOrchestration>()
        .0
        .get()
        .expect("orchestration port is installed after ServiceContext construction")
}

/// Fixtures get the registered crate when a test registered it, else the recording fake.
#[cfg(any(test, feature = "test-support"))]
pub(crate) fn register_fixture_extensions(builder: &mut ExtensionsBuilder) {
    if let Some((_, extensions)) = FACTORY.get() {
        extensions(builder);
    }
}

#[cfg(any(test, feature = "test-support"))]
pub(crate) fn install_fixture(svc: &ServiceState) {
    match FACTORY.get() {
        Some((make, _)) => fill(svc, make(svc.clone())),
        None => fill(svc, Arc::new(RecordingOrchestration::default())),
    }
}

#[cfg(any(test, feature = "test-support"))]
mod recording;
#[cfg(any(test, feature = "test-support"))]
pub use recording::RecordingOrchestration;

#[cfg(test)]
mod tests {
    #[test]
    fn orchestration_port_is_object_safe_and_thread_safe() {
        super::super::assert_port::<dyn super::OrchestrationPort>();
    }
}
