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
    let service = || sandbox.service().ok_or("host service unavailable");
    operation
        .check()
        .map_err(|error| Failure::from(error.to_string()))?;
    match request {
        GraphEventEmit { event } => {
            if !sandbox.graph_events_allowed() {
                return Err("host.events.emit: graph_events permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| error.to_string())?;
            parsed(service()?.emit_graph_event_json(sandbox.plugin_id(), &event.to_string()))
        }
        Progress { options } => {
            operation
                .begin_effect()
                .map_err(|error| error.to_string())?;
            Ok(json!(service()?.progress_raw_json(
                sandbox.plugin_id(),
                &options.to_string()
            )))
        }
        LocalRecordingStatus => parsed(service()?.local_recording_status_json(sandbox.plugin_id())),
        LocalRecordingSetEnabled { enabled } => {
            if enabled && !sandbox.local_recording_allowed() {
                return Err("local_recording_grant_required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| error.to_string())?;
            parsed(service()?.local_recording_enabled_json(sandbox.plugin_id(), enabled))
        }
        ReceiverStatusReport => {
            if !sandbox.local_recording_allowed() {
                return Err("local_recording_grant_required".into());
            }
            let query = sandbox
                .receiver_status_query()
                .ok_or("receiver_status_not_declared")?;
            parsed(service()?.receiver_status_report_json(sandbox.plugin_id(), query))
        }
        SecretGet { name } => {
            if !sandbox.secrets_allowed() {
                return Ok(Value::Null);
            }
            if sandbox.cloud_machine_provider {
                let key_name = format!("{}-api-key", sandbox.plugin_id());
                let scoped_name = name
                    .strip_prefix(&key_name)
                    .and_then(|suffix| suffix.strip_prefix('-'))
                    .is_some_and(|suffix| {
                        !suffix.is_empty()
                            && suffix
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    });
                if name != key_name && !scoped_name {
                    return Ok(Value::Null);
                }
                return service()?
                    .cloud_provider_key(&name)
                    .map(|key| json!(key))
                    .map_err(Into::into);
            }
            let Some(name) = crate::provider_account::secret_name(sandbox.plugin_id(), &name)
            else {
                return Ok(Value::Null);
            };
            Ok(json!(service()?.secret_get(sandbox.plugin_id(), &name)))
        }
        SecretSet { name, value } => {
            let Some(name) = crate::provider_account::secret_name(sandbox.plugin_id(), &name)
            else {
                return Ok(json!(false));
            };
            if !sandbox.secrets_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Ok(json!(false));
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            Ok(json!(service()?.secret_set(
                sandbox.plugin_id(),
                &name,
                &value
            )))
        }
        SecretDelete { name } => {
            let Some(name) = crate::provider_account::secret_name(sandbox.plugin_id(), &name)
            else {
                return Ok(json!(false));
            };
            if !sandbox.secrets_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Ok(json!(false));
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            Ok(json!(service()?.secret_delete(sandbox.plugin_id(), &name)))
        }
        MemSearch { options } => parsed(service()?.mem_search_json(&options.to_string())),
        AnalyticsQuery { query } => {
            if !sandbox.analytics_allowed() {
                return Err("analytics permission required".into());
            }
            parsed(service()?.analytics_query_json(&query.to_string()))
        }
        WorkspaceEnsure { options } => {
            if !sandbox.manage_workspaces_allowed()
                || !codeterm_decision::physical::effects_allowed()
            {
                return Err("host.workspace: manage_workspaces permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.workspace_ensure_json(&options.to_string()))
        }
        WorkspaceTabs { workspace } => {
            if !sandbox.manage_workspaces_allowed() {
                return Err("manage_workspaces permission required".into());
            }
            parsed(service()?.workspace_tabs_json(&workspace))
        }
        WorkerStart { options } => {
            if !codeterm_decision::physical::effects_allowed() {
                return Err("worker mutation refused during observation".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.worker_start_json(sandbox.plugin_id(), &options.to_string()))
        }
        WorkerPoll { job } => parsed(service()?.worker_poll_json(&job)),
        AgentSpawn { workspace, options } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.agent_spawn_json(
                sandbox.plugin_id(),
                &workspace,
                &options.to_string(),
            ))
        }
        AgentSend { session, text } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.agent_send_json(sandbox.plugin_id(), &session, &text))
        }
        AgentPoll { session } => {
            if !sandbox.spawn_agents_allowed() {
                return Err("spawn_agents permission required".into());
            }
            parsed(service()?.agent_poll_json(&session))
        }
        AgentReap { session } => {
            if !sandbox.spawn_agents_allowed() || !codeterm_decision::physical::effects_allowed() {
                return Err("spawn_agents permission required".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.agent_reap_json(&session))
        }
        ReceiverTarget => parsed(service()?.receiver_target_json(sandbox.plugin_id())),
        ReceiverTargetClear => {
            if !codeterm_decision::physical::effects_allowed() {
                return Err("receiver mutation refused during observation".into());
            }
            operation
                .begin_effect()
                .map_err(|error| Failure::from(error.to_string()))?;
            parsed(service()?.receiver_target_clear_json(sandbox.plugin_id()))
        }
        _ => Err("unsupported host service I/O request".into()),
    }
}
