use std::sync::{Arc, OnceLock};

use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::Tab;
use crate::service::agent_lifecycle::{
    AgentRestartError, ModelSwitchError, ModelSwitchOutcome, RestartAttemptError, RestartOptions,
    RestartOutcome, TabRestartSnapshot, WorkerInterruptionEvent,
};
use crate::service::extensions::{Extensions, ExtensionsBuilder};
use crate::service::launch_readiness::LaunchAttempt;
use crate::service::plan::PlanError;
use crate::service::ServiceState;

/// A prepared model-switch confirmation watch: started once its payload is queued,
/// retired when dropped.
pub trait SwitchConfirmationWatch: Send {
    fn start(self: Box<Self>);
}

/// Calls the core makes up into agent restart, model switch, exited-member restore and
/// worker interruption; each method forwards one lifecycle method unchanged.
pub trait AgentLifecyclePort: Send + Sync {
    fn agent_restart(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError>;
    fn agent_restart_observed_with_options(
        &self,
        tab_id: &str,
        options: RestartOptions,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn agent_switch_model_with_fallback(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
        force: bool,
        restart_fallback: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError>;
    fn prepare_switch_confirmation(
        &self,
        pty: &str,
        text: &str,
        picker_selection: Option<&str>,
    ) -> Option<Box<dyn SwitchConfirmationWatch>>;
    fn provider_switch_notice(&self, tab: &str) -> Option<ChatMsg>;
    fn recover_exited_provider_after_launch_exit(
        &self,
        attempt: &LaunchAttempt,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn recover_general_agent_provider_exit(
        &self,
        exited: &Tab,
    ) -> Result<RestartOutcome, RestartAttemptError>;
    fn require_fresh_restart_allowed(
        &self,
        snap: &TabRestartSnapshot,
        force: bool,
    ) -> Result<(), AgentRestartError>;
    fn resume_exited_member_on_shell_ready(&self, tab_id: &str);
    fn signal_provider_exhaustion(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError>;
    fn signal_provider_recovery(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError>;
    fn signal_worker_interruption(
        &self,
        tab_id: &str,
        event: WorkerInterruptionEvent,
    ) -> Result<bool, PlanError>;
}

pub type Factory = fn(ServiceState) -> Arc<dyn AgentLifecyclePort>;
pub type ExtensionRegistrar = fn(&mut ExtensionsBuilder);

static FACTORY: OnceLock<(Factory, ExtensionRegistrar)> = OnceLock::new();

/// Per-context slot, filled once right after the context is wrapped in its `Arc`.
#[derive(Default)]
pub(crate) struct InstalledLifecycle(OnceLock<Arc<dyn AgentLifecyclePort>>);

pub fn register_agent_lifecycle(make: Factory, extensions: ExtensionRegistrar) {
    let _ = FACTORY.set((make, extensions));
}

pub(crate) fn register_extensions(builder: &mut ExtensionsBuilder) {
    let (_, extensions) = FACTORY
        .get()
        .expect("agent lifecycle port is registered before the service starts");
    extensions(builder);
}

pub fn install(svc: &ServiceState) {
    let (make, _) = FACTORY
        .get()
        .expect("agent lifecycle port is registered before the service starts");
    fill(svc, make(svc.clone()));
}

fn fill(svc: &ServiceState, port: Arc<dyn AgentLifecyclePort>) {
    let _ = svc.extensions.get::<InstalledLifecycle>().0.set(port);
}

pub(crate) fn installed(extensions: &Extensions) -> &Arc<dyn AgentLifecyclePort> {
    extensions
        .get::<InstalledLifecycle>()
        .0
        .get()
        .expect("agent lifecycle port is installed after ServiceContext construction")
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
        None => fill(svc, Arc::new(RecordingLifecycle::default())),
    }
}

/// Records each forwarded call by method name and tab or provider; changes no state.
#[cfg(any(test, feature = "test-support"))]
#[derive(Default)]
pub struct RecordingLifecycle {
    pub calls: parking_lot::Mutex<Vec<(&'static str, String)>>,
}

#[cfg(any(test, feature = "test-support"))]
impl RecordingLifecycle {
    fn record(&self, method: &'static str, subject: &str) {
        self.calls.lock().push((method, subject.to_owned()));
    }
}

#[cfg(any(test, feature = "test-support"))]
const NOT_INSTALLED: &str = "agent lifecycle crate is not registered in this fixture";

#[cfg(any(test, feature = "test-support"))]
impl AgentLifecyclePort for RecordingLifecycle {
    fn agent_restart(
        &self,
        tab_id: &str,
        _force: bool,
        _grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError> {
        self.record("agent_restart", tab_id);
        Err(AgentRestartError::Internal(NOT_INSTALLED.into()))
    }

    fn agent_restart_observed_with_options(
        &self,
        tab_id: &str,
        _options: RestartOptions,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.record("agent_restart_observed_with_options", tab_id);
        Err(AgentRestartError::Internal(NOT_INSTALLED.into()).into())
    }

    fn agent_switch_model_with_fallback(
        &self,
        tab_id: &str,
        _requested: &str,
        _requested_effort: Option<&str>,
        _force: bool,
        _restart_fallback: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.record("agent_switch_model_with_fallback", tab_id);
        Err(ModelSwitchError::NotSwitchable(NOT_INSTALLED.into()))
    }

    fn prepare_switch_confirmation(
        &self,
        pty: &str,
        _text: &str,
        _picker_selection: Option<&str>,
    ) -> Option<Box<dyn SwitchConfirmationWatch>> {
        self.record("prepare_switch_confirmation", pty);
        None
    }

    fn provider_switch_notice(&self, tab: &str) -> Option<ChatMsg> {
        self.record("provider_switch_notice", tab);
        None
    }

    fn recover_exited_provider_after_launch_exit(
        &self,
        attempt: &LaunchAttempt,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.record("recover_exited_provider_after_launch_exit", &attempt.tab_id);
        Err(AgentRestartError::Internal(NOT_INSTALLED.into()).into())
    }

    fn recover_general_agent_provider_exit(
        &self,
        exited: &Tab,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.record("recover_general_agent_provider_exit", &exited.id);
        Err(AgentRestartError::Internal(NOT_INSTALLED.into()).into())
    }

    fn require_fresh_restart_allowed(
        &self,
        snap: &TabRestartSnapshot,
        _force: bool,
    ) -> Result<(), AgentRestartError> {
        self.record("require_fresh_restart_allowed", &snap.tab_id);
        Err(AgentRestartError::Internal(NOT_INSTALLED.into()))
    }

    fn resume_exited_member_on_shell_ready(&self, tab_id: &str) {
        self.record("resume_exited_member_on_shell_ready", tab_id);
    }

    fn signal_provider_exhaustion(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError> {
        self.record(
            "signal_provider_exhaustion",
            &format!("{provider}:{}", account.unwrap_or("default")),
        );
        Ok(0)
    }

    fn signal_provider_recovery(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError> {
        self.record(
            "signal_provider_recovery",
            &format!("{provider}:{}", account.unwrap_or("default")),
        );
        Ok(0)
    }

    fn signal_worker_interruption(
        &self,
        tab_id: &str,
        _event: WorkerInterruptionEvent,
    ) -> Result<bool, PlanError> {
        self.record("signal_worker_interruption", tab_id);
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn agent_lifecycle_port_is_object_safe_and_thread_safe() {
        super::super::assert_port::<dyn super::AgentLifecyclePort>();
    }
}
