use super::launch_profile_install as profiles;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::AgentRuntimeCommand;
use parking_lot::Mutex;
use serde::Serialize;
use std::{collections::BTreeMap, sync::LazyLock};

const MAX_JOBS: usize = 256;
const MAX_ACTIVE: usize = 4;
pub const EVENT: &str = "provider-profile-setup";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupState {
    Accepted,
    Installing,
    Installed,
    Failed,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupJob {
    pub job_id: String,
    pub provider_id: String,
    pub profile_id: String,
    pub state: SetupState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
impl SetupJob {
    pub fn active(&self) -> bool {
        matches!(self.state, SetupState::Accepted | SetupState::Installing)
    }
}

#[derive(Default)]
struct Jobs(BTreeMap<(String, String), SetupJob>);
impl Jobs {
    fn reserve(&mut self, provider: &str, profile: &str) -> Result<SetupJob, String> {
        let key = (provider.to_owned(), profile.to_owned());
        if self.0.get(&key).is_some_and(SetupJob::active) {
            return Err("Profile setup is already in progress".into());
        }
        if self.0.values().filter(|job| job.active()).count() >= MAX_ACTIVE {
            return Err("Profile installers are busy; wait for a setup completion".into());
        }
        if self.0.len() >= MAX_JOBS && !self.0.contains_key(&key) {
            let expired = self
                .0
                .iter()
                .find(|(_, job)| !job.active())
                .map(|(key, _)| key.clone())
                .ok_or("Profile setup capacity exceeded")?;
            self.0.remove(&expired);
        }
        let job = SetupJob {
            job_id: uuid::Uuid::new_v4().to_string(),
            provider_id: provider.into(),
            profile_id: profile.into(),
            state: SetupState::Accepted,
            error: None,
        };
        self.0.insert(key, job.clone());
        Ok(job)
    }

    fn advance(
        &mut self,
        job: &SetupJob,
        state: SetupState,
        error: Option<String>,
    ) -> Option<SetupJob> {
        let current = self
            .0
            .get_mut(&(job.provider_id.clone(), job.profile_id.clone()))?;
        if current.job_id != job.job_id || !current.active() {
            return None;
        }
        current.state = state;
        current.error = error;
        Some(current.clone())
    }
}
static JOBS: LazyLock<Mutex<Jobs>> = LazyLock::new(Default::default);
static MUTATION: LazyLock<Mutex<()>> = LazyLock::new(Default::default);

pub(super) fn mutation() -> Result<parking_lot::MutexGuard<'static, ()>, String> {
    MUTATION
        .try_lock()
        .ok_or("Profile change already in progress".into())
}

pub fn current(provider: &str, profile: &str) -> Option<SetupJob> {
    JOBS.lock()
        .0
        .get(&(provider.to_owned(), profile.to_owned()))
        .cloned()
}

pub fn for_provider(provider: &str) -> Vec<SetupJob> {
    JOBS.lock()
        .0
        .values()
        .filter(|job| job.provider_id == provider)
        .cloned()
        .collect()
}

pub(super) fn forget_finished(provider: &str, id: &str) {
    let key = (provider.to_owned(), id.to_owned());
    let mut jobs = JOBS.lock();
    if jobs.0.get(&key).is_some_and(|job| !job.active()) {
        jobs.0.remove(&key);
    }
}

fn advance(svc: &ServiceContext, job: &SetupJob, state: SetupState, error: Option<String>) {
    let updated = JOBS.lock().advance(job, state, error);
    if let Some(updated) = updated {
        svc.ui.emit(EVENT, serde_json::json!(updated));
    }
}

pub fn begin(
    svc: &ServiceContext,
    provider: String,
    id: String,
    command: Option<AgentRuntimeCommand>,
) -> Result<SetupJob, String> {
    let _mutation = mutation()?;
    let profile = profiles::declaration(svc, &provider, &id)?;
    let root = profiles::profile_root(&provider)?;
    if command.is_none() && profile.install.is_none() {
        return Err("Profile has no supported installer".into());
    }
    if command.is_some() && !profile.owner_configured {
        return Err("Only owner-configured profiles accept a local command".into());
    }
    start(svc, provider, id.clone(), move || {
        if let Some(command) = command {
            profiles::confirm_local(&root, &id, &profile, command, true)
        } else {
            crate::async_runtime::block_on(profiles::install(&root, &id, &profile, true))
        }
        .map(|_| ())
    })
}

pub(super) fn start(
    svc: &ServiceContext,
    provider: String,
    id: String,
    operation: impl FnOnce() -> Result<(), String> + Send + 'static,
) -> Result<SetupJob, String> {
    let job = JOBS.lock().reserve(&provider, &id)?;
    svc.ui.emit(EVENT, serde_json::json!(job));
    let accepted = job.clone();
    let svc = svc.clone();
    crate::async_runtime::spawn(async move {
        advance(&svc, &job, SetupState::Installing, None);
        let result = tokio::task::spawn_blocking(operation).await;
        match result {
            Ok(Ok(_)) => advance(&svc, &job, SetupState::Installed, None),
            Ok(Err(error)) => advance(&svc, &job, SetupState::Failed, Some(error)),
            Err(_) => advance(
                &svc,
                &job,
                SetupState::Failed,
                Some("Profile setup worker failed".into()),
            ),
        }
    });
    Ok(accepted)
}

#[cfg(test)]
mod tests;
