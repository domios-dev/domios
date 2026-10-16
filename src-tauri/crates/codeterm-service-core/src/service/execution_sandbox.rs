use crate::{
    agent::registry::AgentRegistry,
    service::{ServiceContext, SpawnAgentParams},
};
use codeterm_types::plan::{ExecutionIntent, ExecutionSandboxPolicy};

fn mode_for_intent(
    registry: &AgentRegistry,
    provider: &str,
    intent: Option<&ExecutionIntent>,
    arguments: &[String],
    runtime: bool,
) -> Result<Option<codeterm_plugin::provider::ExecutionSandboxMode>, String> {
    let Some(intent) = intent.filter(|intent| **intent != ExecutionIntent::Unspecified) else {
        return Ok(None);
    };
    let capability = registry
        .with_provider(provider, |provider| provider.execution_sandbox_capability())
        .flatten()
        .unwrap_or_default();
    let mode = capability
        .resolve(
            intent,
            crate::agent::provider::current_platform_tag(),
            runtime,
        )
        .map_err(|reason| format!("Provider {provider}: {reason}"))?;
    let Some(mode) = mode else {
        return Ok(None);
    };
    capability.validate_arguments(arguments)?;
    if !arguments.is_empty() {
        return Err("Execution sandbox does not accept custom launch arguments; use typed model and effort options".into());
    }
    Ok(Some(mode))
}

pub(crate) fn preflight(
    registry: &AgentRegistry,
    provider: &str,
    params: &SpawnAgentParams,
    runtime: bool,
) -> Result<(), String> {
    mode_for_intent(
        registry,
        provider,
        params.execution_intent.as_ref(),
        params.args.as_deref().unwrap_or_default(),
        runtime,
    )?;
    Ok(())
}

pub(crate) fn resolve(
    registry: &AgentRegistry,
    provider: &str,
    intent: Option<&ExecutionIntent>,
    cwd: Option<&str>,
    arguments: &[String],
    runtime: bool,
) -> Result<Option<ExecutionSandboxPolicy>, String> {
    let Some(mode) = mode_for_intent(registry, provider, intent, arguments, runtime)? else {
        return Ok(None);
    };
    let intent = intent.ok_or("Execution sandbox intent absent")?;
    let path = std::path::Path::new(
        cwd.ok_or("Execution sandbox requires an explicit working directory")?,
    );
    if !path.is_absolute() || !path.is_dir() {
        return Err("Execution sandbox requires an existing absolute working directory".into());
    }
    let canonical = path.canonicalize().map_err(|reason| reason.to_string())?;
    let cwd = codeterm_paths::strip_verbatim_prefix(
        canonical
            .to_str()
            .ok_or("Execution sandbox working directory is not Unicode")?,
    );
    let launch_args = mode
        .launch_args
        .into_iter()
        .map(|argument| argument.replace("{cwd}", &cwd))
        .collect();
    Ok(Some(ExecutionSandboxPolicy {
        intent: intent.clone(),
        cwd,
        launch_args,
        runtime_mode: mode.runtime_mode,
        writable_roots: Vec::new(),
    }))
}

