use codeterm_plugin::view_host::{
    ViewCallContext, ViewCallOrigin, ViewProvider, ViewRegistryHandle,
};
use codeterm_service_core::{multiplexer::types::TabBacking, service::ServiceState};
use serde_json::Value;
use std::sync::Arc;

fn remote_binding(
    svc: &ServiceState,
    tab_id: &str,
    method: &str,
) -> Result<(Arc<dyn ViewProvider>, ViewCallContext, String), String> {
    let (plugin, session, mode, mut context) = {
        let mux = svc.mux.read();
        let tab = mux.get_tab(tab_id).ok_or("remote_task_scope_invalid")?;
        let TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } = &tab.backing
        else {
            return Err("remote_task_scope_invalid".into());
        };
        let mode = tab
            .restorable_launch
            .execution_mode
            .clone()
            .ok_or("remote_task_scope_invalid")?;
        if session_id.is_empty() {
            return Err("remote_task_scope_invalid".into());
        }
        let mut context = ViewCallContext::panel(plugin_id);
        context.origin = ViewCallOrigin::RemoteTask;
        context.tab = Some(tab_id.into());
        context.owner = Some(tab_id.into());
        context.workspace = mux.find_workspace_for_tab(tab_id);
        context.team = tab.team_id.clone();
        (plugin_id.clone(), session_id.clone(), mode, context)
    };
    let capability = svc
        .chatbackend_registry
        .descriptor(&plugin)
        .and_then(|value| value.remote_task)
        .ok_or("remote_task_scope_invalid")?;
    if capability.mode != mode {
        return Err("remote_task_scope_invalid".into());
    }
    if method == "submit"
        && matches!(
            capability.availability,
            codeterm_plugin::remote_tasks::RemoteTaskAvailability::Unavailable { .. }
        )
    {
        return Err("remote_task_unavailable".into());
    }
    let provider = svc
        .ext::<ViewRegistryHandle>()
        .get(&plugin)
        .ok_or("remote_task_scope_invalid")?;
    context.plugin = plugin;
    Ok((provider, context, session))
}

fn remote_args(method: &str, args: Value, session: &str) -> Result<Value, String> {
    let mut args = match args {
        Value::Null => serde_json::Map::new(),
        Value::Object(args) => args,
        _ => return Err("remote_task_request_invalid".into()),
    };
    match method {
        "read" | "refresh" if args.is_empty() => {}
        "submit"
            if args.len() == 2
                && args
                    .get("prompt")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty() && v.len() <= 32768)
                && args.get("branch").and_then(Value::as_str).is_some_and(|v| {
                    !v.trim().is_empty() && v.len() <= 1024 && !v.chars().any(char::is_control)
                }) => {}
        _ => return Err("remote_task_request_invalid".into()),
    }
    args.insert("sessionId".into(), Value::String(session.into()));
    Ok(Value::Object(args))
}

pub async fn remote_task_call(
    svc: &ServiceState,
    tab_id: &str,
    expected_plugin: &str,
    expected_session: &str,
    method: &str,
    args: Value,
) -> Result<Value, String> {
    let (provider, context, session) = remote_binding(svc, tab_id, method)?;
    if context.plugin != expected_plugin || session != expected_session {
        return Err("remote_task_scope_invalid".into());
    }
    let args = remote_args(method, args, &session)?;
    let result = provider
        .call(method, args, &context)
        .await
        .map_err(|error| error.to_string());
    let (current, current_context, current_session) = remote_binding(svc, tab_id, method)?;
    if !Arc::ptr_eq(&provider, &current) || context != current_context || session != current_session
    {
        return Err("remote_task_scope_invalid".into());
    }
    result
}

fn recording_args(args: Value) -> Result<Value, String> {
    let Some(object) = args.as_object() else {
        return Err("local_recording_request_invalid".into());
    };
    if object.len() != 2
        || !["inputs", "microphones"].iter().all(|key| {
            object
                .get(*key)
                .and_then(Value::as_array)
                .is_some_and(|values| values.len() <= 256)
        })
        || serde_json::to_vec(&args)
            .map_err(|_| "local_recording_request_invalid")?
            .len()
            > 65536
    {
        return Err("local_recording_request_invalid".into());
    }
    Ok(args)
}

pub async fn recording_suggestion(
    svc: &ServiceState,
    plugin: &str,
    args: Value,
) -> Result<Value, String> {
    let args = recording_args(args)?;
    let status = svc.local_recording_status(plugin)?;
    if !status.granted {
        return Err("local_recording_grant_required".into());
    }
    let provider = svc
        .ext::<ViewRegistryHandle>()
        .get(plugin)
        .ok_or("local_recording_not_registered")?;
    let mut context = ViewCallContext::panel(plugin);
    context.origin = ViewCallOrigin::RecordingSetup;
    let result = provider
        .call("selectRecordingDevices", args, &context)
        .await
        .map_err(|error| error.to_string());
    let current_status = svc.local_recording_status(plugin)?;
    let current = svc
        .ext::<ViewRegistryHandle>()
        .get(plugin)
        .ok_or("local_recording_not_registered")?;
    if !current_status.granted
        || status.generation != current_status.generation
        || !Arc::ptr_eq(&provider, &current)
    {
        return Err("local_recording_configuration_changed".into());
    }
    result
}

#[cfg(test)]
mod tests;
