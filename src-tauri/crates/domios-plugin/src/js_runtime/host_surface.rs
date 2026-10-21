//! Sandbox audit: no FS, env or network reach; it acts only inside `onAgentCommand`, on the
//! calling agent's own stream surface of this plugin, and only with the `surfaces` grant.

use std::cell::RefCell;
use std::sync::Arc;

use rquickjs::function::Func;
use rquickjs::Object;

use super::host_api::PluginSandbox;
use super::vm::VmError;

pub(crate) const SURFACE_DENIED: &str = "host.surface: surfaces permission required";
pub(crate) const NO_AGENT_CALLER: &str =
    "host.surface.call acts for the calling agent and is only available inside onAgentCommand";

thread_local! {
    static AGENT_CALLER: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Core-stamped caller of the running `onAgentCommand`; the plugin cannot name another agent.
pub(crate) struct AgentCallerScope {
    previous: Option<String>,
}

impl AgentCallerScope {
    pub(crate) fn enter(caller: &str) -> Self {
        let caller = Some(caller.to_string()).filter(|caller| !caller.trim().is_empty());
        Self {
            previous: AGENT_CALLER.with(|scope| scope.replace(caller)),
        }
    }
}

impl Drop for AgentCallerScope {
    fn drop(&mut self) {
        AGENT_CALLER.with(|scope| *scope.borrow_mut() = self.previous.take());
    }
}

pub(crate) fn current_agent_caller() -> Option<String> {
    AGENT_CALLER.with(|scope| scope.borrow().clone())
}

fn err(message: &str) -> String {
    serde_json::json!({ "error": message }).to_string()
}

pub(crate) fn surface_call(sandbox: &PluginSandbox, verb: &str, args_json: &str) -> String {
    if !sandbox.surfaces_allowed() {
        return err(SURFACE_DENIED);
    }
    let Some(caller) = current_agent_caller() else {
        return err(NO_AGENT_CALLER);
    };
    match sandbox.service() {
        Some(svc) => svc.surface_call_json(sandbox.plugin_id(), &caller, verb, args_json),
        None => err("host service unavailable"),
    }
}

pub(super) fn install(host: &Object<'_>, sandbox: &Arc<PluginSandbox>) -> Result<(), VmError> {
    let sb = sandbox.clone();
    host.set(
        "surfaceCall",
        Func::new(move |verb: String, args_json: String| -> String {
            surface_call(&sb, &verb, &args_json)
        }),
    )
    .map_err(|e| VmError::Script(format!("set surfaceCall: {e}")))
}

#[cfg(test)]
mod tests;
