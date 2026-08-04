use crate::agent::registry::AgentRegistry;
#[derive(Clone, Debug)]
pub(crate) struct PermissionBypassAuthorization {
    provider_id: String,
}

impl PermissionBypassAuthorization {
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

pub(crate) fn configured_full_access() -> Result<bool, String> {
    require_full_access_choice(crate::config::load_config().agent.full_access)
}

fn require_full_access_choice(choice: Option<bool>) -> Result<bool, String> {
    choice.ok_or_else(|| {
        "Choose agent access before starting the first agent Tab: Settings > Agents > Full access for agents"
            .to_string()
    })
}

pub(crate) fn configured_permission_bypass(
    registry: &AgentRegistry,
    provider_id: &str,
) -> Result<Option<PermissionBypassAuthorization>, String> {
    Ok(authorize_permission_bypass(
        registry,
        provider_id,
        configured_full_access()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_claude() -> AgentRegistry {
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(crate::plugin::agent_claude::ClaudeProvider::new()));
        registry
    }

    #[test]
    fn permission_bypass_requires_request_and_provider_capability() {
        let registry = registry_with_claude();
        assert!(authorize_permission_bypass(&registry, "claude", true).is_some());
        assert!(authorize_permission_bypass(&registry, "claude", false).is_none());
        assert!(authorize_permission_bypass(&registry, "missing-provider", true).is_none());
    }

    #[test]
    fn authorization_is_provider_bound() {
        let registry = registry_with_claude();
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
}
