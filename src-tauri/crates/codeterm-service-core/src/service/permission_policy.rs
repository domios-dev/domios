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
            })
            .unwrap_or(false))
    .then(|| PermissionBypassAuthorization {
        provider_id: provider_id.to_owned(),
    })
}

pub(crate) fn configured_full_access_for(svc: &super::ServiceContext) -> Result<bool, String> {
    let choice = svc
        .try_ext::<crate::config::ConfigState>()
        .map(|config| config.lock().agent.full_access)
        .ok_or_else(|| "Agent configuration state is unavailable".to_string())?;
    require_full_access_choice(choice)
}

pub(crate) fn require_full_access_choice(choice: Option<bool>) -> Result<bool, String> {
    choice.ok_or_else(|| {
        "Choose agent access before starting the first agent Tab: Settings > Agents > Full access for agents"
            .to_string()
    })
}

pub fn configured_permission_bypass_for(
    svc: &super::ServiceContext,
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    Ok(authorize_permission_bypass(
        registry,
        provider_id,
        configured_full_access_for(svc)?,
    ))
}

pub(crate) fn spawn_permission_bypass_for(
    svc: &super::ServiceContext,
    registry: &AgentRegistry,
    provider: &str,
    worker: bool,
    confined: bool,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    let requested = if worker {
        !confined
    } else {
        configured_full_access_for(svc)? && !confined
    };
    Ok(authorize_permission_bypass(registry, provider, requested))
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
mod tests {
    use super::*;

    fn registry_with_capable_provider() -> AgentRegistry {
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        registry
    }

    #[test]
    fn permission_bypass_requires_request_and_provider_capability() {
        let registry = registry_with_capable_provider();
        assert!(authorize_permission_bypass(&registry, "claude", true).is_some());
        assert!(authorize_permission_bypass(&registry, "claude", false).is_none());
        assert!(authorize_permission_bypass(&registry, "missing-provider", true).is_none());
    }

    #[test]
    fn authorization_is_provider_bound() {
        let registry = registry_with_capable_provider();
        let authorization = authorize_permission_bypass(&registry, "claude", true)
            .expect("capable provider is authorized");
        assert!(authorization.authorizes("claude"));
        assert!(!authorization.authorizes("different-provider"));
    }

    #[test]
    fn first_agent_requires_one_global_access_choice() {
        assert!(require_full_access_choice(None).is_err());
        assert!(require_full_access_choice(Some(true)).unwrap());
        assert!(!require_full_access_choice(Some(false)).unwrap());
    }

    #[test]
    fn configured_access_uses_injected_choices_and_refuses_unset() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        assert!(!configured_full_access_for(&fixture.service).unwrap());
        fixture.set_agent_full_access(true);
        assert!(configured_full_access_for(&fixture.service).unwrap());
        fixture
            .service
            .ext::<crate::config::ConfigState>()
            .lock()
            .agent
            .full_access = None;
        assert!(configured_full_access_for(&fixture.service).is_err());
    }

    #[test]
    fn worker_launch_defaults_to_full_access_and_explicit_sandbox_lowers_it() {
        let (mux, _) = crate::multiplexer::state::MultiplexerState::new();
        let fixture = crate::service::test_fixture::ServiceFixture::new(mux);
        fixture.set_agent_full_access(false);
        let registry = registry_with_capable_provider();
        let provider = registry.provider_ids().pop().unwrap();
        assert!(
            spawn_permission_bypass_for(&fixture.service, &registry, &provider, true, false)
                .unwrap()
                .is_some()
        );
        assert!(
            spawn_permission_bypass_for(&fixture.service, &registry, &provider, true, true)
                .unwrap()
                .is_none()
        );
        assert!(
            spawn_permission_bypass_for(&fixture.service, &registry, &provider, false, false)
                .unwrap()
                .is_none()
        );
    }
}
