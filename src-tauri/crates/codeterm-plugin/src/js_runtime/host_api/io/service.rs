use serde_json::{json, Value};

use super::{operation, Failure, PluginSandbox, Request};

fn parsed(raw: String) -> Result<Value, Failure> {
    serde_json::from_str(&raw).map_err(|error| Failure::from(error.to_string()))
}

pub(super) fn execute(
    request: Request,
    sandbox: &PluginSandbox,
    operation: &operation::Context,
) -> Result<Value, Failure> {
    use Request::*;
    let service = sandbox.service().ok_or("host service unavailable")?;
    operation
        .check()
        .map_err(|error| Failure::from(error.to_string()))?;
    match request {
        SecretGet { name } => {
            if !sandbox.secrets_allowed() {
                return Ok(Value::Null);
            }
            Ok(json!(service.secret_get(sandbox.plugin_id(), &name)))
        }
        SecretSet { name, value } => {
            if !sandbox.secrets_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Ok(json!(false));
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            Ok(json!(service.secret_set(
                sandbox.plugin_id(),
                &name,
                &value
            )))
        }
        SecretDelete { name } => {
            if !sandbox.secrets_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Ok(json!(false));
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            Ok(json!(service.secret_delete(sandbox.plugin_id(), &name)))
        }
        MemSearch { options } => parsed(service.mem_search_json(&options.to_string())),
        AnalyticsQuery { query } => {
            if !sandbox.analytics_allowed() {
                return Err("analytics permission required".into());
            }
            parsed(service.analytics_query_json(&query.to_string()))
        }
        WorkspaceEnsure { options } => {
            if !sandbox.manage_workspaces_allowed()
                || !codeterm_decision::physical::effects_allowed()
            {
                return Err("workspace mutation permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.workspace_ensure_json(&options.to_string()))
        }
        WorkspaceTabs { workspace } => {
            if !sandbox.manage_workspaces_allowed() {
                return Err("manage_workspaces permission required".into());
            }
            parsed(service.workspace_tabs_json(&workspace))
        }
        WorkerStart { options } => {
            if !codeterm_decision::physical::effects_allowed() {
                return Err("worker mutation refused during observation".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.worker_start_json(sandbox.plugin_id(), &options.to_string()))
        }
        WorkerPoll { job } => parsed(service.worker_poll_json(&job)),
        AgentSpawn { workspace, options } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.agent_spawn_json(sandbox.plugin_id(), &workspace, &options.to_string()))
        }
        AgentSend { session, text } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.agent_send_json(sandbox.plugin_id(), &session, &text))
        }
        AgentPoll { session } => {
            if !sandbox.spawn_agents_allowed() {
                return Err("spawn_agents permission required".into());
            }
            parsed(service.agent_poll_json(&session))
        }
        AgentReap { session } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.agent_reap_json(&session))
        }
        ReceiverTarget => parsed(service.receiver_target_json(sandbox.plugin_id())),
        ReceiverTargetClear => {
            if !codeterm_decision::physical::effects_allowed() {
                return Err("receiver mutation refused during observation".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service.receiver_target_clear_json(sandbox.plugin_id()))
        }
        _ => Err("unsupported host service I/O request".into()),
    }
}
