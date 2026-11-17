use std::sync::{Arc, Weak};

use codeterm_service_core::agent::chat_types::ChatMsg;
use codeterm_service_core::multiplexer::types::Tab;
use codeterm_service_core::ports::lifecycle::{AgentLifecyclePort, SwitchConfirmationWatch};
use codeterm_service_core::service::agent_lifecycle::{
    AgentRestartError, ModelSwitchError, ModelSwitchOutcome, RestartAttemptError, RestartOptions,
    RestartOutcome, TabRestartSnapshot, WorkerInterruptionEvent,
};
use codeterm_service_core::service::launch_readiness::LaunchAttempt;
use codeterm_service_core::service::plan::PlanError;
use codeterm_service_core::service::{ExtensionsBuilder, ServiceContext, ServiceState};

use crate::prelude::*;
use crate::service::agent_restart::switch_confirmation::PreparedSwitchConfirmation;
use crate::service::agent_restart::ProviderSwitchOperations;

/// Weak, because the port lives in the context's own extensions.
struct LifecyclePortImpl(Weak<ServiceContext>);

impl LifecyclePortImpl {
    fn svc(&self) -> ServiceState {
        self.0
            .upgrade()
            .expect("agent lifecycle port outlived its ServiceContext")
    }
}

pub fn register_agent_lifecycle() {
    codeterm_service_core::ports::lifecycle::register_agent_lifecycle(
        |svc| Arc::new(LifecyclePortImpl(Arc::downgrade(&svc))),
        register_extensions,
    );
}

fn register_extensions(extensions: &mut ExtensionsBuilder) {
    extensions.insert_late::<ProviderSwitchOperations>();
}

impl SwitchConfirmationWatch for PreparedSwitchConfirmation {
    fn start(self: Box<Self>) {
        PreparedSwitchConfirmation::start(*self);
    }
}

impl AgentLifecyclePort for LifecyclePortImpl {
    fn agent_restart(
        &self,
        tab_id: &str,
        force: bool,
        grace_secs: u64,
    ) -> Result<RestartOutcome, AgentRestartError> {
        self.svc().agent_restart(tab_id, force, grace_secs)
    }

    fn agent_restart_observed_with_options(
        &self,
        tab_id: &str,
        options: RestartOptions,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.svc()
            .agent_restart_observed_with_options(tab_id, options)
    }

    fn agent_switch_model_with_fallback(
        &self,
        tab_id: &str,
        requested: &str,
        requested_effort: Option<&str>,
        force: bool,
        restart_fallback: bool,
    ) -> Result<ModelSwitchOutcome, ModelSwitchError> {
        self.svc().agent_switch_model_with_fallback(
            tab_id,
            requested,
            requested_effort,
            force,
            restart_fallback,
        )
    }

    fn prepare_switch_confirmation(
        &self,
        pty: &str,
        text: &str,
        picker_selection: Option<&str>,
    ) -> Option<Box<dyn SwitchConfirmationWatch>> {
        self.svc()
            .prepare_switch_confirmation(pty, text, picker_selection)
            .map(|prepared| Box::new(prepared) as Box<dyn SwitchConfirmationWatch>)
    }

    fn provider_switch_notice(&self, tab: &str) -> Option<ChatMsg> {
        self.svc().provider_switch_notice(tab)
    }

    fn recover_exited_provider_after_launch_exit(
        &self,
        attempt: &LaunchAttempt,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.svc()
            .recover_exited_provider_after_launch_exit(attempt)
    }

    fn recover_general_agent_provider_exit(
        &self,
        exited: &Tab,
    ) -> Result<RestartOutcome, RestartAttemptError> {
        self.svc().recover_general_agent_provider_exit(exited)
    }

    fn require_fresh_restart_allowed(
        &self,
        snap: &TabRestartSnapshot,
        force: bool,
    ) -> Result<(), AgentRestartError> {
        self.svc().require_fresh_restart_allowed(snap, force)
    }

    fn resume_exited_member_on_shell_ready(&self, tab_id: &str) {
        self.svc().resume_exited_member_on_shell_ready(tab_id)
    }

    fn signal_provider_exhaustion(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError> {
        self.svc().signal_provider_exhaustion(provider, account)
    }

    fn signal_provider_recovery(
        &self,
        provider: &str,
        account: Option<&str>,
    ) -> Result<usize, PlanError> {
        self.svc().signal_provider_recovery(provider, account)
    }

    fn signal_worker_interruption(
        &self,
        tab_id: &str,
        event: WorkerInterruptionEvent,
    ) -> Result<bool, PlanError> {
        self.svc().signal_worker_interruption(tab_id, event)
    }
}
