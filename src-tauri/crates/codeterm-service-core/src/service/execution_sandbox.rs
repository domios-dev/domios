use crate::{
    agent::registry::AgentRegistry,
    service::{ServiceContext, SpawnAgentParams},
};
use codeterm_types::plan::{ExecutionIntent, ExecutionSandboxPolicy};
mod build_roots;
mod ceiling;
mod neighbor;

/// Every caller identity a delegated spawn names, plus the request's caller header, bounds its sandbox.
pub fn enforce_delegation_ceiling(
    svc: &ServiceContext,
    params: &SpawnAgentParams,
    header_caller: Option<&str>,
) -> Result<(), String> {
    let callers: Vec<_> = {
        let mux = svc.mux.read();
        [
            header_caller,
            params.spawned_by_tab_id.as_deref(),
            params.creator_tab_id.as_deref(),
            params.parent_tab_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter_map(|tab| {
            mux.get_tab(tab)
                .and_then(|caller| caller.restorable_launch.execution_sandbox.clone())
                .map(|policy| (tab.to_string(), policy))
        })
        .collect()
    };
    ceiling::check(
        params.sandbox.as_deref(),
        svc.configuration().agent.sandbox_ceiling.as_deref(),
        &callers,
        params.cwd.as_deref(),
    )
}

/// A peer cannot enforce this machine's caller ceiling, so a confined caller never delegates across the mesh.
pub fn refuse_confined_mesh_delegation(
    svc: &ServiceContext,
    callers: &[Option<&str>],
) -> Result<(), String> {
    let mux = svc.mux.read();
    match callers.iter().flatten().find(|tab| {
        mux.get_tab(tab)
            .is_some_and(|caller| caller.restorable_launch.execution_sandbox.is_some())
    }) {
        Some(tab) => Err(format!(
            "Sandboxed caller tab {tab} cannot delegate a launch to a mesh peer"
        )),
        None => Ok(()),
    }
}

pub(crate) fn requested_label(intent: &ExecutionIntent) -> &'static str {
    if *intent == ExecutionIntent::ReadOnly {
        "read-only"
    } else {
        "workspace-write"
    }
}

pub(crate) fn requested_intent(sandbox: Option<&str>) -> Result<Option<ExecutionIntent>, String> {
    match sandbox {
        None => Ok(None),
        Some("read-only") => Ok(Some(ExecutionIntent::ReadOnly)),
        Some("workspace-write") => Ok(Some(ExecutionIntent::Mutating)),
        Some(_) => Err("Sandbox must be read-only or workspace-write".into()),
    }
}

pub(crate) fn resolve_requested(
    registry: &AgentRegistry,
    provider: &str,
    params: &SpawnAgentParams,
    runtime: bool,
) -> Result<Option<ExecutionSandboxPolicy>, String> {
    resolve(
        registry,
        provider,
        requested_intent(params.sandbox.as_deref())?.as_ref(),
        params.cwd.as_deref(),
        params.args.as_deref().unwrap_or_default(),
        runtime,
    )
}

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
        return Err(format!(
            "Provider {provider}: requested sandbox is unsupported"
        ));
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
        requested_intent(params.sandbox.as_deref())?.as_ref(),
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
    let intent = intent.ok_or("Execution sandbox mode absent")?;
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
        environment: Default::default(),
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
    let requested = requested_intent(params.sandbox.as_deref())?;
    let intent = saved
        .as_ref()
        .map(|policy| &policy.intent)
        .or(requested.as_ref());
    if saved.as_ref().is_some_and(|policy| {
        requested
            .as_ref()
            .is_some_and(|intent| intent != &policy.intent)
    }) {
        return Err("Execution sandbox is immutable for this tab".into());
    }
    let mut policy = resolve(
        &svc.agent_registry,
        provider,
        intent,
        params.cwd.as_deref().or(cwd.as_deref()),
        params.args.as_deref().unwrap_or_default(),
        runtime,
    )?;
    let neighbor = neighbor::apply_roots(svc, tab, provider, runtime, &mut policy)?;
    if let (Some(saved), Some(resolved)) = (saved.as_ref(), policy.as_mut()) {
        let allowed_roots = neighbor.then(|| resolved.writable_roots.clone());
        *resolved = restore_recorded(saved, resolved, |root| {
            build_roots::restore_recorded_root(root, &saved.cwd);
            std::path::Path::new(root)
                .canonicalize()
                .ok()
                .and_then(|path| path.to_str().map(codeterm_paths::strip_verbatim_prefix))
                .as_deref()
                == Some(root)
        })?;
        if let Some(allowed) = allowed_roots {
            resolved
                .writable_roots
                .retain(|root| allowed.contains(root));
        }
    }
    if let Some(policy) = policy
        .as_mut()
        .filter(|policy| !neighbor && saved.is_none() && policy.intent == ExecutionIntent::Mutating)
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
        let build = build_roots::for_lane(&policy.cwd)?;
        for root in build.writable_roots {
            if !policy.writable_roots.contains(&root) {
                policy.writable_roots.push(root);
            }
        }
        policy.environment = build.environment;
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

fn restore_recorded(
    saved: &ExecutionSandboxPolicy,
    resolved: &ExecutionSandboxPolicy,
    mut resolves_unchanged: impl FnMut(&str) -> bool,
) -> Result<ExecutionSandboxPolicy, String> {
    let mut confinement = saved.clone();
    confinement.cwd = resolved.cwd.clone();
    confinement.intent = resolved.intent.clone();
    confinement.launch_args = resolved.launch_args.clone();
    confinement.runtime_mode = resolved.runtime_mode.clone();
    validate_saved(Some(saved), Some(&confinement))?;
    confinement.writable_roots.retain(|root| {
        let retained = resolves_unchanged(root);
        if !retained {
            log::warn!("[execution-sandbox] dropping unavailable or redirected recorded writable root: {root}");
        }
        retained
    });
    Ok(confinement)
}

fn validate_saved(
    saved: Option<&ExecutionSandboxPolicy>,
    resolved: Option<&ExecutionSandboxPolicy>,
) -> Result<(), String> {
    if saved.is_some() && saved != resolved {
        if let (Some(saved), Some(resolved)) = (saved, resolved) {
            if saved.cwd != resolved.cwd {
                return Err(format!(
                    "Execution sandbox working root changed: {} -> {}",
                    saved.cwd, resolved.cwd
                ));
            }
            for root in &resolved.writable_roots {
                if !saved.writable_roots.contains(root) {
                    return Err(format!(
                        "Execution sandbox refuses new writable root: {root}"
                    ));
                }
            }
        }
        if let (Some(saved), Some(resolved)) = (saved, resolved) {
            let mut narrowed = saved.clone();
            narrowed.writable_roots = resolved.writable_roots.clone();
            if &narrowed == resolved {
                return Ok(());
            }
        }
        return Err(
            "Execution sandbox policy changed; refusing to weaken or replace the recorded policy"
                .into(),
        );
    }
    Ok(())
}

pub(crate) fn environment(
    svc: &ServiceContext,
    tab: &str,
) -> std::collections::BTreeMap<String, String> {
    svc.mux
        .read()
        .get_tab(tab)
        .and_then(|tab| tab.restorable_launch.execution_sandbox.as_ref())
        .map(|policy| policy.environment.clone())
        .unwrap_or_default()
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
