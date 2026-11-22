use super::*;
use crate::multiplexer::types::Tab;
use codeterm_plugin::provider_account::InvocationContext;
use std::sync::Arc;

#[derive(Clone)]
pub(super) struct LaunchAccount {
    provider: String,
    resume: Option<String>,
    expected: Arc<parking_lot::Mutex<Tab>>,
    switch_launch: Option<Arc<crate::service::messenger::switch_handoff::SwitchLaunchAuthority>>,
}

fn exact_resume_source(svc: &ServiceContext, tab: &Tab, provider: &str, resume: &str) -> bool {
    if tab.agent_type.as_deref() == Some(provider)
        && tab.agent_session_id.as_deref() == Some(resume)
    {
        return true;
    }
    let retained = tab
        .prior_session_binding_nonces
        .last()
        .is_some_and(|prior| {
            prior
                .session_id
                .as_deref()
                .or(prior.resume_target_session_id.as_deref())
                == Some(resume)
        });
    if !retained {
        return false;
    }
    if tab
        .restorable_launch
        .exited_chat_binding
        .as_ref()
        .is_some_and(|exit| {
            exit.provider_id == provider
                && exit.session_id == resume
                && tab.prior_session_binding_nonces.iter().any(|prior| {
                    prior.nonce == exit.nonce && prior.session_id.as_deref() == Some(resume)
                })
        })
    {
        return true;
    }
    let journal = svc.lifecycle_journal.load();
    if !journal.tail.is_intact() {
        return false;
    }
    crate::session::lifecycle::checkpoint_history(&journal, &tab.id)
        .iter()
        .any(|checkpoint| {
            let authority = &checkpoint.authority;
            authority.provider == provider
                && authority.session_id == resume
                && authority.role == tab.role.map(|role| format!("{role:?}"))
                && authority.role_profile == tab.role_profile
                && authority.team_id == tab.team_id
                && authority.parent_tab_id == tab.parent_tab_id
                && tab.prior_session_binding_nonces.iter().any(|prior| {
                    prior.nonce == authority.binding_nonce
                        && prior.session_id.as_deref() == Some(resume)
                })
        })
}

pub(super) fn prepare(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    resume: Option<&str>,
) -> Result<(LaunchAccount, Option<String>), String> {
    let switch_launch = crate::service::messenger::switch_handoff::launch_authority(svc, tab);
    if let Some(authority) = &switch_launch {
        authority.validate(svc)?;
    }
    let source = match &switch_launch {
        Some(authority) => authority.snapshot(),
        None => svc
            .mux
            .read()
            .get_tab(tab)
            .cloned()
            .ok_or("launch_account_source_unavailable")?,
    };
    if source.frozen
        || source
            .agent_type
            .as_deref()
            .is_some_and(|current| current != provider)
        || match resume {
            Some(resume) => !exact_resume_source(svc, &source, provider, resume),
            None => source.agent_type.as_deref() != Some(provider),
        }
    {
        return Err(
            "launch_account_source_changed: exact provider/resume authority is unavailable".into(),
        );
    }
    let (reserved, expected) =
        svc.reserve_agent_launch_binding_nonce_if_current(&source, resume, None)?;
    if let Some(authority) = &switch_launch {
        authority.reserved(expected.clone());
    }
    let account = LaunchAccount {
        provider: provider.into(),
        resume: resume.map(str::to_owned),
        expected: Arc::new(parking_lot::Mutex::new(expected)),
        switch_launch,
    };
    account.validate(svc, tab)?;
    if let Some(session) = resume.filter(|_| reserved.is_some()) {
        crate::service::launch_session_detection::register_resume_launch_marker(tab, session);
    }
    Ok((account, reserved))
}

