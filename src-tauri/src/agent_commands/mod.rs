//! `agent_commands` — the inverse seam. A spawned agent runs
//! `codeterm plugin <id> <verb> [args…]`; the socket dispatch routes
//! `plugin:<id>.<verb>` to the owning plugin's `onAgentCommand` handler, gated by
//! the plugin's `agent_commands` grant (plan §1.5 F5 / §1.10).
//!
//! There is no native handler — the capability is JS-only, so the acceptance
//! oracle is a `call_json` contract test (see `plugin::js_runtime::agent_commands`)
//! plus the routing test in `socket::server::agent_commands`, not `parity_test.rs`.
//!
//! Core asks the capability, never the identity — resolution goes through
//! [`AgentCommandRegistry`] (a
//! [`CapabilityRegistry`](crate::plugin::capability::CapabilityRegistry)).

use std::sync::Arc;

use crate::plugin::capability::CapabilityRegistry;

/// A plugin that handles agent-issued commands via its exported `onAgentCommand`.
/// Synchronous because the QuickJS `call_json` bridge is synchronous (the VM is
/// not `Send`); the socket dispatch calls it on the request thread.
pub trait AgentCommandProvider: Send + Sync {
    fn id(&self) -> &str;

    /// Whether the plugin holds the `agent_commands` grant. The dispatch refuses
    /// to route to an ungranted plugin even if a handler is registered.
    fn granted(&self) -> bool;

    /// Route one agent-issued command to the plugin. `Ok(result)` becomes the
    /// plugin's `{ result }`; `Err(msg)` becomes its `{ error }` (or surfaces a
    /// VM failure). Both are non-fatal at the RPC layer — the call succeeded, the
    /// plugin reported an outcome.
    fn on_agent_command(
        &self,
        session_id: &str,
        verb: &str,
        args: &[String],
    ) -> Result<String, String>;
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
        ) -> Result<String, String> {
            Ok(format!("{session_id}:{verb}:{}", args.join(",")))
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
            "s1:say:hi"
        );
        assert!(reg.get("missing").is_none());
    }
}
