//! Agent-run authority is limited to Blueprint steps with spawnAgents. Start receives the host
//! step context; no plugin-selected provider, owner, workspace or Team crosses this boundary.
//! Both launch and waiting run off the VM thread, so provider callbacks can acquire its lock.
use super::{host_api::PluginSandbox, vm::VmError};
use rquickjs::{function::Func, Object};
use std::sync::Arc;

fn error(message: &str) -> String {
    serde_json::json!({"error":message}).to_string()
}

fn context(sandbox: &PluginSandbox) -> Result<crate::blueprint_step::StepInputContext, String> {
    if !sandbox.spawn_agents_allowed() {
        return Err(error("spawnAgents permission required"));
    }
    crate::blueprint_step::current_step_input(sandbox.plugin_id())
        .filter(|scope| {
            !scope.cancellation.is_cancelled() && std::time::Instant::now() < scope.deadline
        })
        .ok_or_else(|| error("agentRun requires a Blueprint step"))
}

fn start(sandbox: &PluginSandbox, opts: &str) -> String {
    if !codeterm_decision::physical::effects_allowed() {
        return error("agent runs unavailable during observation");
    }
    let scope = match context(sandbox) {
        Ok(scope) if scope.submitting => scope,
        _ => return error("agentRun.start requires a Blueprint submit with spawnAgents"),
    };
    if opts.len() > 64 * 1024 {
        return error("agentRun options too large");
    }
    let Some(service) = sandbox.service() else {
        return error("host service unavailable");
    };
    let plugin = sandbox.plugin_id().to_string();
    let owner = plugin.clone();
    let opts = opts.to_string();
    let run = Arc::new(parking_lot::Mutex::new((false, None::<String>)));
    let abort = run.clone();
    let cancel_service = service.clone();
    let cancel_plugin = plugin.clone();
    let cancel_operation = scope.request.context.operation.clone();
    let run_operation = cancel_operation.clone();
    let deadline = scope.deadline;
    let operation = scope.request.context.operation.clone();
    let job = super::host_exec::start_scoped_json_job(
        &plugin,
        &operation,
        deadline,
        Arc::new(move || {
            let id = {
                let mut state = abort.lock();
                state.0 = true;
                state.1.clone()
            };
            if let Some(id) = id {
                cancel_service.agent_run_cancel_json(&cancel_plugin, &cancel_operation, &id);
            }
        }),
        serde_json::json!({"error":"launch_unconfirmed"}),
        move || {
            if run.lock().0 {
                return error("launch canceled");
            }
            let result = service.agent_run_start_json(&owner, &scope, &opts);
            let id = serde_json::from_str::<serde_json::Value>(&result)
                .ok()
                .and_then(|value| value["run"].as_str().map(str::to_owned));
            let canceled = {
                let mut state = run.lock();
                state.1 = id.clone();
                state.0
            };
            if canceled {
                if let Some(id) = id {
                    service.agent_run_cancel_json(&owner, &run_operation, &id);
                }
            }
            result
        },
    );
    match job {
        Ok(id) => serde_json::json!({"jobId":id}).to_string(),
        Err(reason) => error(&reason),
    }
}

fn wait(sandbox: &PluginSandbox, run: &str) -> String {
    let scope = match context(sandbox) {
        Ok(scope) => scope,
        Err(error) => return error,
    };
    if !crate::blueprint_step::valid_job_identity(run) {
        return error("invalid run");
    }
    let Some(service) = sandbox.service() else {
        return error("host service unavailable");
    };
    let plugin = sandbox.plugin_id().to_string();
    let owner = plugin.clone();
    let run = run.to_string();
    let deadline = scope
        .deadline
        .checked_sub(std::time::Duration::from_millis(100))
        .unwrap_or(scope.deadline);
    let operation = scope.request.context.operation.clone();
    match super::host_exec::start_scoped_json_job(
        &plugin,
        &scope.request.context.operation,
        scope.deadline,
        Arc::new(|| {}),
        serde_json::json!({"state":"running"}),
        move || service.agent_run_wait_json(&owner, &operation, &run, deadline),
    ) {
        Ok(job) => serde_json::json!({"jobId":job}).to_string(),
        Err(reason) => error(&reason),
    }
}

pub(super) fn install(host: &Object<'_>, sandbox: &Arc<PluginSandbox>) -> Result<(), VmError> {
    let sb = sandbox.clone();
    host.set(
        "agentRunStart",
        Func::new(move |opts: String| start(&sb, &opts)),
    )
    .map_err(|error| VmError::Script(error.to_string()))?;
    let sb = sandbox.clone();
    host.set(
        "agentRunWait",
        Func::new(move |run: String| wait(&sb, &run)),
    )
    .map_err(|error| VmError::Script(error.to_string()))?;
    let sb = sandbox.clone();
    host.set(
        "agentRunCancel",
        Func::new(move |run: String| {
            if !codeterm_decision::physical::effects_allowed() {
                return error("agent runs unavailable during observation");
            }
            let scope = match context(&sb) {
                Ok(scope) => scope,
                Err(error) => return error,
            };
            if !crate::blueprint_step::valid_job_identity(&run) {
                return error("invalid run");
            }
            let Some(service) = sb.service() else {
                return error("host service unavailable");
            };
            let plugin = sb.plugin_id().to_string();
            let owner = plugin.clone();
            let operation = scope.request.context.operation.clone();
            match super::host_exec::start_scoped_json_job(
                &plugin,
                &scope.request.context.operation,
                scope.deadline,
                Arc::new(|| {}),
                serde_json::json!({"state":"unknown"}),
                move || service.agent_run_cancel_json(&owner, &operation, &run),
            ) {
                Ok(job) => serde_json::json!({"jobId":job}).to_string(),
                Err(reason) => error(&reason),
            }
        }),
    )
    .map_err(|error| VmError::Script(error.to_string()))
}