impl LaunchAccount {
    pub(super) fn reserve_replacement(
        &self,
        svc: &ServiceContext,
        launch: &LaunchGeneration,
    ) -> Result<String, String> {
        let source = self.expected.lock().clone();
        let registry = LAUNCH_REGISTRY.lock();
        if !registry.owns(launch) {
            return Err("launch_account_source_changed: replacement generation is obsolete".into());
        }
        let (nonce, expected) = svc.reserve_agent_launch_binding_nonce_if_current(
            &source,
            self.resume.as_deref(),
            Some(&launch.pty_id),
        )?;
        let nonce = nonce.ok_or(
            "launch_account_replacement_unnecessary: current launch already authenticated",
        )?;
        if let Some(authority) = &self.switch_launch {
            authority.reserved(expected.clone());
        }
        *self.expected.lock() = expected;
        drop(registry);
        Ok(nonce)
    }

    pub(super) fn fail_if_current(
        &self,
        launch: &LaunchGeneration,
        svc: &ServiceContext,
        failure: LaunchFailure,
    ) -> bool {
        let bound = svc.mux.read().get_tab(&launch.tab_id).is_some_and(|tab| {
            tab.launch_session_authenticated() && tab.boot_stage == Some(BootStage::AgentReady)
        });
        if (bound && !failure.proves_process_gone()) || self.validate(svc, &launch.tab_id).is_err()
        {
            LAUNCH_REGISTRY.lock().finish(launch);
            return false;
        }
        launch.fail(svc, failure)
    }

    pub(super) fn validate(&self, svc: &ServiceContext, tab: &str) -> Result<(), String> {
        if crate::crash::shutdown::is_shutting_down() {
            return Err("launch_account_source_changed: service is shutting down".into());
        }
        let mux = svc.mux.read();
        let current = mux
            .get_tab(tab)
            .ok_or("launch_account_source_unavailable")?;
        self.validate_snapshot(current)
    }

    fn validate_snapshot(&self, current: &Tab) -> Result<(), String> {
        if self
            .switch_launch
            .as_ref()
            .is_some_and(|authority| !authority.matches(current))
        {
            return Err("launch_account_source_changed: admitted switch launch was revoked".into());
        }
        let expected = self.expected.lock();
        if current.session_binding_nonce.is_none()
            || current.session_binding_nonce != expected.session_binding_nonce
            || current.lifecycle_epoch != expected.lifecycle_epoch
            || current.lifecycle_operation_id != expected.lifecycle_operation_id
            || current.restorable_launch.account_id != expected.restorable_launch.account_id
            || current.restorable_launch.account_namespace
                != expected.restorable_launch.account_namespace
            || current.team_id != expected.team_id
            || current.parent_tab_id != expected.parent_tab_id
            || current.role != expected.role
            || current.role_profile != expected.role_profile
            || current.task_binding != expected.task_binding
            || current.cwd != expected.cwd
            || current.can_spawn != expected.can_spawn
            || current.depth != expected.depth
            || current.report_target_tab_id != expected.report_target_tab_id
            || current.report_target_mesh_peer != expected.report_target_mesh_peer
            || current.spawned_by_tab_id != expected.spawned_by_tab_id
            || current.ownership != expected.ownership
            || current.neighbor_authority != expected.neighbor_authority
            || current.backing != expected.backing
            || current.frozen
            || current
                .agent_type
                .as_deref()
                .is_some_and(|provider| provider != self.provider)
            || current.agent_session_id.as_deref().is_some_and(|session| {
                Some(session) != self.resume.as_deref()
                    && Some(session) != expected.agent_session_id.as_deref()
            })
            || (current.agent_type.is_none() && self.resume.is_none())
        {
            return Err("launch_account_source_changed: provider, account, nonce or lifecycle authority changed".into());
        }
        Ok(())
    }

    pub(super) fn set_booting(&self, svc: &ServiceContext, tab: &str, value: bool) {
        if let Some(current) = svc.mux.write().get_tab_mut(tab) {
            if self.validate_snapshot(current).is_ok() {
                current.agent_booting = value;
                if value {
                    current.restored_from_snapshot = false;
                    current.restorable_launch.failed_provider = None;
                }
            }
        }
    }

