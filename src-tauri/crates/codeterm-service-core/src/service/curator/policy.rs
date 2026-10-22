use super::*;
use crate::service::message_gateway::{DeliveryError, DeliveryReceipt};
use crate::service::role_policy::{BlueprintAction, CronLaunch, RolePolicy, RoleProfile};

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
        Err(super::ask::notification_refusal(service, tab))
    }
    fn run_completion(&self) -> crate::service::role_policy::RunCompletion {
        crate::service::role_policy::RunCompletion::ExplicitFinish
    }
    fn session_claimed(&self, service: &ServiceContext, tab: &str) -> Result<(), String> {
        service.bind_curator_session(tab)
    }
    fn tab_closed(&self, service: &ServiceContext, tab: &str) -> Result<(), String> {
        service.settle_closed_curator_tab(tab)
    }
    fn settle_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<crate::service::role_policy::CronSettlement, String> {
        let round = service.settle_curator_run(&job.task, tab, run, now)?;
        Ok(crate::service::role_policy::CronSettlement {
            success: matches!(
                round.outcome,
                codeterm_types::plan::CuratorOutcome::Done
                    | codeterm_types::plan::CuratorOutcome::ModeChanged
            ),
            summary: round.outcome.to_string(),
        })
    }
    fn prepare_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String> {
        match service.prepare_curator_fire(job, now) {
            Ok(Some((mirror, task))) => Ok(Some(CronLaunch {
                provider: mirror.provider,
                model: mirror.model,
                effort: mirror.effort,
                task,
            })),
            result => {
                if let Err(error) = result {
                    log::debug!("[curator] prepare {}: {error}", job.task);
                    service.curator_fire_failed(&job.task, error.reason, now)?;
                }
                let cron = service
                    .extensions
                    .try_get::<crate::CronState>()
                    .ok_or("cron unavailable")?;
                let mut disabled = job.clone();
                disabled.enabled = false;
                disabled.next_run = None;
                commit(cron, |_| {
                    Ok(Prepared::new(CronChange::Patch(disabled.clone()), ()))
                })
                .map_err(|error| format!("disable Curator job {}: {error}", job.id))?;
                Ok(None)
            }
        }
    }
    fn launch_failed(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<(), String> {
        service.curator_fire_failed(&job.task, CuratorFailure::LaunchFailed, now)
    }
    fn spawned(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String> {
        service.curator_spawned(&job.task, tab, run, now)
    }
    fn prepare_message(
        &self,
        service: &ServiceContext,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String> {
        service.curator_message(sender, target, body)
    }
    fn message_receipt(
        &self,
        service: &ServiceContext,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String> {
        service.curator_message_receipt(sender, receipt)
    }
    fn authorize_blueprint(
        &self,
        service: &ServiceContext,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String> {
        service.authorize_curator_operation(tab, id, action)
    }
    fn learning_owner(
        &self,
        service: &ServiceContext,
        caller: &AuthenticatedPlanCaller,
        plan_id: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String> {
        let identity = service.authenticate_curator(caller)?;
        if identity.plan_id != plan_id || !matches!(tag, Some("curator" | "curator:next")) {
            return Err("Curator learnings require this round's plan and curator tag".into());
        }
        let plan = service.plan_store.get(plan_id).ok_or("plan absent")?;
        Ok((plan.owner.tab_id, plan.owner.session_id))
    }
}
