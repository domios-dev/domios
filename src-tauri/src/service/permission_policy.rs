use crate::agent::registry::AgentRegistry;
use crate::multiplexer::types::TabAudience;

pub(crate) fn permission_bypass_allowed(
    registry: &AgentRegistry,
    provider_id: &str,
    requested: bool,
    hidden: bool,
    audience: TabAudience,
) -> bool {
    requested
        && !hidden
        && audience != TabAudience::Automation
        && registry
            .with_provider(provider_id, |provider| {
                !provider.auto_approve_flags().is_empty()
            })
            .unwrap_or(false)
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
}
