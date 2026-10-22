use super::message_gateway::{DeliveryError, DeliveryReceipt};
use super::{AuthenticatedPlanCaller, ServiceContext};
use crate::cron::store::CronJob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleProfile {
    Curator,
}

impl RoleProfile {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "curator" => Some(Self::Curator),
            _ => None,
        }
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Curator => "curator",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum BlueprintAction {
    Input,
    Update,
    Create,
    Resolve,
    Stop,
    Delete,
    Apply,
}
impl BlueprintAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Update => "update",
            Self::Create => "create",
            Self::Resolve => "resolve",
            Self::Stop => "stop",
            Self::Delete => "delete",
            Self::Apply => "apply",
        }
    }
}

pub struct CronLaunch {
    pub provider: String,
    pub model: String,
    pub effort: Option<String>,
    pub task: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunCompletion {
    TurnEnd,
    ExplicitFinish,
}

pub struct CronSettlement {
    pub success: bool,
    pub summary: String,
}

pub trait RolePolicy: Send + Sync {
    fn profile(&self) -> RoleProfile;
    fn spawn_role(&self) -> &'static str;
    fn retain_oneshot(&self) -> bool;
    fn allow_manual_fire(&self) -> bool;
    fn raw_task(&self) -> bool;
    fn retarget_only(&self) -> bool;
    fn conservative_save(&self) -> bool;
    fn authorize_notification(&self, service: &ServiceContext, tab: &str) -> Result<(), String>;
    fn run_completion(&self) -> RunCompletion;
    fn session_claimed(&self, service: &ServiceContext, tab: &str) -> Result<(), String>;
    fn tab_closed(&self, service: &ServiceContext, tab: &str) -> Result<(), String>;
    fn settle_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<CronSettlement, String>;
    fn prepare_cron(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<Option<CronLaunch>, String>;
    fn launch_failed(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        now: u64,
    ) -> Result<(), String>;
    fn spawned(
        &self,
        service: &ServiceContext,
        job: &CronJob,
        tab: &str,
        run: &str,
        now: u64,
    ) -> Result<(), String>;
    fn prepare_message(
        &self,
        service: &ServiceContext,
        sender: &str,
        target: &str,
        body: &str,
    ) -> Result<Option<String>, String>;
    fn message_receipt(
        &self,
        service: &ServiceContext,
        sender: &str,
        receipt: &Result<DeliveryReceipt, DeliveryError>,
    ) -> Result<(), String>;
    fn authorize_blueprint(
        &self,
        service: &ServiceContext,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String>;
    fn learning_owner(
        &self,
        service: &ServiceContext,
        caller: &AuthenticatedPlanCaller,
        plan: &str,
        tag: Option<&str>,
    ) -> Result<(Option<String>, Option<String>), String>;
}

pub fn resolve(value: Option<&str>) -> Option<&'static dyn RolePolicy> {
    match value.and_then(RoleProfile::parse) {
        Some(RoleProfile::Curator) => Some(&super::curator::policy::POLICY),
        None => None,
    }
}

impl ServiceContext {
    pub fn bind_role_session(&self, tab: &str) -> Result<(), String> {
        if let Some(policy) = self.role_policy_for_tab(tab) {
            policy.session_claimed(self, tab)?;
        }
        Ok(())
    }
    pub fn run_completion_for_tab(&self, tab: &str) -> RunCompletion {
        self.role_policy_for_tab(tab)
            .map_or(RunCompletion::TurnEnd, |policy| policy.run_completion())
    }
    pub fn role_policy_for_tab(&self, tab: &str) -> Option<&'static dyn RolePolicy> {
        resolve(self.mux.read().get_tab(tab)?.role_profile.as_deref())
    }
    pub fn authorize_role_blueprint(
        &self,
        tab: &str,
        id: Option<&str>,
        action: BlueprintAction,
    ) -> Result<bool, String> {
        match self.role_policy_for_tab(tab) {
            Some(policy) => policy.authorize_blueprint(self, tab, id, action),
            None => Ok(false),
        }
    }
}

#[cfg(test)]
#[path = "role_policy_tests.rs"]
mod tests;
