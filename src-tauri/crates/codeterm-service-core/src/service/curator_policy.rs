use crate::cron::store::CronJob;
use crate::service::message_gateway::{DeliveryError, DeliveryReceipt};
use crate::service::role_policy::{BlueprintAction, CronLaunch, RolePolicy, RoleProfile};
use crate::service::{AuthenticatedPlanCaller, ServiceContext};

pub(crate) struct CuratorPolicy;
pub(crate) static POLICY: CuratorPolicy = CuratorPolicy;

impl RolePolicy for CuratorPolicy {
    fn profile(&self) -> RoleProfile {
        RoleProfile::Curator
    }
    fn spawn_role(&self) -> &'static str {
        "Manager"
    }
    fn retain_oneshot(&self) -> bool {
        true
    }
    fn allow_manual_fire(&self) -> bool {
        false
    }
    fn raw_task(&self) -> bool {
        true
    }
    fn retarget_only(&self) -> bool {
        true
    }
    fn conservative_save(&self) -> bool {
        true
    }
    fn authorize_notification(&self, service: &ServiceContext, tab: &str) -> Result<(), String> {
        service
            .orchestration()
            .curator_policy_authorize_notification(tab)
    }
    fn run_completion(&self) -> crate::service::role_policy::RunCompletion {
        crate::service::role_policy::RunCompletion::ExplicitFinish
    }
    fn session_claimed(&self, service: &ServiceContext, tab: &str) -> Result<(), String> {
        service.orchestration().curator_policy_session_claimed(tab)
    }
    fn tab_closed(&self, service: &ServiceContext, tab: &str) -> Result<(), String> {
        service.orchestration().curator_policy_tab_closed(tab)
    }
    fn settle_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<crate::service::role_policy::CronSettlement, String> {
        service
            .orchestration()
            .curator_policy_settle_cron(job, tab, run, now)
    }
    fn prepare_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String> {
        service
            .orchestration()
            .curator_policy_prepare_cron(job, now)
    }
    fn launch_failed(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<(), String> {
        service
            .orchestration()
            .curator_policy_launch_failed(job, now)
    }
    fn spawned(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String> {
        service
            .orchestration()
            .curator_policy_spawned(job, tab, run, now)
    }
    fn prepare_message(
        &self,
        service: &ServiceContext,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String> {
        service
            .orchestration()
            .curator_policy_prepare_message(sender, target, body)
    }
    fn message_receipt(
        &self,
        service: &ServiceContext,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String> {
        service
            .orchestration()
            .curator_policy_message_receipt(sender, receipt)
    }
    fn authorize_blueprint(
        &self,
        service: &ServiceContext,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String> {
        service
            .orchestration()
            .curator_policy_authorize_blueprint(tab, id, action)
    }
    fn learning_owner(
        &self,
        service: &ServiceContext,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String> {
        service
            .orchestration()
            .curator_policy_learning_owner(caller, plan_id, tag)
    }
}
