use std::cell::RefCell;

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
    serde_json::to_string(&codeterm_wire::plugin_host::HostError {
        error: message.into(),
    })
    .unwrap_or_else(|_| "null".into())
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

pub(super) fn install(host: &Object<'_>) -> Result<(), VmError> {
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
    Ok(())
}

#[cfg(test)]
mod continuation_tests;
#[cfg(test)]
pub(crate) mod step_tests;
#[cfg(test)]
mod tests;