    pub(super) fn resolve_with(
        &self,
        svc: &ServiceContext,
        tab: &str,
        provision: impl FnOnce() -> Result<Option<Arc<InvocationContext>>, String>,
    ) -> Result<Option<Arc<InvocationContext>>, String> {
        self.validate(svc, tab)?;
        let invocation = provision()?;
        self.validate(svc, tab)?;
        if !crate::service::provider_accounts::invocation_matches(
            invocation.as_deref(),
            &self.provider,
            crate::service::provider_accounts::selected_account(&self.expected.lock()),
        ) {
            return Err(
                "launch_account_context_changed: selected account context does not match".into(),
            );
        }
        Ok(invocation)
    }

    pub(super) fn resolve_bounded(
        &self,
        svc: &ServiceContext,
        tab: &str,
        provided: Option<Option<Arc<InvocationContext>>>,
    ) -> Result<Option<Arc<InvocationContext>>, String> {
        crate::startup::boot_timing::mark(tab, None, "account_supervision_begin");
        let owner = svc.clone();
        let tab = tab.to_owned();
        let account = self.clone();
        let provision = move || {
            let (id, saved_namespace) = {
                let expected = account.expected.lock();
                (
                    crate::service::provider_accounts::selected_account(&expected).to_owned(),
                    expected.restorable_launch.account_namespace.clone(),
                )
            };
            let namespace = crate::service::provider_accounts::credential_namespace(
                &owner,
                &account.provider,
                Some(&id),
                saved_namespace.as_deref(),
            )?;
            let resolved = crate::service::provider_accounts::invocation_at(
                &owner,
                &account.provider,
                Some(&id),
                namespace.as_deref(),
            )?;
            let current_namespace = crate::service::provider_accounts::credential_namespace(
                &owner,
                &account.provider,
                Some(&id),
                saved_namespace.as_deref(),
            )?;
            if namespace != current_namespace
                || provided.as_ref().is_some_and(|provided| {
                    provided.as_ref().map(|context| context.cache_key())
                        != resolved.as_ref().map(|context| context.cache_key())
                })
            {
                return Err("launch_account_context_changed: credential namespace changed".into());
            }
            Ok(resolved)
        };
        let result = if crate::service::provider_accounts::provisioning::in_startup(svc, &tab)
            && crate::service::provider_accounts::selected_account(&self.expected.lock())
                == codeterm_types::config::provider_accounts::IMPLICIT_ACCOUNT
        {
            self.resolve_with(svc, &tab, provision)
        } else {
            self.start_resolution(svc, &tab, provision)
                .and_then(|job| job.wait_blocking(std::time::Duration::from_secs(60)))
        };
        crate::startup::boot_timing::mark(&tab, None, "account_supervision_settled");
        result
    }

    pub(super) fn start_resolution(
        &self,
        svc: &ServiceContext,
        tab: &str,
        provision: impl FnOnce() -> Result<Option<Arc<InvocationContext>>, String> + Send + 'static,
    ) -> Result<crate::service::provider_accounts::provisioning::Observation, String> {
        self.validate(svc, tab)?;
        let owner = svc.clone();
        let account = self.clone();
        let tab = tab.to_owned();
        crate::service::provider_accounts::provisioning::start(svc, move || {
            account.resolve_with(&owner, &tab, provision)
        })
    }
}

#[cfg(test)]
pub(crate) fn reserve_launch_binding_nonce(
    svc: &ServiceContext,
    tab_id: &str,
    resume_session_id: Option<&str>,
) -> Option<String> {
    let authenticated = svc
        .mux
        .read()
        .get_tab(tab_id)
        .is_some_and(|tab| tab.launch_session_authenticated());
    (!authenticated).then(|| generation::reserve_binding_nonce(svc, tab_id, resume_session_id))
}

pub fn mark_tab_agent_launch_started(mux: &crate::multiplexer::commands::MuxState, tab_id: &str) {
    if let Some(tab) = mux.write().get_tab_mut(tab_id) {
        tab.restored_from_snapshot = false;
        tab.agent_booting = true;
        tab.restorable_launch.failed_provider = None;
    }
}
