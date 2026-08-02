use crate::agent::registry::AgentRegistry;
use crate::multiplexer::types::TabAudience;

#[derive(Clone, Debug)]
pub(crate) struct PermissionBypassAuthorization {
    provider_id: String,
}

impl PermissionBypassAuthorization {
    pub(crate) fn authorizes(&self, provider_id: &str, audience: TabAudience) -> bool {
        self.provider_id == provider_id && audience != TabAudience::Automation
    }
}

pub(crate) fn authorize_permission_bypass(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: bool,
    hidden: bool,
    audience: TabAudience,
) -> Option<PermissionBypassAuthorization> {
    (requested
        && !hidden
        && audience != TabAudience::Automation
        && registry
            .with_provider(provider_id, |provider| {
                !provider.auto_approve_flags().is_empty()
            })
            .unwrap_or(false))
    .then(|| PermissionBypassAuthorization {
        provider_id: provider_id.to_owned(),
    })
}

pub(crate) fn permission_bypass_allowed(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: bool,
    hidden: bool,
    audience: TabAudience,
) -> bool {
    authorize_permission_bypass(registry, provider_id, requested, hidden, audience).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_claude() -> AgentRegistry {
        let registry = AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::plugin::agent_claude::ClaudeProvider::new(),
        ));
        registry
    }

    #[test]
    fn permission_bypass_requires_visible_user_and_provider_capability() {
        let registry = registry_with_claude();
        assert!(permission_bypass_allowed(
            &registry,
            "claude",
            true,
            false,
            TabAudience::User,
        ));
        assert!(!permission_bypass_allowed(
            &registry,
            "claude",
            true,
            true,
            TabAudience::User,
        ));
        assert!(!permission_bypass_allowed(
            &registry,
            "claude",
            true,
            false,
            TabAudience::Automation,
        ));
        assert!(!permission_bypass_allowed(
            &registry,
            "missing-provider",
            true,
            false,
            TabAudience::User,
        ));
    }

    #[test]
    fn restored_hidden_and_automation_launches_cannot_receive_authorization() {
        let registry = registry_with_claude();
        assert!(authorize_permission_bypass(
            &registry,
            "claude",
            true,
            true,
            TabAudience::User,
        )
        .is_none());
        assert!(authorize_permission_bypass(
            &registry,
            "claude",
            true,
            false,
            TabAudience::Automation,
        )
        .is_none());
        let authorization = authorize_permission_bypass(
            &registry,
            "claude",
            true,
            false,
            TabAudience::User,
        )
        .expect("visible capable provider is authorized");
        assert!(!authorization.authorizes("different-provider", TabAudience::User));
    }
}
