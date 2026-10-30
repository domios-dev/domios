//! Host API audit: `call` requires `surfaces` and a host-scoped caller: the agent of the running
//! onAgentCommand, or the run owner (`request.context.owner`, the Blueprint creator tab stamped by
//! the pipeline) while JsBlueprintStep runs `blueprintSubmit`. Reconcile, cancel and every other
//! entry point have no caller. The plugin can never name the caller; core resolves ownership,
//! declared verbs and attribution before returning a view dispatch.
//! A step receives a separate surface on open; every later call supplies that exact tab and
//! instance, checked against the host-owned submit scope. Owner surfaces are never adopted.
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
    "host.surface.call acts for the calling agent and is only available inside onAgentCommand or a Blueprint step submit";

thread_local! {
    static AGENT_CALLER: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Core-stamped caller of the running `onAgentCommand` or step submit; the plugin cannot name another.
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
        Some(svc) => match crate::blueprint_step::current_step_input(sandbox.plugin_id()) {
            Some(context) if context.submitting => {
                svc.step_surface_call_json(sandbox.plugin_id(), &context, verb, args_json)
            }
            _ => svc.surface_call_json(sandbox.plugin_id(), &caller, verb, args_json),
        },
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
    host.set(
        "awaitInvocationId",
        Func::new(crate::surface_invocation::root_identity),
    )
    .map_err(|_| VmError::Script("set awaitInvocationId failed".into()))?;
    host.set(
        "awaitTakeMode",
        Func::new(|key: String, job: String, owner: String| {
            crate::surface_invocation::take_mode(&key, &job, &owner)
        }),
    )
    .map_err(|_| VmError::Script("set awaitTakeMode failed".into()))?;
    host.set(
        "surfaceInvocationBegin",
        Func::new(|id: String| crate::surface_invocation::begin(&id)),
    )
    .map_err(|_| VmError::Script("set surfaceInvocationBegin failed".into()))?;
    host.set(
        "surfaceInvocationEnd",
        Func::new(|id: String, suspended: bool| crate::surface_invocation::end(&id, suspended)),
    )
    .map_err(|_| VmError::Script("set surfaceInvocationEnd failed".into()))?;
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
mod continuation_tests;
#[cfg(test)]
pub(crate) mod step_tests;
#[cfg(test)]
mod tests;