pub(crate) fn for_launch(
    svc: &ServiceContext,
    tab: &str,
    provider: &str,
    params: &SpawnAgentParams,
    runtime: bool,
) -> Result<Option<ExecutionSandboxPolicy>, String> {
    let (saved, cwd) = {
        let mux = svc.mux.read();
        let tab = mux
            .get_tab(tab)
            .ok_or("Execution sandbox tab unavailable")?;
        (
            tab.restorable_launch.execution_sandbox.clone(),
            tab.cwd.clone(),
        )
    };
    let intent = saved
        .as_ref()
        .map(|policy| &policy.intent)
        .or(params.execution_intent.as_ref());
    if saved.as_ref().is_some_and(|policy| {
        params
            .execution_intent
            .as_ref()
            .is_some_and(|intent| intent != &policy.intent)
    }) {
        return Err("Execution sandbox intent is immutable for this tab".into());
    }
    let mut policy = resolve(
        &svc.agent_registry,
        provider,
        intent,
        params.cwd.as_deref().or(cwd.as_deref()),
        params.args.as_deref().unwrap_or_default(),
        runtime,
    )?;
    if let Some(policy) = policy
        .as_mut()
        .filter(|policy| policy.intent == ExecutionIntent::Mutating)
    {
        svc.ensure_agent_pad(tab)?;
        let project = crate::service::launch_session_detection::canonical_project_for_launch(
            svc,
            tab,
            params,
            &policy.cwd,
        );
        policy.writable_roots = crate::service::launch_session_detection::host_writable_roots(
            &svc.agent_registry,
            provider,
            false,
            Some(&policy.cwd),
            project.as_deref(),
            &svc.agent_pad_data_dir,
            tab,
        )?
        .into_iter()
        .map(|root| {
            root.to_str()
                .map(codeterm_paths::strip_verbatim_prefix)
                .ok_or_else(|| "Execution sandbox root is not Unicode".to_string())
        })
        .collect::<Result<_, _>>()?;
    }
    validate_saved(saved.as_ref(), policy.as_ref())?;
    if let Some(policy) = &policy {
        let mut mux = svc.mux.write();
        let tab = mux
            .get_tab_mut(tab)
            .ok_or("Execution sandbox tab unavailable")?;
        validate_saved(
            tab.restorable_launch.execution_sandbox.as_ref(),
            Some(policy),
        )?;
        tab.restorable_launch.execution_sandbox = Some(policy.clone());
        tab.cwd = Some(policy.cwd.clone());
        tab.auto_approve = false;
    }
    Ok(policy)
}

fn validate_saved(
    saved: Option<&ExecutionSandboxPolicy>,
    resolved: Option<&ExecutionSandboxPolicy>,
) -> Result<(), String> {
    if saved.is_some() && saved != resolved {
        return Err(
            "Execution sandbox policy changed; refusing to weaken or replace the recorded policy"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn validate_runtime_settings(
    policy: &ExecutionSandboxPolicy,
    settings: &codeterm_plugin::agent_runtime::RuntimeSettings,
    cwd: &str,
) -> Result<(), String> {
    if settings.mode.as_ref() != policy.runtime_mode.as_ref()
        || settings.additional_directories != policy.writable_roots
        || settings.network_access != Some(false)
        || cwd != policy.cwd
    {
        return Err("Runtime settings would override the immutable execution sandbox".into());
    }
    Ok(())
}

pub(crate) fn validate_runtime_operation(
    svc: &ServiceContext,
    context: &codeterm_plugin::agent_runtime::RuntimeContext,
    operation: &codeterm_plugin::agent_runtime::RuntimeOperation,
) -> Result<(), String> {
    use codeterm_plugin::agent_runtime::RuntimeOperation;
    let policy = svc
        .mux
        .read()
        .get_tab(&context.tab_id)
        .and_then(|tab| tab.restorable_launch.execution_sandbox.clone());
    let Some(policy) = policy else {
        return Ok(());
    };
    validate_runtime_settings(&policy, &context.settings, &context.cwd)?;
    match operation {
        RuntimeOperation::OpenThread { settings, .. }
        | RuntimeOperation::ResumeThread { settings, .. }
        | RuntimeOperation::SubmitTurn { settings, .. } => {
            validate_runtime_settings(&policy, settings, &context.cwd)
        }
        RuntimeOperation::Control { action, value, .. } if action == "settings" => {
            let settings = serde_json::from_value(value.clone())
                .map_err(|reason| format!("Invalid sandbox runtime settings: {reason}"))?;
            validate_runtime_settings(&policy, &settings, &context.cwd)
        }
        RuntimeOperation::Control { action, value, .. } if action == "mode" => {
            if value.as_str() != policy.runtime_mode.as_deref() {
                return Err("Runtime mode cannot override the immutable execution sandbox".into());
            }
            Ok(())
        }
        RuntimeOperation::Answer { answer, .. }
            if !matches!(answer, codeterm_plugin::agent_runtime::PromptAnswer::Cancel) =>
        {
            Err(
                "Execution sandbox refuses approval escalation; use an explicit owner-run gate"
                    .into(),
            )
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
