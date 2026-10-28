//! Host API audit: `call` requires `surfaces` and a host-scoped onAgentCommand caller;
//! core resolves ownership, declared verbs and attribution before returning a view dispatch.
//! `invalidate` needs the same grant plus an opted-in declaration and exact live instance.
//! It accepts at most 1024 bytes of identity only, never FS paths, command data or credentials.

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

pub(crate) fn surface_invalidate(sandbox: &PluginSandbox, identity_json: &str) -> String {
    if !sandbox.surfaces_allowed() {
        return err(SURFACE_DENIED);
    }
    if identity_json.len() > 1024 {
        return err("surface invalidation exceeds 1024 bytes");
    }
    match sandbox.service() {
        Some(service) => service.surface_invalidate_json(sandbox.plugin_id(), identity_json),
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
    .map_err(|e| VmError::Script(format!("set surfaceCall: {e}")))?;
    let sb = sandbox.clone();
    host.set(
        "surfaceInvalidate",
        Func::new(move |identity_json: String| -> String {
            surface_invalidate(&sb, &identity_json)
        }),
    )
    .map_err(|e| VmError::Script(format!("set surfaceInvalidate: {e}")))
}

#[cfg(test)]
mod tests;
