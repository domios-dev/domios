use crate::agent::registry::AgentRegistry;
#[derive(Clone, Debug)]
pub struct PermissionBypassAuthorization {
    provider_id: String,
}

impl PermissionBypassAuthorization {
    #[cfg(test)]
    pub(crate) fn for_test(provider: &str) -> Self {
        Self {
            provider_id: provider.into(),
        }
    }
    pub(crate) fn authorizes(&self, provider_id: &str) -> bool {
        self.provider_id == provider_id
    }
}

pub(crate) fn authorize_permission_bypass(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: bool,
) -> Option<PermissionBypassAuthorization> {
    (requested
        && registry
            .with_provider(provider_id, |provider| {
                !provider.auto_approve_flags().is_empty()
                    || provider
                        .agent_runtime()
                        .is_some_and(|runtime| runtime.host_approvals)
            })
            .unwrap_or(false))
    .then(|| PermissionBypassAuthorization {
        provider_id: provider_id.to_owned(),
    })
}

pub fn configured_permission_bypass_for(
    _svc: &super::ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    Ok(authorize_permission_bypass(registry, provider_id, true))
}

pub(crate) fn permission_bypass_for_tab(
    svc: &super::ServiceContext,
    provider: &str,
    tab_id: &str,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    let launch = svc
        .mux
        .read()
        .get_tab(tab_id)
        .ok_or("Agent tab unavailable")?
        .restorable_launch
        .clone();
    super::restored_launch::restore_permission_bypass_authorization(
        svc,
        &svc.agent_registry,
        tab_id,
        provider,
        &launch,
    )
}

pub(crate) fn spawn_permission_bypass_for(
    _svc: &super::ServiceContext,
    registry: &AgentRegistry,
    provider: &str,
    _worker: bool,
    confined: bool,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    Ok(authorize_permission_bypass(registry, provider, !confined))
}

pub(crate) fn spawn_permission_bypass_for_params(
    svc: &super::ServiceContext,
    provider: &str,
    worker: bool,
    params: &super::SpawnAgentParams,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    if params.neighbor_authority.is_some() {
        return Ok(params
            .permission_bypass_authorization
            .clone()
            .filter(|_| params.permission_bypass_requested(provider)));
    }
    spawn_permission_bypass_for(
        svc,
        &svc.agent_registry,
        provider,
        worker,
        params.sandbox.is_some(),
    )
}

pub(crate) fn spawn_permission_bypass_for_launch(
    svc: &super::ServiceContext,
    provider: &str,
    worker: bool,
    confined: bool,
    params: Option<&super::SpawnAgentParams>,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    match params.filter(|params| params.neighbor_authority.is_some()) {
        Some(params) => spawn_permission_bypass_for_params(svc, provider, worker, params),
        None => spawn_permission_bypass_for(svc, &svc.agent_registry, provider, worker, confined),
    }
}

#[cfg(test)]
mod tests;
