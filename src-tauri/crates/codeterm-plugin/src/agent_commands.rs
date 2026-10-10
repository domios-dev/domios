//! `agent_commands` — the inverse seam: a spawned agent's `codeterm plugin <id>
//! <verb>` is routed to the owning plugin's `onAgentCommand`, gated by its
//! `agent_commands` grant. JS-only (acceptance oracle is a `call_json` contract
//! test + the routing test), resolved via [`AgentCommandRegistry`].

use std::sync::Arc;

use crate::agent_verbs::AgentVerbCatalog;
use crate::capability::CapabilityRegistry;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentCommandResult {
    pub result: String,
    pub catalogue_changed: bool,
}

/// A plugin handling agent-issued commands via `onAgentCommand`. Synchronous (the `call_json` bridge is), called on the request thread.
pub trait AgentCommandProvider: Send + Sync {
    fn id(&self) -> &str;

    /// Whether the plugin holds the `agent_commands` grant. The dispatch refuses
    /// to route to an ungranted plugin even if a handler is registered.
    fn granted(&self) -> bool;

    /// The verbs this plugin exposes to agents, for discovery surfaces.
    fn catalog(&self) -> Option<AgentVerbCatalog> {
        None
    }

    /// Route one agent command to the plugin and report whether its catalogue changed.
    fn on_agent_command(
        &self,
        session_id: &str,
        verb: &str,
        args: &[String],
    ) -> Result<AgentCommandResult, String>;
}

pub type AgentCommandRegistry = CapabilityRegistry<dyn AgentCommandProvider>;
pub type AgentCommandRegistryHandle = Arc<AgentCommandRegistry>;

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        id: String,
        granted: bool,
    }
    impl AgentCommandProvider for Fake {
        fn id(&self) -> &str {
            &self.id
        }
        fn granted(&self) -> bool {
            self.granted
        }
        fn on_agent_command(
            &self,
            session_id: &str,
            verb: &str,
            args: &[String],
        ) -> Result<AgentCommandResult, String> {
            Ok(AgentCommandResult {
                result: format!("{session_id}:{verb}:{}", args.join(",")),
                catalogue_changed: false,
            })
        }
    }

    #[test]
    fn registry_resolves_provider() {
        let reg: AgentCommandRegistry = AgentCommandRegistry::new();
        reg.register(
            "local-shell",
            Arc::new(Fake {
                id: "local-shell".into(),
                granted: true,
            }),
        );
        let p = reg.get("local-shell").expect("registered");
        assert_eq!(p.id(), "local-shell");
        assert!(p.granted());
        assert_eq!(
            p.on_agent_command("s1", "say", &["hi".into()]).unwrap(),
            AgentCommandResult {
                result: "s1:say:hi".into(),
                catalogue_changed: false,
            }
        );
        assert!(reg.get("missing").is_none());
    }
}
